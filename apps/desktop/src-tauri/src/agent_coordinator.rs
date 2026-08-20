use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use hal100_core::{
    AGENT_CAPABILITY_COUNT, AgentCapabilityEffect, AgentCapabilityId, AgentCapabilityRegistry,
    AgentCapabilitySet,
};
use hal100_protocol::{
    AGENT_RPC_MAX_ACTION_PLANS, AGENT_RPC_MAX_REQUIRED_TOOLS, AgentActionKind, AgentActionPlan,
    AgentProviderProtocol, AgentRunCompletedPayload, AgentRunStartPayload, AgentToolEvent,
};
use thiserror::Error;

const MAX_PROMPT_BYTES: usize = 4 * 1024;
const MAX_ANSWER_BYTES: usize = 64 * 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub(super) enum AgentCoordinationError {
    #[error("HAL100 Agent 请求不能为空，且最多为 4096 个 UTF-8 字节")]
    InvalidPrompt,
    #[error("该请求超出 HAL100、本地模型和推理环境的职责范围")]
    OutsideDomain,
    #[error("HAL100 Agent 私有协议校验失败")]
    InvalidProtocol,
    #[error("HAL100 Agent 每项任务最多只能生成一个写操作计划，请拆分任务")]
    MultipleActionPlans,
    #[error("HAL100 Agent 单项任务需要的工具过多，请拆分任务")]
    TooManyCapabilities,
    #[error("HAL100 Agent 未按要求完成必需工具：{0}")]
    RequiredToolMissing(&'static str),
    #[error("HAL100 Agent 回答超过安全长度限制")]
    AnswerTooLarge,
    #[error("当前没有可取消的 HAL100 Agent 任务")]
    NoActiveRun,
    #[error("HAL100 Agent 任务状态不可用")]
    StateUnavailable,
}

#[derive(Clone, Default)]
pub(super) struct AgentRunRequirements {
    capabilities: AgentCapabilitySet,
}

impl AgentRunRequirements {
    pub(super) fn for_prompt(prompt: &str) -> Self {
        let model_download = prompt_requires_model_download_plan(prompt);
        let model_start = !model_download && prompt_requires_model_start_plan(prompt);
        let model_removal = prompt_requires_model_removal_plan(prompt);
        let engine_install = prompt_requires_engine_install_plan(prompt);
        let engine_remove = prompt_requires_engine_remove_plan(prompt);
        let opencode_configuration = prompt_requires_opencode_configuration_plan(prompt);
        let has_explicit_action = model_start
            || model_removal
            || engine_install
            || engine_remove
            || opencode_configuration
            || model_download;
        let diagnostic_repair =
            !has_explicit_action && prompt_requires_diagnostic_repair_plan(prompt);

        let mut requirements = Self::default();
        for (required, capability) in [
            (
                prompt_requires_system_summary(prompt),
                AgentCapabilityId::InspectSystemSummary,
            ),
            (
                prompt_requires_runtime_catalog(prompt),
                AgentCapabilityId::InspectRuntimeCatalog,
            ),
            (model_start, AgentCapabilityId::PlanModelStart),
            (model_removal, AgentCapabilityId::PlanModelRemoval),
            (
                prompt_requires_environment_diagnostics(prompt),
                AgentCapabilityId::InspectEnvironmentDiagnostics,
            ),
            (diagnostic_repair, AgentCapabilityId::PlanDiagnosticRepair),
            (engine_install, AgentCapabilityId::PlanEngineInstall),
            (engine_remove, AgentCapabilityId::PlanEngineRemove),
            (
                prompt_requires_opencode_status(prompt),
                AgentCapabilityId::InspectOpenCodeStatus,
            ),
            (
                opencode_configuration,
                AgentCapabilityId::PlanOpenCodeConfiguration,
            ),
            (
                prompt_requires_model_catalog_search(prompt),
                AgentCapabilityId::SearchModelCatalog,
            ),
            (
                prompt_requires_model_repository_inspection(prompt),
                AgentCapabilityId::InspectModelRepository,
            ),
            (model_download, AgentCapabilityId::PlanModelDownload),
        ] {
            if required {
                requirements.capabilities.require(capability);
            }
        }
        requirements
    }

    #[cfg(test)]
    pub(super) fn requiring(capabilities: impl IntoIterator<Item = AgentCapabilityId>) -> Self {
        let mut requirements = Self::default();
        for capability in capabilities {
            requirements.capabilities.require(capability);
        }
        requirements
    }

    pub(super) fn requires(&self, capability: AgentCapabilityId) -> bool {
        self.capabilities.contains(capability)
    }

    pub(super) fn validate(&self) -> Result<(), AgentCoordinationError> {
        let action_plan_count = self
            .iter()
            .filter(|capability| {
                AgentCapabilityRegistry::descriptor(*capability).effect
                    == AgentCapabilityEffect::ActionPlan
            })
            .count();
        if action_plan_count > AGENT_RPC_MAX_ACTION_PLANS {
            return Err(AgentCoordinationError::MultipleActionPlans);
        }
        if self.capabilities.len() > AGENT_RPC_MAX_REQUIRED_TOOLS {
            return Err(AgentCoordinationError::TooManyCapabilities);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.capabilities.len()
    }

    fn iter(&self) -> impl Iterator<Item = AgentCapabilityId> + '_ {
        self.capabilities.iter()
    }

    pub(super) fn to_rpc_v4(
        &self,
        prompt: &str,
        gateway_base_url: &str,
        api_key: &str,
        model_id: &str,
        provider_protocol: AgentProviderProtocol,
    ) -> AgentRunStartPayload {
        AgentRunStartPayload {
            prompt: prompt.to_owned(),
            required_tools: self
                .iter()
                .map(|capability| {
                    AgentCapabilityRegistry::descriptor(capability)
                        .tool_name
                        .to_owned()
                })
                .collect(),
            gateway_base_url: gateway_base_url.to_owned(),
            api_key: api_key.to_owned(),
            model_id: model_id.to_owned(),
            provider_protocol,
        }
    }
}

#[derive(Clone, Default)]
pub(super) struct AgentRunRegistry {
    active: Arc<Mutex<Option<ActiveRun>>>,
}

struct ActiveRun {
    run_id: String,
    cancellation: Arc<AtomicBool>,
}

pub(super) struct AgentRunLease {
    registry: AgentRunRegistry,
    run_id: String,
    cancellation: Arc<AtomicBool>,
}

pub(super) struct AgentRunSnapshot {
    pub(super) run_id: String,
    pub(super) cancellation_requested: bool,
}

impl AgentRunRegistry {
    pub(super) fn begin(&self, run_id: String) -> Result<AgentRunLease, AgentCoordinationError> {
        let cancellation = Arc::new(AtomicBool::new(false));
        let mut active = self
            .active
            .lock()
            .map_err(|_| AgentCoordinationError::StateUnavailable)?;
        if active.is_some() {
            return Err(AgentCoordinationError::StateUnavailable);
        }
        *active = Some(ActiveRun {
            run_id: run_id.clone(),
            cancellation: cancellation.clone(),
        });
        Ok(AgentRunLease {
            registry: self.clone(),
            run_id,
            cancellation,
        })
    }

    pub(super) fn snapshot(&self) -> Result<Option<AgentRunSnapshot>, AgentCoordinationError> {
        let active = self
            .active
            .lock()
            .map_err(|_| AgentCoordinationError::StateUnavailable)?;
        Ok(active.as_ref().map(|run| AgentRunSnapshot {
            run_id: run.run_id.clone(),
            cancellation_requested: run.cancellation.load(Ordering::Acquire),
        }))
    }

    pub(super) fn cancel(&self) -> Result<(), AgentCoordinationError> {
        let active = self
            .active
            .lock()
            .map_err(|_| AgentCoordinationError::StateUnavailable)?;
        let run = active.as_ref().ok_or(AgentCoordinationError::NoActiveRun)?;
        run.cancellation.store(true, Ordering::Release);
        Ok(())
    }

    fn finish(&self, run_id: &str) {
        if let Ok(mut active) = self.active.lock()
            && active.as_ref().is_some_and(|run| run.run_id == run_id)
        {
            active.take();
        }
    }
}

impl AgentRunLease {
    pub(super) fn cancellation(&self) -> Arc<AtomicBool> {
        self.cancellation.clone()
    }
}

impl Drop for AgentRunLease {
    fn drop(&mut self) {
        self.registry.finish(&self.run_id);
    }
}

pub(super) fn validate_completion(
    run_id: &str,
    requirements: &AgentRunRequirements,
    diagnostic_repair_available: bool,
    completed: &AgentRunCompletedPayload,
    tool_events: &[AgentToolEvent],
    action_plans: &[AgentActionPlan],
) -> Result<(), AgentCoordinationError> {
    let expected_action_kind = if requirements.requires(AgentCapabilityId::PlanModelStart) {
        Some(AgentActionKind::StartOrSwitchModel)
    } else if requirements.requires(AgentCapabilityId::PlanModelRemoval) {
        Some(AgentActionKind::RemoveModel)
    } else if requirements.requires(AgentCapabilityId::PlanEngineInstall) {
        Some(AgentActionKind::InstallLlamaCpp)
    } else if requirements.requires(AgentCapabilityId::PlanEngineRemove) {
        Some(AgentActionKind::RemoveLlamaCpp)
    } else if requirements.requires(AgentCapabilityId::PlanOpenCodeConfiguration) {
        Some(AgentActionKind::ConfigureOpenCode)
    } else if requirements.requires(AgentCapabilityId::PlanModelDownload) {
        Some(AgentActionKind::DownloadModel)
    } else {
        None
    };
    let diagnostic_action_kind_is_allowed = |kind: AgentActionKind| {
        matches!(
            kind,
            AgentActionKind::InstallLlamaCpp
                | AgentActionKind::ConfigureOpenCode
                | AgentActionKind::RemoveModel
        )
    };
    if completed.run_id != run_id
        || completed.registered_tool_count != AGENT_CAPABILITY_COUNT
        || completed.completed_tool_calls as usize != completed.tool_names.len()
        || completed.completed_tool_calls as usize != tool_events.len()
        || completed
            .tool_names
            .iter()
            .any(|name| AgentCapabilityRegistry::by_tool_name(name).is_none())
        || completed.answer.trim().is_empty()
        || action_plans.len() > 1
        || expected_action_kind
            .is_some_and(|kind| action_plans.len() != 1 || action_plans[0].action_kind != kind)
        || (requirements.requires(AgentCapabilityId::PlanDiagnosticRepair)
            && diagnostic_repair_available
            && (action_plans.len() != 1
                || !diagnostic_action_kind_is_allowed(action_plans[0].action_kind)))
        || (requirements.requires(AgentCapabilityId::PlanDiagnosticRepair)
            && !diagnostic_repair_available
            && !action_plans.is_empty())
    {
        return Err(AgentCoordinationError::InvalidProtocol);
    }
    for capability in requirements.iter() {
        if capability == AgentCapabilityId::PlanDiagnosticRepair && !diagnostic_repair_available {
            continue;
        }
        let tool_name = AgentCapabilityRegistry::descriptor(capability).tool_name;
        if !tool_events.iter().any(|event| event.tool_name == tool_name) {
            return Err(AgentCoordinationError::RequiredToolMissing(tool_name));
        }
    }
    if completed.answer.len() > MAX_ANSWER_BYTES {
        return Err(AgentCoordinationError::AnswerTooLarge);
    }
    Ok(())
}

pub(super) fn validate_prompt(prompt: &str) -> Result<String, AgentCoordinationError> {
    let prompt = prompt.trim();
    if prompt.is_empty()
        || prompt.len() > MAX_PROMPT_BYTES
        || prompt
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(AgentCoordinationError::InvalidPrompt);
    }
    let normalized = prompt.to_lowercase();
    const DOMAIN_MARKERS: &[&str] = &[
        "hal100", "本地", "模型", "推理", "引擎", "后端", "配置", "电脑", "mac", "硬件", "内存",
        "cpu", "芯片", "安装", "卸载", "删除", "下载", "切换", "llama", "vllm", "opencode", "api",
        "token",
    ];
    if !DOMAIN_MARKERS
        .iter()
        .any(|marker| normalized.contains(marker))
    {
        return Err(AgentCoordinationError::OutsideDomain);
    }
    Ok(prompt.to_owned())
}

