use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use hal100_core::{
    AGENT_CAPABILITY_COUNT, AgentCapabilityEffect, AgentCapabilityId, AgentCapabilityRegistry,
    AgentCapabilitySet, AgentTaskIntentRouter, AgentTaskSpec, AgentTaskTargetKind,
    ExternalAgentIntegrationId, ExternalAgentIntegrationRegistry,
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
    external_agent_target: Option<ExternalAgentIntegrationId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct AgentRunCapacity {
    context_window_tokens: u32,
    max_output_tokens: u32,
}

impl AgentRunCapacity {
    pub(super) const fn new(context_window_tokens: u32, max_output_tokens: u32) -> Self {
        Self {
            context_window_tokens,
            max_output_tokens,
        }
    }
}

impl AgentRunRequirements {
    pub(super) fn for_prompt(prompt: &str) -> Self {
        if AgentTaskIntentRouter::is_explanation_only(prompt) {
            return Self::default();
        }
        let model_download = prompt_requires_model_download_plan(prompt);
        let model_stop = !model_download && prompt_requires_model_stop_plan(prompt);
        let model_start =
            !model_download && !model_stop && prompt_requires_model_start_plan(prompt);
        let model_removal = prompt_requires_model_removal_plan(prompt);
        let engine_install = prompt_requires_engine_install_plan(prompt);
        let engine_remove = prompt_requires_engine_remove_plan(prompt);
        let external_agent_target = prompt_external_agent_target(prompt);
        let external_agent_installation = external_agent_target.is_some()
            && prompt_requires_external_agent_installation_plan(prompt);
        let managed_external_agent_removal = external_agent_target.is_some()
            && prompt_requires_managed_external_agent_removal_plan(prompt);
        let external_agent_configuration = !external_agent_installation
            && !managed_external_agent_removal
            && external_agent_target.is_some()
            && prompt_requires_external_agent_configuration_plan(prompt);
        let external_agent_disconnection = !managed_external_agent_removal
            && external_agent_target.is_some()
            && prompt_requires_external_agent_disconnection_plan(prompt);
        let has_explicit_action = model_start
            || model_stop
            || model_removal
            || engine_install
            || engine_remove
            || external_agent_configuration
            || external_agent_disconnection
            || external_agent_installation
            || managed_external_agent_removal
            || model_download;
        let diagnostic_repair =
            !has_explicit_action && prompt_requires_diagnostic_repair_plan(prompt);

        let mut requirements = Self {
            capabilities: AgentCapabilitySet::default(),
            external_agent_target,
        };
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
            (model_stop, AgentCapabilityId::PlanModelStop),
            (model_removal, AgentCapabilityId::PlanModelRemoval),
            (
                prompt_requires_environment_diagnostics(prompt),
                AgentCapabilityId::InspectEnvironmentDiagnostics,
            ),
            (diagnostic_repair, AgentCapabilityId::PlanDiagnosticRepair),
            (engine_install, AgentCapabilityId::PlanEngineInstall),
            (engine_remove, AgentCapabilityId::PlanEngineRemove),
            (
                external_agent_target.is_some(),
                AgentCapabilityId::InspectExternalAgent,
            ),
            (
                external_agent_configuration,
                AgentCapabilityId::PlanExternalAgentConfiguration,
            ),
            (
                external_agent_disconnection,
                AgentCapabilityId::PlanExternalAgentDisconnection,
            ),
            (
                external_agent_installation,
                AgentCapabilityId::PlanExternalAgentInstallation,
            ),
            (
                managed_external_agent_removal,
                AgentCapabilityId::PlanManagedExternalAgentRemoval,
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
            (
                prompt_requires_operational_history(prompt),
                AgentCapabilityId::InspectOperationalHistory,
            ),
            (
                prompt_requires_operational_health_observation(prompt),
                AgentCapabilityId::ObserveOperationalHealth,
            ),
        ] {
            if required {
                requirements.capabilities.require(capability);
            }
        }
        requirements
    }

    pub(super) fn for_task_spec(spec: &AgentTaskSpec) -> Result<Self, AgentCoordinationError> {
        let external_agent_target = if spec.target().kind() == AgentTaskTargetKind::ExternalAgent {
            Some(
                spec.target()
                    .resource_id()
                    .and_then(ExternalAgentIntegrationRegistry::by_integration_id)
                    .ok_or(AgentCoordinationError::InvalidProtocol)?
                    .id,
            )
        } else {
            None
        };
        let requirements = Self {
            capabilities: spec.required_capabilities(),
            external_agent_target,
        };
        requirements.validate()?;
        Ok(requirements)
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

    pub(super) fn external_agent_target(&self) -> Option<ExternalAgentIntegrationId> {
        self.external_agent_target
    }

    pub(super) fn is_empty(&self) -> bool {
        self.capabilities.is_empty()
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

    pub(super) fn len(&self) -> usize {
        self.capabilities.len()
    }

    fn iter(&self) -> impl Iterator<Item = AgentCapabilityId> + '_ {
        self.capabilities.iter()
    }

    pub(super) fn to_rpc_v12(
        &self,
        prompt: &str,
        gateway_base_url: &str,
        api_key: &str,
        model_id: &str,
        provider_protocol: AgentProviderProtocol,
        capacity: AgentRunCapacity,
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
            context_window_tokens: capacity.context_window_tokens,
            max_output_tokens: capacity.max_output_tokens,
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

#[derive(Debug, Clone, Copy)]
pub(super) struct AgentCompletionValidationContext<'a> {
    pub(super) task_spec: Option<&'a AgentTaskSpec>,
    pub(super) diagnostic_repair_available: bool,
    pub(super) desired_state_satisfied: bool,
}

impl AgentCompletionValidationContext<'_> {
    #[cfg(test)]
    pub(super) const fn legacy(
        diagnostic_repair_available: bool,
        desired_state_satisfied: bool,
    ) -> Self {
        Self {
            task_spec: None,
            diagnostic_repair_available,
            desired_state_satisfied,
        }
    }
}

pub(super) fn validate_completion(
    run_id: &str,
    requirements: &AgentRunRequirements,
    context: AgentCompletionValidationContext<'_>,
    completed: &AgentRunCompletedPayload,
    tool_events: &[AgentToolEvent],
    action_plans: &[AgentActionPlan],
) -> Result<(), AgentCoordinationError> {
    let AgentCompletionValidationContext {
        task_spec,
        diagnostic_repair_available,
        desired_state_satisfied,
    } = context;
    let expected_action_kind = if requirements.requires(AgentCapabilityId::PlanModelStart) {
        Some(AgentActionKind::StartOrSwitchModel)
    } else if requirements.requires(AgentCapabilityId::PlanModelStop) {
        Some(AgentActionKind::StopModel)
    } else if requirements.requires(AgentCapabilityId::PlanModelRemoval) {
        Some(AgentActionKind::RemoveModel)
    } else if requirements.requires(AgentCapabilityId::PlanEngineInstall) {
        Some(AgentActionKind::InstallLlamaCpp)
    } else if requirements.requires(AgentCapabilityId::PlanEngineRemove) {
        Some(AgentActionKind::RemoveLlamaCpp)
    } else if requirements.requires(AgentCapabilityId::PlanExternalAgentConfiguration) {
        Some(AgentActionKind::ConfigureExternalAgent)
    } else if requirements.requires(AgentCapabilityId::PlanExternalAgentDisconnection) {
        Some(AgentActionKind::DisconnectExternalAgent)
    } else if requirements.requires(AgentCapabilityId::PlanExternalAgentInstallation) {
        Some(AgentActionKind::InstallExternalAgent)
    } else if requirements.requires(AgentCapabilityId::PlanManagedExternalAgentRemoval) {
        Some(AgentActionKind::RemoveExternalAgent)
    } else if requirements.requires(AgentCapabilityId::PlanModelDownload) {
        Some(AgentActionKind::DownloadModel)
    } else {
        None
    };
    let diagnostic_action_kind_is_allowed = |kind: AgentActionKind| {
        matches!(
            kind,
            AgentActionKind::InstallLlamaCpp
                | AgentActionKind::ConfigureExternalAgent
                | AgentActionKind::RemoveModel
        )
    };
    let task_action_contract_invalid = task_spec.is_some_and(|spec| {
        let allowed = spec.task_kind().allowed_action_kinds();
        match action_plans {
            [] => {
                !allowed.is_empty()
                    && !desired_state_satisfied
                    && !(spec.task_kind() == hal100_core::AgentTaskKind::RepairEnvironment
                        && !diagnostic_repair_available)
            }
            [plan] => !allowed.contains(&plan.action_kind),
            _ => true,
        }
    });
    let expected_external_target = (requirements
        .requires(AgentCapabilityId::PlanExternalAgentConfiguration)
        || requirements.requires(AgentCapabilityId::PlanExternalAgentDisconnection)
        || requirements.requires(AgentCapabilityId::PlanExternalAgentInstallation)
        || requirements.requires(AgentCapabilityId::PlanManagedExternalAgentRemoval))
    .then(|| requirements.external_agent_target())
    .flatten();
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
        || task_action_contract_invalid
        || (task_spec.is_none()
            && expected_action_kind.is_some_and(|kind| {
                if action_plans.is_empty() {
                    !desired_state_satisfied
                } else {
                    action_plans.len() != 1 || action_plans[0].action_kind != kind
                }
            }))
        || expected_external_target.is_some_and(|integration_id| {
            action_plans.first().is_none_or(|plan| {
                plan.target_id
                    != ExternalAgentIntegrationRegistry::descriptor(integration_id).integration_id
            })
        })
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
        if desired_state_satisfied
            && action_plans.is_empty()
            && AgentCapabilityRegistry::descriptor(capability).effect
                == AgentCapabilityEffect::ActionPlan
        {
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
        "hal100",
        "本地",
        "模型",
        "推理",
        "引擎",
        "后端",
        "配置",
        "电脑",
        "mac",
        "硬件",
        "内存",
        "cpu",
        "芯片",
        "安装",
        "卸载",
        "删除",
        "下载",
        "切换",
        "llama",
        "vllm",
        "opencode",
        "openclaw",
        "open claw",
        "hermes",
        "pi coding",
        "pi agent",
        "pi 副本",
        "自己维护的 pi",
        "agent",
        "智能体",
        "配好",
        "接好",
        "api",
        "token",
        "调试",
        "诊断",
        "修复",
        "排查",
        "故障",
        "错误",
        "出错",
        "失败",
        "日志",
        "运维",
        "监测",
        "监控",
        "部署",
        "就绪",
        "稳定性",
        "链路",
        "线路",
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

pub(super) fn prompt_requires_operational_history(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    [
        "调试",
        "最近失败",
        "近期失败",
        "失败原因",
        "错误历史",
        "操作历史",
        "操作记录",
        "运行记录",
        "运维记录",
        "为什么失败",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

pub(super) fn prompt_requires_operational_health_observation(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    [
        "部署前检查",
        "部署就绪",
        "运行监测",
        "运行监控",
        "短时监测",
        "短时监控",
        "观察运行",
        "运行稳定性",
        "运维检查",
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

pub(super) fn prompt_requires_model_stop_plan(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    let refers_to_model = ["模型", "qwen", "gguf", "llama"]
        .iter()
        .any(|marker| normalized.contains(marker));
    let requests_stop = ["停止", "停掉", "停下", "关闭当前", "关闭本地模型"]
        .iter()
        .any(|marker| normalized.contains(marker));
    refers_to_model && requests_stop && !prompt_refers_to_llama_cpp_engine(prompt)
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

pub(super) fn prompt_external_agent_target(prompt: &str) -> Option<ExternalAgentIntegrationId> {
    let targets = external_agent_targets(prompt);
    (targets.len() == 1).then(|| targets[0])
}

fn external_agent_targets(prompt: &str) -> Vec<ExternalAgentIntegrationId> {
    let normalized = prompt.to_lowercase();
    [
        (
            ExternalAgentIntegrationId::OpenCode,
            &["opencode", "open code"][..],
        ),
        (
            ExternalAgentIntegrationId::PiCodingAgent,
            &[
                "pi coding",
                "pi-coding",
                "pi agent",
                "官方 pi",
                "私有 pi",
                "受管 pi",
            ][..],
        ),
        (
            ExternalAgentIntegrationId::OpenClaw,
            &["openclaw", "open claw", "open-claw", "open爪"][..],
        ),
        (
            ExternalAgentIntegrationId::HermesAgent,
            &["hermes agent", "hermes-agent", "hermes"][..],
        ),
    ]
    .into_iter()
    .filter_map(|(integration_id, markers)| {
        markers
            .iter()
            .any(|marker| normalized.contains(marker))
            .then_some(integration_id)
    })
    .collect()
}

pub(super) fn prompt_requires_external_agent_configuration_plan(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    if prompt_external_agent_target(prompt).is_none()
        || prompt_requires_external_agent_disconnection_plan(prompt)
        || prompt_requires_managed_external_agent_removal_plan(prompt)
    {
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
        "连接 hal100",
        "连接hal100",
    ]
    .iter()
    .any(|marker| normalized.contains(marker));
    let inspection_only = ["查看", "解释", "是什么", "检查配置", "检测配置", "配置状态"]
        .iter()
        .any(|marker| normalized.contains(marker));
    explicit_change
        || (!inspection_only
            && ["配置 ", "配置", "接入 ", "接入"]
                .iter()
                .any(|prefix| normalized.trim().starts_with(prefix)))
}

pub(super) fn prompt_requires_managed_external_agent_removal_plan(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    let explicitly_managed = [
        "hal100 私有",
        "hal100私有",
        "hal100 受管",
        "hal100受管",
        "私有安装",
        "私有运行时",
        "私有 pi",
        "受管 pi",
    ]
    .iter()
    .any(|marker| normalized.contains(marker));
    let requests_removal = ["卸载", "移除", "删除", "清理", "移入废纸篓", "移到废纸篓"]
        .iter()
        .any(|marker| normalized.contains(marker));
    prompt_external_agent_target(prompt).is_some() && explicitly_managed && requests_removal
}

pub(super) fn prompt_requires_external_agent_installation_plan(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    let requests_install = normalized.contains("安装计划")
        || normalized.trim().starts_with("安装")
        || ["帮我安装", "请安装", "并安装", "部署", "装上"]
            .iter()
            .any(|marker| normalized.contains(marker));
    prompt_external_agent_target(prompt).is_some()
        && requests_install
        && !["断开", "卸载", "移除", "删除"]
            .iter()
            .any(|marker| normalized.contains(marker))
}

pub(super) fn prompt_requires_external_agent_disconnection_plan(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    prompt_external_agent_target(prompt).is_some()
        && [
            "断开",
            "取消接入",
            "解除接入",
            "解除连接",
            "移除接入",
            "删除 hal100 配置",
            "删除hal100配置",
        ]
        .iter()
        .any(|marker| normalized.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hal100_core::AgentTaskKind;

    #[derive(Debug)]
    struct LegacyRoute {
        disposition: &'static str,
        task_kind: Option<AgentTaskKind>,
        target_id: Option<&'static str>,
        mutating: bool,
    }

    fn iteration_31_prompt_was_admitted(prompt: &str) -> bool {
        let prompt = prompt.trim();
        if prompt.is_empty()
            || prompt.len() > MAX_PROMPT_BYTES
            || prompt
                .chars()
                .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
            || external_agent_targets(prompt).len() > 1
        {
            return false;
        }
        let normalized = prompt.to_lowercase();
        [
            "hal100",
            "本地",
            "模型",
            "推理",
            "引擎",
            "后端",
            "配置",
            "电脑",
            "mac",
            "硬件",
            "内存",
            "cpu",
            "芯片",
            "安装",
            "卸载",
            "删除",
            "下载",
            "切换",
            "停止",
            "llama",
            "vllm",
            "opencode",
            "openclaw",
            "open claw",
            "hermes",
            "pi coding",
            "pi agent",
            "api",
            "token",
            "调试",
            "故障",
            "错误",
            "失败",
            "日志",
            "运维",
            "监测",
            "监控",
            "部署",
            "就绪",
            "稳定性",
        ]
        .iter()
        .any(|marker| normalized.contains(marker))
    }

    fn legacy_route(prompt: &str) -> LegacyRoute {
        if !iteration_31_prompt_was_admitted(prompt) {
            return LegacyRoute {
                disposition: "reject",
                task_kind: None,
                target_id: None,
                mutating: false,
            };
        }
        let requirements = AgentRunRequirements::for_prompt(prompt);
        let mapped = [
            (
                AgentCapabilityId::PlanModelStart,
                AgentTaskKind::StartModel,
                true,
            ),
            (
                AgentCapabilityId::PlanModelStop,
                AgentTaskKind::StopModel,
                true,
            ),
            (
                AgentCapabilityId::PlanModelRemoval,
                AgentTaskKind::RemoveModel,
                true,
            ),
            (
                AgentCapabilityId::PlanDiagnosticRepair,
                AgentTaskKind::RepairEnvironment,
                true,
            ),
            (
                AgentCapabilityId::PlanEngineInstall,
                AgentTaskKind::InstallEngine,
                true,
            ),
            (
                AgentCapabilityId::PlanEngineRemove,
                AgentTaskKind::RemoveEngine,
                true,
            ),
            (
                AgentCapabilityId::PlanExternalAgentConfiguration,
                AgentTaskKind::ConfigureExternalAgent,
                true,
            ),
            (
                AgentCapabilityId::PlanExternalAgentDisconnection,
                AgentTaskKind::DisconnectExternalAgent,
                true,
            ),
            (
                AgentCapabilityId::PlanExternalAgentInstallation,
                AgentTaskKind::InstallManagedExternalAgent,
                true,
            ),
            (
                AgentCapabilityId::PlanManagedExternalAgentRemoval,
                AgentTaskKind::RemoveManagedExternalAgent,
                true,
            ),
            (
                AgentCapabilityId::PlanModelDownload,
                AgentTaskKind::DownloadModel,
                true,
            ),
            (
                AgentCapabilityId::InspectEnvironmentDiagnostics,
                AgentTaskKind::DiagnoseEnvironment,
                false,
            ),
            (
                AgentCapabilityId::InspectOperationalHistory,
                AgentTaskKind::AnalyzeOperationalHistory,
                false,
            ),
            (
                AgentCapabilityId::ObserveOperationalHealth,
                AgentTaskKind::ObserveDeploymentHealth,
                false,
            ),
            (
                AgentCapabilityId::InspectModelRepository,
                AgentTaskKind::InspectModelRepository,
                false,
            ),
            (
                AgentCapabilityId::SearchModelCatalog,
                AgentTaskKind::SearchModelCatalog,
                false,
            ),
            (
                AgentCapabilityId::InspectExternalAgent,
                AgentTaskKind::InspectExternalAgent,
                false,
            ),
            (
                AgentCapabilityId::InspectRuntimeCatalog,
                AgentTaskKind::InspectRuntime,
                false,
            ),
            (
                AgentCapabilityId::InspectSystemSummary,
                AgentTaskKind::InspectSystem,
                false,
            ),
        ]
        .into_iter()
        .find(|(capability, _, _)| requirements.requires(*capability));
        let Some((_, task_kind, mutating)) = mapped else {
            return LegacyRoute {
                disposition: "reject",
                task_kind: None,
                target_id: None,
                mutating: false,
            };
        };
        LegacyRoute {
            disposition: "task",
            task_kind: Some(task_kind),
            target_id: requirements
                .external_agent_target()
                .map(|target| ExternalAgentIntegrationRegistry::descriptor(target).integration_id),
            mutating,
        }
    }

    #[test]
    fn records_the_iteration_31_legacy_keyword_routing_baseline() {
        let manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../contracts/agent-evals/v1-config-tasks.json"
        ))
        .expect("Agent configuration evaluation manifest");
        let scenarios = manifest["scenarios"]
            .as_array()
            .expect("evaluation scenarios");
        let mut evaluated = 0_u32;
        let mut matched = 0_u32;
        let mut adversarial_mutation_plans = 0_u32;
        let mut mismatch_ids = Vec::new();

        for scenario in scenarios {
            let Some(prompt) = scenario["input"]["prompt"].as_str() else {
                continue;
            };
            evaluated += 1;
            let actual = legacy_route(prompt);
            let expected = &scenario["expected"];
            let expected_disposition = expected["disposition"]
                .as_str()
                .expect("expected disposition");
            let disposition_matches = actual.disposition == expected_disposition;
            let task_matches = expected_disposition != "task"
                || (expected["taskKind"]
                    .as_str()
                    .and_then(AgentTaskKind::from_key)
                    == actual.task_kind
                    && expected["targetId"].as_str() == actual.target_id);
            if disposition_matches && task_matches {
                matched += 1;
            } else {
                mismatch_ids.push(scenario["id"].as_str().expect("scenario id").to_owned());
            }
            if scenario["category"] == "adversarial" && actual.mutating {
                adversarial_mutation_plans += 1;
            }
        }

        eprintln!(
            "iteration31 legacy routing baseline: {matched}/{evaluated} matched, {adversarial_mutation_plans} adversarial mutation plan(s), mismatches={mismatch_ids:?}"
        );
        assert_eq!(evaluated, 20);
        assert_eq!(matched, 6);
        assert_eq!(adversarial_mutation_plans, 1);
        assert_eq!(mismatch_ids.len(), 14);
    }

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

    #[test]
    fn operational_debugging_is_in_domain_and_requires_only_sanitized_history() {
        assert!(validate_prompt("调试 HAL100 最近失败原因").is_ok());
        let requirements = AgentRunRequirements::for_prompt("调试 HAL100 最近失败原因");
        assert!(requirements.requires(AgentCapabilityId::InspectOperationalHistory));
        assert_eq!(requirements.len(), 1);
    }

    #[test]
    fn prompt_validation_admits_bounded_clarification_inputs_before_tool_routing() {
        assert!(validate_prompt("帮我把这个 Agent 配好").is_ok());
        assert!(validate_prompt("同时配置 OpenCode 和 OpenClaw 接入 HAL100").is_ok());
    }

    #[test]
    fn open_chinese_pi_subset_is_admitted_without_broadening_tool_authority() {
        let manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../contracts/agent-evals/v8-open-chinese-inputs.json"
        ))
        .expect("open Chinese evaluation manifest");
        for scenario in manifest["piScenarios"].as_array().expect("Pi scenarios") {
            let id = scenario["id"].as_str().expect("scenario id");
            let prompt = scenario["input"]["prompt"].as_str().expect("prompt");
            validate_prompt(prompt).unwrap_or_else(|error| {
                panic!("open Pi prompt rejected before routing {id}: {error:?}")
            });
        }
    }

    #[test]
    fn deployment_readiness_requires_only_the_bounded_observation() {
        assert!(validate_prompt("执行 HAL100 部署前检查并观察运行稳定性").is_ok());
        let requirements =
            AgentRunRequirements::for_prompt("执行 HAL100 部署前检查并观察运行稳定性");
        assert!(requirements.requires(AgentCapabilityId::ObserveOperationalHealth));
        assert_eq!(requirements.len(), 1);
    }

    #[test]
    fn pi_install_requires_inspection_and_one_private_install_plan() {
        let requirements = AgentRunRequirements::for_prompt(
            "检查官方 Pi Coding Agent 是否已安装；如果没有，为固定版本生成 HAL100 私有安装计划，不要修改 PATH、HOME 或用户配置。",
        );
        assert_eq!(
            requirements.external_agent_target(),
            Some(ExternalAgentIntegrationId::PiCodingAgent)
        );
        assert!(requirements.requires(AgentCapabilityId::InspectExternalAgent));
        assert!(requirements.requires(AgentCapabilityId::PlanExternalAgentInstallation));
        assert!(!requirements.requires(AgentCapabilityId::PlanExternalAgentConfiguration));
        assert_eq!(requirements.len(), 2);
        assert!(requirements.validate().is_ok());
    }

    #[test]
    fn pi_private_removal_is_distinct_from_disconnect_and_user_uninstall() {
        let requirements =
            AgentRunRequirements::for_prompt("将 HAL100 私有 Pi Coding Agent 运行时移入废纸篓");
        assert!(requirements.requires(AgentCapabilityId::InspectExternalAgent));
        assert!(requirements.requires(AgentCapabilityId::PlanManagedExternalAgentRemoval));
        assert!(!requirements.requires(AgentCapabilityId::PlanExternalAgentDisconnection));
        assert_eq!(requirements.len(), 2);
        assert!(requirements.validate().is_ok());

        let user_owned = AgentRunRequirements::for_prompt("卸载官方 Pi Coding Agent");
        assert!(!user_owned.requires(AgentCapabilityId::PlanManagedExternalAgentRemoval));
        assert!(user_owned.requires(AgentCapabilityId::InspectExternalAgent));
    }

    #[test]
    fn structured_task_requirements_come_only_from_the_rust_workflow() {
        let route = hal100_core::AgentTaskIntentRouter::route(
            "请把 OpenCode 重新接好，继续走 HAL100 的本地推理服务",
            hal100_core::AgentTaskProviderMode::Local,
        );
        let spec = route.task_spec().expect("structured OpenCode task");
        let requirements = AgentRunRequirements::for_task_spec(spec).expect("task requirements");

        assert_eq!(
            requirements.external_agent_target(),
            Some(ExternalAgentIntegrationId::OpenCode)
        );
        assert!(requirements.requires(AgentCapabilityId::InspectExternalAgent));
        assert!(requirements.requires(AgentCapabilityId::PlanExternalAgentConfiguration));
        assert_eq!(requirements.len(), 2);
    }
}