pub(super) fn prompt_requires_system_summary(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    [
        "检测",
        "电脑配置",
        "硬件",
        "内存",
        "cpu",
        "芯片",
        "适合运行",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

pub(super) fn prompt_requires_runtime_catalog(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    [
        "模型列表",
        "可用模型",
        "有哪些模型",
        "当前模型",
        "活动模型",
        "引擎状态",
        "后端状态",
        "运行状态",
        "是否安装",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

pub(super) fn prompt_requires_environment_diagnostics(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    [
        "全面诊断",
        "环境诊断",
        "健康检查",
        "环境健康",
        "排查故障",
        "检查并修复",
        "诊断并修复",
        "修复问题",
        "修复故障",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

pub(super) fn prompt_requires_diagnostic_repair_plan(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    [
        "检查并修复",
        "诊断并修复",
        "自动修复",
        "修复问题",
        "修复故障",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

pub(super) fn prompt_requires_model_start_plan(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    let refers_to_model = ["模型", "qwen", "gguf", "llama"]
        .iter()
        .any(|marker| normalized.contains(marker));
    let requests_start_or_switch = ["启动", "切换", "换成", "改用", "设为当前"]
        .iter()
        .any(|marker| normalized.contains(marker));
    refers_to_model && requests_start_or_switch
}

pub(super) fn prompt_requires_model_catalog_search(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    let refers_to_model = [
        "模型",
        "qwen",
        "gguf",
        "hugging face",
        "huggingface",
        "modelscope",
        "魔搭",
    ]
    .iter()
    .any(|marker| normalized.contains(marker));
    refers_to_model
        && [
            "搜索",
            "查找",
            "找一个",
            "找一下",
            "模型目录",
            "模型仓库",
            "下载",
        ]
        .iter()
        .any(|marker| normalized.contains(marker))
}

pub(super) fn prompt_requires_model_repository_inspection(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    prompt_requires_model_download_plan(prompt)
        || [
            "仓库详情",
            "检查仓库",
            "查看仓库",
            "gguf 文件",
            "gguf文件",
            "量化版本",
        ]
        .iter()
        .any(|marker| normalized.contains(marker))
}

pub(super) fn prompt_requires_model_download_plan(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    let refers_to_model = [
        "模型",
        "qwen",
        "gguf",
        "hugging face",
        "huggingface",
        "modelscope",
        "魔搭",
    ]
    .iter()
    .any(|marker| normalized.contains(marker));
    let manages_existing_download = ["下载状态", "暂停下载", "继续下载", "恢复下载", "取消下载"]
        .iter()
        .any(|marker| normalized.contains(marker));
    refers_to_model
        && !manages_existing_download
        && ["下载", "获取模型", "加入模型库"]
            .iter()
            .any(|marker| normalized.contains(marker))
}

pub(super) fn prompt_requires_model_removal_plan(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    let refers_to_model = ["模型", "qwen", "gguf", "llama"]
        .iter()
        .any(|marker| normalized.contains(marker));
    let requests_removal = ["删除", "移除", "卸载", "移出模型库", "清理索引"]
        .iter()
        .any(|marker| normalized.contains(marker));
    refers_to_model && requests_removal && !prompt_refers_to_llama_cpp_engine(prompt)
}

fn prompt_refers_to_llama_cpp_engine(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    normalized.contains("llama.cpp")
        || normalized.contains("llama cpp")
        || normalized.contains("推理引擎")
        || normalized.contains("本地引擎")
}

pub(super) fn prompt_requires_engine_install_plan(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    prompt_refers_to_llama_cpp_engine(prompt)
        && ["安装", "部署", "装上"]
            .iter()
            .any(|marker| normalized.contains(marker))
        && !["卸载", "移除", "删除"]
            .iter()
            .any(|marker| normalized.contains(marker))
}

pub(super) fn prompt_requires_engine_remove_plan(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    prompt_refers_to_llama_cpp_engine(prompt)
        && ["卸载", "移除", "删除引擎"]
            .iter()
            .any(|marker| normalized.contains(marker))
}

pub(super) fn prompt_requires_opencode_status(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    normalized.contains("opencode")
        && ["状态", "检测", "检查", "配置", "接入", "连接"]
            .iter()
            .any(|marker| normalized.contains(marker))
}

pub(super) fn prompt_requires_opencode_configuration_plan(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    if !normalized.contains("opencode") {
        return false;
    }
    let explicit_change = [
        "帮我配置",
        "生成配置",
        "配置计划",
        "重新配置",
        "写入配置",
        "接入 hal100",
        "接入hal100",
        "连接到 hal100",
        "连接到hal100",
        "设置 opencode",
        "设置opencode",
    ]
    .iter()
    .any(|marker| normalized.contains(marker));
    let inspection_only = ["查看", "解释", "是什么", "检查配置", "检测配置", "配置状态"]
        .iter()
        .any(|marker| normalized.contains(marker));
    explicit_change || (!inspection_only && normalized.trim().starts_with("配置 opencode"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_run_lifecycle_is_exact_and_cancellation_is_observable() {
        let registry = AgentRunRegistry::default();
        let lease = registry.begin("run-a".to_owned()).expect("begin run");
        assert_eq!(
            registry
                .snapshot()
                .expect("snapshot")
                .expect("active")
                .run_id,
            "run-a"
        );
        registry.cancel().expect("cancel run");
        assert!(lease.cancellation().load(Ordering::Acquire));
        assert!(
            registry
                .snapshot()
                .expect("snapshot")
                .expect("active")
                .cancellation_requested
        );
        drop(lease);
        assert!(registry.snapshot().expect("snapshot").is_none());
    }

    #[test]
    fn registry_rejects_overlap_and_cancel_without_an_active_run() {
        let registry = AgentRunRegistry::default();
        assert_eq!(registry.cancel(), Err(AgentCoordinationError::NoActiveRun));
        let _lease = registry.begin("run-a".to_owned()).expect("begin run");
        assert!(matches!(
            registry.begin("run-b".to_owned()),
            Err(AgentCoordinationError::StateUnavailable)
        ));
    }
}
