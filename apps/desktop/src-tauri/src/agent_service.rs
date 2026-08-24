use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(test)]
use std::{
    env, fs,
    io::{Read, Write},
    sync::mpsc,
    thread,
    time::Instant,
};

#[cfg(test)]
use hal100_core::{AGENT_CAPABILITY_COUNT, AgentCapabilityId};
use hal100_core::{ExternalAgentIntegrationId, ExternalAgentIntegrationRegistry};
use hal100_infra::{
    AGENT_IDLE_TIMEOUT, AgentModelRuntime, AgentRuntimeError, ClientCredentialError,
    CredentialRegistry, Database, DatabaseError, EngineManagerError, EnvironmentDiagnosticError,
    EnvironmentDiagnostics, GatewayRouteError, GatewayState, HermesAgentIntegrationAdapter,
    HermesAgentIntegrationError, LlamaCppManager, ManagedExternalAgentDeploymentError,
    ManagedExternalAgentDeploymentManager, ModelDownloadError, ModelDownloadManager,
    ModelRemovalError, ModelRemovalManager, OpenClawIntegrationAdapter, OpenClawIntegrationError,
    OpenCodeIntegrationError, OpenCodeManager, PiCodingAgentIntegrationAdapter,
    PiCodingAgentIntegrationError, RemoteModelCatalog, RemoteModelCatalogError,
    stored_client_credential,
};
#[cfg(test)]
use hal100_infra::{
    ExternalModelProfileRegistry, HermesAgentPaths, OpenClawPaths, PiCodingAgentPaths,
};
use hal100_platform::{HardwareProbeError, SidecarLaunchError};
use hal100_protocol::{
    AGENT_RPC_MAX_REQUIRED_TOOLS, AGENT_RPC_VERSION, AgentActionPlan, AgentActionResult,
    AgentCloudRunPreview, AgentCloudSessionPreview, AgentCloudSessionStatus, AgentCloudTarget,
    AgentComponentState, AgentPromptRequest, AgentProviderProtocol, AgentRpcEnvelope,
    AgentRpcFrameError, AgentRunCompletedPayload, AgentRunResult, AgentStatus, AgentToolEvent,
    EnvironmentDiagnosticReport, ModelRemovalKind, ToolCallRequestPayload,
};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use crate::agent_action::{
    AgentActionExecutor, AgentActionPlanError, AgentActionPlanStore, action_kind_key,
};
use crate::agent_coordinator::{
    AgentCoordinationError, AgentRunRegistry, AgentRunRequirements, validate_completion,
    validate_prompt,
};
#[cfg(test)]
use crate::agent_coordinator::{
    prompt_external_agent_target, prompt_requires_diagnostic_repair_plan,
    prompt_requires_engine_install_plan, prompt_requires_engine_remove_plan,
    prompt_requires_environment_diagnostics, prompt_requires_external_agent_configuration_plan,
    prompt_requires_external_agent_disconnection_plan,
    prompt_requires_external_agent_installation_plan, prompt_requires_model_catalog_search,
    prompt_requires_model_download_plan, prompt_requires_model_removal_plan,
    prompt_requires_model_repository_inspection, prompt_requires_model_start_plan,
    prompt_requires_operational_health_observation, prompt_requires_operational_history,
    prompt_requires_runtime_catalog, prompt_requires_system_summary,
};
use crate::agent_kernel::{AgentKernelChannel, AgentKernelError, AgentKernelRunner};
#[cfg(test)]
use crate::agent_provider::{
    AGENT_CLIENT_APP_ID, CLOUD_AGENT_CLIENT_APP_ID, CLOUD_AGENT_ROUTE_PREFIX,
};
use crate::agent_provider::{AgentProviderError, AgentProviderService, ResolvedAgentProvider};
use crate::agent_tools::{AgentToolExecutionError, AgentToolExecutor};
#[cfg(test)]
use hal100_protocol::{
    AgentActionKind, BackendKind, ENVIRONMENT_DIAGNOSTICS_TOOL, EXTERNAL_AGENT_STATUS_TOOL,
    EngineInstallState, LocalModelState, PLAN_DIAGNOSTIC_REPAIR_TOOL, PLAN_ENGINE_REMOVE_TOOL,
    PLAN_EXTERNAL_AGENT_CONFIGURATION_TOOL, PLAN_MODEL_REMOVAL_TOOL, PLAN_MODEL_START_TOOL,
    RUNTIME_CATALOG_TOOL, SYSTEM_SUMMARY_TOOL,
};

const MAX_TOOL_CALLS_PER_RUN: usize = AGENT_RPC_MAX_REQUIRED_TOOLS;

#[derive(Debug, Error)]
pub enum AgentServiceError {
    #[error("HAL100 Agent 请求不能为空，且最多为 4096 个 UTF-8 字节")]
    InvalidPrompt,
    #[error("该请求超出 HAL100、本地模型和推理环境的职责范围")]
    OutsideDomain,
    #[error("HAL100 Agent Kernel 尚未准备好")]
    KernelUnavailable,
    #[error("HAL100 Agent Kernel 运行时版本不匹配")]
    KernelRuntimeVersion,
    #[error("HAL100 Agent Kernel 启动失败")]
    KernelStart,
    #[error("HAL100 Agent Kernel 响应超时")]
    KernelTimeout,
    #[error("HAL100 Agent Kernel 私有协议校验失败")]
    InvalidProtocol,
    #[error("HAL100 Agent 工具结果超过私有协议预算")]
    ToolResultTooLarge,
    #[error("HAL100 Agent 每项任务最多只能生成一个写操作计划，请拆分任务")]
    MultipleActionPlans,
    #[error("HAL100 Agent 单项任务需要的工具过多，请拆分任务")]
    TooManyCapabilities,
    #[error("HAL100 Agent Kernel 拒绝了本次运行：{0}")]
    KernelRejected(String),
    #[error("HAL100 Agent 未按要求完成必需工具：{0}")]
    RequiredToolMissing(&'static str),
    #[error("HAL100 Agent 回答超过安全长度限制")]
    AnswerTooLarge,
    #[error("HAL100 Agent 当前正在处理另一项任务")]
    Busy,
    #[error("HAL100 Agent 任务已取消")]
    Cancelled,
    #[error("当前没有可取消的 HAL100 Agent 任务")]
    NoActiveRun,
    #[error("HAL100 Agent 操作计划不存在、已失效或已被替换")]
    ActionPlanUnavailable,
    #[error("HAL100 Agent 操作计划已经过期")]
    ActionPlanExpired,
    #[error("云端 Agent 目标无效")]
    InvalidCloudTarget,
    #[error("云端 Agent 后端不存在、未启用或尚未载入 Gateway")]
    CloudBackendUnavailable,
    #[error("云端 Agent 仅支持 OpenAI 兼容或 Anthropic 兼容后端")]
    CloudBackendUnsupported,
    #[error("云端 Agent 后端缺少 Keychain 凭据")]
    CloudCredentialMissing,
    #[error("当前已经启用一个云端 Agent 会话；请先明确退出")]
    CloudSessionAlreadyActive,
    #[error("当前没有启用的云端 Agent 会话")]
    NoActiveCloudSession,
    #[error(transparent)]
    Runtime(#[from] AgentRuntimeError),
    #[error(transparent)]
    Engine(#[from] EngineManagerError),
    #[error(transparent)]
    OpenCode(#[from] OpenCodeIntegrationError),
    #[error(transparent)]
    PiCodingAgent(#[from] PiCodingAgentIntegrationError),
    #[error(transparent)]
    OpenClaw(#[from] OpenClawIntegrationError),
    #[error(transparent)]
    HermesAgent(#[from] HermesAgentIntegrationError),
    #[error(transparent)]
    ModelRemoval(#[from] ModelRemovalError),
    #[error(transparent)]
    ModelDownload(#[from] ModelDownloadError),
    #[error(transparent)]
    RemoteCatalog(#[from] RemoteModelCatalogError),
    #[error(transparent)]
    Diagnostics(#[from] EnvironmentDiagnosticError),
    #[error(transparent)]
    ManagedDeployment(#[from] ManagedExternalAgentDeploymentError),
    #[error(transparent)]
    Credential(#[from] ClientCredentialError),
    #[error(transparent)]
    Database(#[from] DatabaseError),
    #[error(transparent)]
    GatewayRoute(#[from] GatewayRouteError),
    #[error(transparent)]
    Launch(#[from] SidecarLaunchError),
    #[error(transparent)]
    Probe(#[from] HardwareProbeError),
    #[error(transparent)]
    Frame(#[from] AgentRpcFrameError),
    #[error("HAL100 Agent 本地进程通信失败")]
    Io(#[source] std::io::Error),
    #[error("HAL100 Agent 后台任务异常结束")]
    Join,
}

impl AgentServiceError {
    fn code(&self) -> &'static str {
        match self {
            Self::InvalidPrompt => "invalid_prompt",
            Self::OutsideDomain => "outside_domain",
            Self::KernelUnavailable => "kernel_unavailable",
            Self::KernelRuntimeVersion => "kernel_runtime_version",
            Self::KernelStart => "kernel_start_failed",
            Self::KernelTimeout => "kernel_timeout",
            Self::InvalidProtocol => "invalid_protocol",
            Self::ToolResultTooLarge => "tool_result_too_large",
            Self::MultipleActionPlans => "multiple_action_plans",
            Self::TooManyCapabilities => "too_many_agent_capabilities",
            Self::KernelRejected(_) => "kernel_rejected",
            Self::RequiredToolMissing(_) => "required_tool_missing",
            Self::AnswerTooLarge => "answer_too_large",
            Self::Busy => "agent_busy",
            Self::Cancelled => "agent_cancelled",
            Self::NoActiveRun => "no_active_agent_run",
            Self::ActionPlanUnavailable => "agent_action_plan_unavailable",
            Self::ActionPlanExpired => "agent_action_plan_expired",
            Self::InvalidCloudTarget => "invalid_cloud_target",
            Self::CloudBackendUnavailable => "cloud_backend_unavailable",
            Self::CloudBackendUnsupported => "cloud_backend_unsupported",
            Self::CloudCredentialMissing => "cloud_credential_missing",
            Self::CloudSessionAlreadyActive => "cloud_session_already_active",
            Self::NoActiveCloudSession => "no_active_cloud_session",
            Self::Runtime(_) => "model_runtime_failed",
            Self::Engine(_) => "managed_model_operation_failed",
            Self::OpenCode(_) => "opencode_configuration_failed",
            Self::PiCodingAgent(_) => "pi_coding_agent_integration_failed",
            Self::OpenClaw(_) => "openclaw_integration_failed",
            Self::HermesAgent(_) => "hermes_agent_integration_failed",
            Self::ModelRemoval(_) => "model_removal_failed",
            Self::ModelDownload(error) => error.code(),
            Self::RemoteCatalog(error) => error.code(),
            Self::Diagnostics(_) => "environment_diagnostics_failed",
            Self::ManagedDeployment(_) => "managed_external_agent_deployment_failed",
            Self::Credential(_) => "credential_failed",
            Self::Database(_) => "database_failed",
            Self::GatewayRoute(_) => "gateway_route_failed",
            Self::Launch(_) => "kernel_launch_policy_failed",
            Self::Probe(_) => "hardware_probe_failed",
            Self::Frame(_) => "rpc_frame_failed",
            Self::Io(_) => "kernel_io_failed",
            Self::Join => "agent_task_join_failed",
        }
    }
}

impl From<AgentProviderError> for AgentServiceError {
    fn from(error: AgentProviderError) -> Self {
        match error {
            AgentProviderError::InvalidCloudTarget => Self::InvalidCloudTarget,
            AgentProviderError::CloudBackendUnavailable => Self::CloudBackendUnavailable,
            AgentProviderError::CloudBackendUnsupported => Self::CloudBackendUnsupported,
            AgentProviderError::CloudCredentialMissing => Self::CloudCredentialMissing,
            AgentProviderError::CloudSessionAlreadyActive => Self::CloudSessionAlreadyActive,
            AgentProviderError::NoActiveCloudSession => Self::NoActiveCloudSession,
            AgentProviderError::StateUnavailable => Self::KernelUnavailable,
            AgentProviderError::Database(error) => Self::Database(error),
        }
    }
}

impl From<AgentKernelError> for AgentServiceError {
    fn from(error: AgentKernelError) -> Self {
        match error {
            AgentKernelError::Unavailable => Self::KernelUnavailable,
            AgentKernelError::RuntimeVersion => Self::KernelRuntimeVersion,
            AgentKernelError::Start => Self::KernelStart,
            AgentKernelError::Timeout => Self::KernelTimeout,
            AgentKernelError::Cancelled => Self::Cancelled,
            AgentKernelError::InvalidProtocol => Self::InvalidProtocol,
            AgentKernelError::Launch(error) => Self::Launch(error),
            AgentKernelError::Frame(error) => Self::Frame(error),
            AgentKernelError::Io(error) => Self::Io(error),
        }
    }
}

impl From<AgentActionPlanError> for AgentServiceError {
    fn from(error: AgentActionPlanError) -> Self {
        match error {
            AgentActionPlanError::Unavailable => Self::ActionPlanUnavailable,
            AgentActionPlanError::Expired => Self::ActionPlanExpired,
        }
    }
}

impl From<AgentToolExecutionError> for AgentServiceError {
    fn from(error: AgentToolExecutionError) -> Self {
        match error {
            AgentToolExecutionError::InvalidProtocol => Self::InvalidProtocol,
            AgentToolExecutionError::ResultTooLarge => Self::ToolResultTooLarge,
            AgentToolExecutionError::Cancelled => Self::Cancelled,
            AgentToolExecutionError::ActionPlan(error) => Self::from(error),
            AgentToolExecutionError::Probe(error) => Self::Probe(error),
            AgentToolExecutionError::Database(error) => Self::Database(error),
            AgentToolExecutionError::Engine(error) => Self::Engine(error),
            AgentToolExecutionError::OpenCode(error) => Self::OpenCode(error),
            AgentToolExecutionError::PiCodingAgent(error) => Self::PiCodingAgent(error),
            AgentToolExecutionError::OpenClaw(error) => Self::OpenClaw(error),
            AgentToolExecutionError::HermesAgent(error) => Self::HermesAgent(error),
            AgentToolExecutionError::ModelRemoval(error) => Self::ModelRemoval(error),
            AgentToolExecutionError::Diagnostics(error) => Self::Diagnostics(error),
            AgentToolExecutionError::RemoteCatalog(error) => Self::RemoteCatalog(error),
            AgentToolExecutionError::ModelDownload(error) => Self::ModelDownload(error),
            AgentToolExecutionError::ManagedDeployment(error) => Self::ManagedDeployment(error),
        }
    }
}

impl From<AgentCoordinationError> for AgentServiceError {
    fn from(error: AgentCoordinationError) -> Self {
        match error {
            AgentCoordinationError::InvalidPrompt => Self::InvalidPrompt,
            AgentCoordinationError::OutsideDomain => Self::OutsideDomain,
            AgentCoordinationError::InvalidProtocol => Self::InvalidProtocol,
            AgentCoordinationError::MultipleActionPlans => Self::MultipleActionPlans,
            AgentCoordinationError::TooManyCapabilities => Self::TooManyCapabilities,
            AgentCoordinationError::RequiredToolMissing(tool_name) => {
                Self::RequiredToolMissing(tool_name)
            }
            AgentCoordinationError::AnswerTooLarge => Self::AnswerTooLarge,
            AgentCoordinationError::NoActiveRun => Self::NoActiveRun,
            AgentCoordinationError::StateUnavailable => Self::KernelUnavailable,
        }
    }
}

pub struct AgentService {
    runtime: Arc<AgentModelRuntime>,
    engine: Arc<LlamaCppManager>,
    open_code: Arc<OpenCodeManager>,
    pi_coding_agent: Arc<PiCodingAgentIntegrationAdapter>,
    openclaw: Arc<OpenClawIntegrationAdapter>,
    hermes_agent: Arc<HermesAgentIntegrationAdapter>,
    managed_deployment: Arc<ManagedExternalAgentDeploymentManager>,
    model_removal: Arc<ModelRemovalManager>,
    diagnostics: Arc<EnvironmentDiagnostics>,
    gateway: GatewayState,
    database: Arc<Database>,
    credentials: CredentialRegistry,
    gateway_base_url: String,
    kernel: AgentKernelRunner,
    run_lock: AsyncMutex<()>,
    kernel_state: Mutex<KernelState>,
    runs: AgentRunRegistry,
    action_plans: AgentActionPlanStore,
    tools: AgentToolExecutor,
    provider: AgentProviderService,
    idle_generation: AtomicU64,
    idle_timeout: Duration,
}

struct KernelState {
    state: AgentComponentState,
    last_error_code: Option<String>,
}

impl AgentService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        runtime: Arc<AgentModelRuntime>,
        engine: Arc<LlamaCppManager>,
        open_code: Arc<OpenCodeManager>,
        pi_coding_agent: Arc<PiCodingAgentIntegrationAdapter>,
        openclaw: Arc<OpenClawIntegrationAdapter>,
        hermes_agent: Arc<HermesAgentIntegrationAdapter>,
        model_removal: Arc<ModelRemovalManager>,
        remote_catalog: Arc<RemoteModelCatalog>,
        model_download: Arc<ModelDownloadManager>,
        managed_deployment: Arc<ManagedExternalAgentDeploymentManager>,
        gateway: GatewayState,
        database: Arc<Database>,
        credentials: CredentialRegistry,
        gateway_base_url: String,
        model_storage_path: PathBuf,
        data_dir: &Path,
    ) -> Result<Self, AgentServiceError> {
        Self::with_idle_timeout(
            runtime,
            engine,
            open_code,
            pi_coding_agent,
            openclaw,
            hermes_agent,
            model_removal,
            remote_catalog,
            model_download,
            managed_deployment,
            gateway,
            database,
            credentials,
            gateway_base_url,
            model_storage_path,
            data_dir,
            AGENT_IDLE_TIMEOUT,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn with_idle_timeout(
        runtime: Arc<AgentModelRuntime>,
        engine: Arc<LlamaCppManager>,
        open_code: Arc<OpenCodeManager>,
        pi_coding_agent: Arc<PiCodingAgentIntegrationAdapter>,
        openclaw: Arc<OpenClawIntegrationAdapter>,
        hermes_agent: Arc<HermesAgentIntegrationAdapter>,
        model_removal: Arc<ModelRemovalManager>,
        remote_catalog: Arc<RemoteModelCatalog>,
        model_download: Arc<ModelDownloadManager>,
        managed_deployment: Arc<ManagedExternalAgentDeploymentManager>,
        gateway: GatewayState,
        database: Arc<Database>,
        credentials: CredentialRegistry,
        gateway_base_url: String,
        model_storage_path: PathBuf,
        data_dir: &Path,
        idle_timeout: Duration,
    ) -> Result<Self, AgentServiceError> {
        let kernel = AgentKernelRunner::discover(data_dir)?;
        let diagnostics = Arc::new(EnvironmentDiagnostics::new(
            database.clone(),
            engine.clone(),
            open_code.clone(),
            pi_coding_agent.clone(),
            openclaw.clone(),
            hermes_agent.clone(),
            gateway.clone(),
        ));
        let provider = AgentProviderService::new(database.clone(), gateway.clone());
        let action_plans = AgentActionPlanStore::new();
        let tools = AgentToolExecutor::new(
            model_storage_path.clone(),
            database.clone(),
            engine.clone(),
            open_code.clone(),
            pi_coding_agent.clone(),
            openclaw.clone(),
            hermes_agent.clone(),
            model_removal.clone(),
            diagnostics.clone(),
            remote_catalog,
            model_download,
            managed_deployment.clone(),
            gateway.clone(),
            action_plans.clone(),
        );
        Ok(Self {
            runtime,
            engine,
            open_code,
            pi_coding_agent,
            openclaw,
            hermes_agent,
            managed_deployment,
            model_removal,
            diagnostics,
            gateway,
            database,
            credentials,
            gateway_base_url,
            kernel,
            run_lock: AsyncMutex::new(()),
            kernel_state: Mutex::new(KernelState {
                state: AgentComponentState::Stopped,
                last_error_code: None,
            }),
            runs: AgentRunRegistry::default(),
            action_plans,
            tools,
            provider,
            idle_generation: AtomicU64::new(0),
            idle_timeout,
        })
    }

    pub fn status(&self) -> Result<AgentStatus, AgentServiceError> {
        let mut status = self.runtime.status()?;
        let kernel = self
            .kernel_state
            .lock()
            .map_err(|_| AgentServiceError::KernelUnavailable)?;
        status.kernel_state = kernel.state;
        status.idle_timeout_seconds = self.idle_timeout.as_secs() as u32;
        if kernel.last_error_code.is_some() {
            status.last_error_code.clone_from(&kernel.last_error_code);
        }
        let active_run = self.runs.snapshot()?;
        status.active_run_id = active_run.as_ref().map(|run| run.run_id.clone());
        status.cancellation_requested = active_run
            .as_ref()
            .is_some_and(|run| run.cancellation_requested);
        Ok(status)
    }

    pub fn environment_diagnostics(
        &self,
    ) -> Result<EnvironmentDiagnosticReport, AgentServiceError> {
        self.diagnostics.run().map_err(AgentServiceError::from)
    }

    pub fn preview_cloud_run(
        &self,
        request: &AgentPromptRequest,
    ) -> Result<AgentCloudRunPreview, AgentServiceError> {
        let prompt = validate_prompt(&request.prompt)?;
        let target = request
            .cloud_target
            .as_ref()
            .ok_or(AgentServiceError::InvalidCloudTarget)?;
        self.provider
            .preview_cloud_run(target, u32::try_from(prompt.len()).unwrap_or(u32::MAX))
            .map_err(AgentServiceError::from)
    }

    pub fn preview_cloud_session(
        &self,
        target: &AgentCloudTarget,
    ) -> Result<AgentCloudSessionPreview, AgentServiceError> {
        self.provider
            .preview_cloud_session(target)
            .map_err(AgentServiceError::from)
    }

    pub fn cloud_session_status(&self) -> Result<AgentCloudSessionStatus, AgentServiceError> {
        self.provider
            .cloud_session_status()
            .map_err(AgentServiceError::from)
    }

    pub fn start_cloud_session(
        &self,
        target: AgentCloudTarget,
    ) -> Result<AgentCloudSessionStatus, AgentServiceError> {
        let _run_guard = self
            .run_lock
            .try_lock()
            .map_err(|_| AgentServiceError::Busy)?;
        self.provider
            .start_cloud_session(target)
            .map_err(AgentServiceError::from)
    }

    pub fn stop_cloud_session(&self) -> Result<AgentCloudSessionStatus, AgentServiceError> {
        let _run_guard = self
            .run_lock
            .try_lock()
            .map_err(|_| AgentServiceError::Busy)?;
        self.provider
            .stop_cloud_session()
            .map_err(AgentServiceError::from)
    }

    fn record_cloud_session_error(&self, error_code: &'static str) {
        self.provider.record_cloud_session_error(error_code);
    }

    fn clear_cloud_session_error(&self) {
        self.provider.clear_cloud_session_error();
    }

    fn resolve_agent_provider(
        &self,
        request: &AgentPromptRequest,
    ) -> Result<ResolvedAgentProvider, AgentServiceError> {
        self.provider
            .resolve_agent_provider(request)
            .map_err(AgentServiceError::from)
    }

    pub async fn run_prompt(
        self: &Arc<Self>,
        request: AgentPromptRequest,
    ) -> Result<AgentRunResult, AgentServiceError> {
        let prompt = validate_prompt(&request.prompt)?;
        let _run_guard = self
            .run_lock
            .try_lock()
            .map_err(|_| AgentServiceError::Busy)?;
        let requirements = AgentRunRequirements::for_prompt(&prompt);
        requirements.validate()?;
        let provider = self.resolve_agent_provider(&request)?;
        if provider.uses_local_runtime {
            self.idle_generation.fetch_add(1, Ordering::AcqRel);
        }
        let started_at_ms = now_ms();
        let run_id = format!("agent-run-{}", Uuid::new_v4().simple());
        self.discard_any_action_plan("superseded_by_new_run");
        let active_run = self.runs.begin(run_id.clone())?;
        let cancellation = active_run.cancellation();
        let _active_run = active_run;
        let provider_label = if provider.uses_local_runtime {
            "local"
        } else if provider.session_bound {
            "cloud_session"
        } else {
            "cloud_single"
        };
        let started_summary = json!({
            "toolPolicy": "read_only_allowlist",
            "provider": provider_label,
            "backendId": provider.backend_id.as_deref(),
            "model": &provider.model_name,
        });
        if let Err(error) = self.database.insert_audit_event(
            "agent_run_started",
            "agent_run",
            &run_id,
            &started_summary.to_string(),
            started_at_ms,
        ) {
            let service_error = AgentServiceError::Database(error);
            if provider.session_bound {
                self.record_cloud_session_error(service_error.code());
            }
            return Err(service_error);
        }
        self.set_kernel_state(AgentComponentState::Starting, None);

        if provider.uses_local_runtime {
            match self
                .runtime
                .ensure_started_cancellable(cancellation.clone())
                .await
            {
                Ok(_) => {}
                Err(AgentRuntimeError::Cancelled) => {
                    self.cancel_completed_run(&run_id, true).await;
                    return Err(AgentServiceError::Cancelled);
                }
                Err(error) => {
                    let service_error = AgentServiceError::Runtime(error);
                    self.fail_run(&run_id, &service_error, true);
                    return Err(service_error);
                }
            }
        }
        if cancellation.load(Ordering::Acquire) {
            self.cancel_completed_run(&run_id, provider.uses_local_runtime)
                .await;
            return Err(AgentServiceError::Cancelled);
        }

        let _temporary_route = if let Some(backend_id) = provider.backend_id.as_deref() {
            if let Err(error) =
                self.gateway
                    .set_model_route(&provider.model_id, backend_id, &provider.model_name)
            {
                let service_error = AgentServiceError::GatewayRoute(error);
                if provider.session_bound {
                    self.record_cloud_session_error(service_error.code());
                }
                self.fail_run(&run_id, &service_error, false);
                return Err(service_error);
            }
            Some(TemporaryAgentRoute {
                gateway: self.gateway.clone(),
                alias: provider.model_id.clone(),
            })
        } else {
            None
        };

        let client_key = format!(
            "hal100_agent_session_{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        let credential = match stored_client_credential(
            format!("{}-{}", provider.client_app_id, Uuid::new_v4().simple()),
            provider.client_app_id,
            if provider.uses_local_runtime {
                "HAL100 Agent"
            } else if provider.session_bound {
                "HAL100 Agent 云端会话任务"
            } else {
                "HAL100 Agent 云端单次任务"
            },
            &client_key,
        ) {
            Ok(credential) => credential,
            Err(error) => {
                let service_error = AgentServiceError::Credential(error);
                if provider.session_bound {
                    self.record_cloud_session_error(service_error.code());
                }
                self.fail_run(&run_id, &service_error, provider.uses_local_runtime);
                return Err(service_error);
            }
        };
        if let Err(error) = self.credentials.upsert(credential) {
            let service_error = AgentServiceError::Credential(error);
            if provider.session_bound {
                self.record_cloud_session_error(service_error.code());
            }
            self.fail_run(&run_id, &service_error, provider.uses_local_runtime);
            return Err(service_error);
        }
        let _credential = TransientAgentCredential {
            registry: self.credentials.clone(),
            client_app_id: provider.client_app_id,
        };
        self.set_kernel_state(AgentComponentState::Running, None);

        let uses_local_runtime = provider.uses_local_runtime;
        let session_bound = provider.session_bound;
        let result_model_name = provider.model_name.clone();

        let input = SidecarRunInput {
            run_id: run_id.clone(),
            prompt,
            requirements,
            gateway_base_url: self.gateway_base_url.clone(),
            api_key: client_key,
            model_id: provider.model_id,
            provider_protocol: provider.protocol,
            kernel: self.kernel.clone(),
            tools: self.tools.clone(),
            cancellation,
            runtime_handle: tokio::runtime::Handle::current(),
        };
        let run_result = tauri::async_runtime::spawn_blocking(move || run_sidecar_once(input))
            .await
            .map_err(|_| AgentServiceError::Join)
            .and_then(|result| result);

        match run_result {
            Ok(sidecar) => {
                let completed_at_ms = now_ms();
                self.set_kernel_state(AgentComponentState::Stopped, None);
                if session_bound {
                    self.clear_cloud_session_error();
                }
                if uses_local_runtime {
                    self.schedule_idle_stop();
                }
                if let Err(error) = self.database.insert_audit_event(
                    "agent_run_completed",
                    "agent_run",
                    &run_id,
                    &json!({
                        "toolCalls": sidecar.tool_events.len(),
                        "model": result_model_name,
                        "provider": provider_label,
                    })
                    .to_string(),
                    completed_at_ms,
                ) {
                    let service_error = AgentServiceError::Database(error);
                    if session_bound {
                        self.record_cloud_session_error(service_error.code());
                    }
                    self.set_kernel_state(
                        AgentComponentState::Error,
                        Some(service_error.code().to_owned()),
                    );
                    return Err(service_error);
                }
                Ok(AgentRunResult {
                    run_id,
                    answer: sidecar.answer,
                    tool_events: sidecar.tool_events,
                    action_plans: sidecar.action_plans,
                    model_name: result_model_name,
                    started_at_ms,
                    completed_at_ms,
                })
            }
            Err(AgentServiceError::Cancelled) => {
                self.cancel_completed_run(&run_id, uses_local_runtime).await;
                Err(AgentServiceError::Cancelled)
            }
            Err(error) => {
                if session_bound {
                    self.record_cloud_session_error(error.code());
                }
                self.fail_run(&run_id, &error, uses_local_runtime);
                Err(error)
            }
        }
    }

    pub async fn stop_runtime(self: &Arc<Self>) -> Result<AgentStatus, AgentServiceError> {
        let _run_guard = self
            .run_lock
            .try_lock()
            .map_err(|_| AgentServiceError::Busy)?;
        self.idle_generation.fetch_add(1, Ordering::AcqRel);
        self.set_kernel_state(AgentComponentState::Stopped, None);
        self.runtime.stop().await?;
        self.status()
    }

    pub fn cancel_active_run(&self) -> Result<AgentStatus, AgentServiceError> {
        self.runs.cancel()?;
        self.status()
    }

    pub fn action_plan(&self, plan_id: &str) -> Result<AgentActionPlan, AgentServiceError> {
        self.action_plans
            .current(plan_id, now_ms())
            .map_err(AgentServiceError::from)
    }

    pub fn discard_action_plan(&self, plan_id: &str, reason: &'static str) {
        if let Some(discarded) = self.action_plans.discard(plan_id) {
            self.discard_executor_plan(&discarded.executor);
            let _ = self.database.insert_audit_event(
                "agent_action_discarded",
                "agent_action_plan",
                plan_id,
                &json!({ "reason": reason }).to_string(),
                now_ms(),
            );
        }
    }

    fn discard_any_action_plan(&self, reason: &'static str) {
        if let Some(discarded) = self.action_plans.discard_any() {
            self.discard_executor_plan(&discarded.executor);
            let _ = self.database.insert_audit_event(
                "agent_action_discarded",
                "agent_action_plan",
                &discarded.plan.plan_id,
                &json!({ "reason": reason }).to_string(),
                now_ms(),
            );
        }
    }

    fn discard_executor_plan(&self, executor: &AgentActionExecutor) {
        match executor {
            AgentActionExecutor::StartOrSwitchModel { .. } => {}
            AgentActionExecutor::DownloadModel { download_plan_id } => {
                let _ = self.tools.discard_model_download_plan(download_plan_id);
            }
            AgentActionExecutor::InstallLlamaCpp { engine_plan_id } => {
                let _ = self.engine.discard_install_plan(engine_plan_id);
            }
            AgentActionExecutor::RemoveLlamaCpp { engine_plan_id } => {
                let _ = self.engine.discard_remove_plan(engine_plan_id);
            }
            AgentActionExecutor::RemoveModel {
                removal_plan_id, ..
            } => {
                let _ = self.model_removal.discard_plan(removal_plan_id);
            }
            AgentActionExecutor::InstallExternalAgent {
                deployment_plan_id, ..
            } => {
                let _ = self
                    .managed_deployment
                    .discard_install_plan(deployment_plan_id);
            }
            AgentActionExecutor::RemoveExternalAgent {
                deployment_plan_id, ..
            } => {
                let _ = self
                    .managed_deployment
                    .discard_removal_plan(deployment_plan_id);
            }
            AgentActionExecutor::ConfigureExternalAgent {
                integration_id,
                integration_plan_id,
            } => self.discard_external_agent_configuration(*integration_id, integration_plan_id),
            AgentActionExecutor::DisconnectExternalAgent {
                integration_id,
                integration_plan_id,
            } => self.discard_external_agent_disconnection(*integration_id, integration_plan_id),
        }
    }

    fn discard_external_agent_configuration(
        &self,
        integration_id: ExternalAgentIntegrationId,
        plan_id: &str,
    ) {
        match integration_id {
            ExternalAgentIntegrationId::OpenCode => {
                let _ = self.open_code.discard_configuration_plan(plan_id);
            }
            ExternalAgentIntegrationId::PiCodingAgent => {
                let _ = self.pi_coding_agent.discard_configuration_plan(plan_id);
            }
            ExternalAgentIntegrationId::OpenClaw => {
                let _ = self.openclaw.discard_configuration_plan(plan_id);
            }
            ExternalAgentIntegrationId::HermesAgent => {
                let _ = self.hermes_agent.discard_configuration_plan(plan_id);
            }
        }
    }

    fn discard_external_agent_disconnection(
        &self,
        integration_id: ExternalAgentIntegrationId,
        plan_id: &str,
    ) {
        match integration_id {
            ExternalAgentIntegrationId::OpenCode => {
                let _ = self.open_code.discard_disconnection_plan(plan_id);
            }
            ExternalAgentIntegrationId::PiCodingAgent => {
                let _ = self.pi_coding_agent.discard_disconnection_plan(plan_id);
            }
            ExternalAgentIntegrationId::OpenClaw => {
                let _ = self.openclaw.discard_disconnection_plan(plan_id);
            }
            ExternalAgentIntegrationId::HermesAgent => {
                let _ = self.hermes_agent.discard_disconnection_plan(plan_id);
            }
        }
    }

    async fn apply_external_agent_configuration(
        &self,
        integration_id: ExternalAgentIntegrationId,
        plan_id: String,
    ) -> Result<String, AgentServiceError> {
        match integration_id {
            ExternalAgentIntegrationId::OpenCode => {
                let adapter = self.open_code.clone();
                tauri::async_runtime::spawn_blocking(move || adapter.apply_configuration(&plan_id))
                    .await
                    .map_err(|_| AgentServiceError::Join)?
                    .map_err(AgentServiceError::from)?;
            }
            ExternalAgentIntegrationId::PiCodingAgent => {
                let adapter = self.pi_coding_agent.clone();
                tauri::async_runtime::spawn_blocking(move || adapter.apply_configuration(&plan_id))
                    .await
                    .map_err(|_| AgentServiceError::Join)?
                    .map_err(AgentServiceError::from)?;
            }
            ExternalAgentIntegrationId::OpenClaw => {
                let adapter = self.openclaw.clone();
                tauri::async_runtime::spawn_blocking(move || adapter.apply_configuration(&plan_id))
                    .await
                    .map_err(|_| AgentServiceError::Join)?
                    .map_err(AgentServiceError::from)?;
            }
            ExternalAgentIntegrationId::HermesAgent => {
                let adapter = self.hermes_agent.clone();
                tauri::async_runtime::spawn_blocking(move || adapter.apply_configuration(&plan_id))
                    .await
                    .map_err(|_| AgentServiceError::Join)?
                    .map_err(AgentServiceError::from)?;
            }
        }
        let display_name =
            ExternalAgentIntegrationRegistry::descriptor(integration_id).display_name;
        Ok(format!("{display_name} 已通过 HAL100 Gateway 接入"))
    }

    async fn apply_external_agent_disconnection(
        &self,
        integration_id: ExternalAgentIntegrationId,
        plan_id: String,
    ) -> Result<String, AgentServiceError> {
        match integration_id {
            ExternalAgentIntegrationId::OpenCode => {
                let adapter = self.open_code.clone();
                tauri::async_runtime::spawn_blocking(move || adapter.apply_disconnection(&plan_id))
                    .await
                    .map_err(|_| AgentServiceError::Join)?
                    .map_err(AgentServiceError::from)?;
            }
            ExternalAgentIntegrationId::PiCodingAgent => {
                let adapter = self.pi_coding_agent.clone();
                tauri::async_runtime::spawn_blocking(move || adapter.apply_disconnection(&plan_id))
                    .await
                    .map_err(|_| AgentServiceError::Join)?
                    .map_err(AgentServiceError::from)?;
            }
            ExternalAgentIntegrationId::OpenClaw => {
                let adapter = self.openclaw.clone();
                tauri::async_runtime::spawn_blocking(move || adapter.apply_disconnection(&plan_id))
                    .await
                    .map_err(|_| AgentServiceError::Join)?
                    .map_err(AgentServiceError::from)?;
            }
            ExternalAgentIntegrationId::HermesAgent => {
                let adapter = self.hermes_agent.clone();
                tauri::async_runtime::spawn_blocking(move || adapter.apply_disconnection(&plan_id))
                    .await
                    .map_err(|_| AgentServiceError::Join)?
                    .map_err(AgentServiceError::from)?;
            }
        }
        let display_name =
            ExternalAgentIntegrationRegistry::descriptor(integration_id).display_name;
        Ok(format!(
            "{display_name} 已与 HAL100 断开；用户配置和非 HAL100 凭据保持不变"
        ))
    }

    pub async fn apply_action_plan(
        self: &Arc<Self>,
        plan_id: &str,
    ) -> Result<AgentActionResult, AgentServiceError> {
        let _run_guard = self
            .run_lock
            .try_lock()
            .map_err(|_| AgentServiceError::Busy)?;
        let pending = self.action_plans.take(plan_id, now_ms())?;
        let plan = pending.plan;
        let execution = match pending.executor {
            AgentActionExecutor::StartOrSwitchModel { model_id } => self
                .engine
                .start_model(&model_id)
                .await
                .map(|status| {
                    (
                        "模型已启动，hal100-active 已安全切换".to_owned(),
                        Some(status.runtime_state),
                    )
                })
                .map_err(AgentServiceError::from),
            AgentActionExecutor::DownloadModel { download_plan_id } => self
                .tools
                .start_model_download(&download_plan_id)
                .map(|snapshot| (format!("模型下载任务已启动：{}", snapshot.file_name), None))
                .map_err(AgentServiceError::from),
            AgentActionExecutor::InstallLlamaCpp { engine_plan_id } => self
                .engine
                .apply_install(&engine_plan_id)
                .await
                .map(|status| {
                    (
                        "固定版本 llama.cpp 已完成下载、校验和安装".to_owned(),
                        Some(status.runtime_state),
                    )
                })
                .map_err(AgentServiceError::from),
            AgentActionExecutor::RemoveLlamaCpp { engine_plan_id } => self
                .engine
                .apply_remove(&engine_plan_id)
                .await
                .map(|status| {
                    (
                        "HAL100 托管的 llama.cpp 已卸载，模型文件未被删除".to_owned(),
                        Some(status.runtime_state),
                    )
                })
                .map_err(AgentServiceError::from),
            AgentActionExecutor::RemoveModel {
                removal_plan_id,
                model_id,
            } => {
                let manager = self.model_removal.clone();
                self.engine
                    .run_if_model_inactive(&model_id, move || {
                        manager.apply_removal(&removal_plan_id, None)
                    })
                    .await
                    .map_err(AgentServiceError::from)
                    .and_then(|result| result.map_err(AgentServiceError::from))
                    .map(|result| {
                        let summary = if result.source_file_preserved {
                            "模型索引已从 HAL100 移除，外部源文件保持不变"
                        } else if result.removal_kind == ModelRemovalKind::MoveManagedFileToTrash {
                            "托管模型已移入系统废纸篓，并从 HAL100 模型库移除"
                        } else {
                            "失效的托管模型索引已从 HAL100 模型库移除"
                        };
                        (summary.to_owned(), None)
                    })
            }
            AgentActionExecutor::InstallExternalAgent {
                integration_id,
                deployment_plan_id,
            } => {
                let manager = self.managed_deployment.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    manager.apply_install(&deployment_plan_id)
                })
                .await
                .map_err(|_| AgentServiceError::Join)
                .and_then(|result| result.map_err(AgentServiceError::from))
                .map(|result| {
                    let display_name =
                        ExternalAgentIntegrationRegistry::descriptor(integration_id).display_name;
                    (
                        format!(
                            "{display_name} {} 已安装到 HAL100 私有目录；用户安装、PATH 和配置未改动",
                            result.package_version
                        ),
                        None,
                    )
                })
            }
            AgentActionExecutor::RemoveExternalAgent {
                integration_id,
                deployment_plan_id,
            } => {
                let manager = self.managed_deployment.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    manager.apply_removal(&deployment_plan_id)
                })
                .await
                .map_err(|_| AgentServiceError::Join)
                .and_then(|result| result.map_err(AgentServiceError::from))
                .map(|_| {
                    let display_name =
                        ExternalAgentIntegrationRegistry::descriptor(integration_id).display_name;
                    (
                        format!(
                            "HAL100 私有 {display_name} 已移入系统废纸篓；用户安装、配置和会话未改动"
                        ),
                        None,
                    )
                })
            }
            AgentActionExecutor::ConfigureExternalAgent {
                integration_id,
                integration_plan_id,
            } => self
                .apply_external_agent_configuration(integration_id, integration_plan_id)
                .await
                .map(|summary| (summary, None)),
            AgentActionExecutor::DisconnectExternalAgent {
                integration_id,
                integration_plan_id,
            } => self
                .apply_external_agent_disconnection(integration_id, integration_plan_id)
                .await
                .map(|summary| (summary, None)),
        };
        let (outcome_summary, runtime_state) = match execution {
            Ok(result) => result,
            Err(error) => {
                let _ = self.database.insert_audit_event(
                    "agent_action_failed",
                    "agent_action_plan",
                    &plan.plan_id,
                    &json!({ "errorCode": error.code() }).to_string(),
                    now_ms(),
                );
                return Err(error);
            }
        };
        self.database.insert_audit_event(
            "agent_action_executed",
            "agent_action_plan",
            &plan.plan_id,
            &json!({
                "action": action_kind_key(plan.action_kind),
                "targetId": plan.target_id,
            })
            .to_string(),
            now_ms(),
        )?;
        let diagnostic_report = match self.diagnostics.run() {
            Ok(report) => Some(report),
            Err(_) => {
                tracing::warn!(
                    error_code = "agent_action_recheck_failed",
                    "agent_action_recheck_failed"
                );
                let _ = self.database.insert_audit_event(
                    "agent_action_recheck_failed",
                    "agent_action_plan",
                    &plan.plan_id,
                    "{}",
                    now_ms(),
                );
                None
            }
        };
        Ok(AgentActionResult {
            plan_id: plan.plan_id,
            action_kind: plan.action_kind,
            target_id: plan.target_id,
            target_name: plan.target_name,
            outcome_summary,
            runtime_state,
            diagnostic_report,
        })
    }

    async fn cancel_completed_run(&self, run_id: &str, uses_local_runtime: bool) {
        self.discard_any_action_plan("agent_run_cancelled");
        self.set_kernel_state(AgentComponentState::Stopped, None);
        if uses_local_runtime && let Err(error) = self.runtime.stop().await {
            tracing::warn!(
                error_code = "agent_cancel_cleanup_failed",
                error = %error,
                "agent_cancel_cleanup_failed"
            );
        }
        let _ = self.database.insert_audit_event(
            "agent_run_cancelled",
            "agent_run",
            run_id,
            "{}",
            now_ms(),
        );
    }

    fn set_kernel_state(&self, state: AgentComponentState, error_code: Option<String>) {
        if let Ok(mut kernel) = self.kernel_state.lock() {
            kernel.state = state;
            kernel.last_error_code = error_code;
        }
    }

    fn record_error(&self, error: &AgentServiceError) {
        self.set_kernel_state(AgentComponentState::Error, Some(error.code().to_owned()));
        tracing::warn!(error_code = error.code(), "agent_run_failed");
    }

    fn fail_run(
        self: &Arc<Self>,
        run_id: &str,
        error: &AgentServiceError,
        uses_local_runtime: bool,
    ) {
        self.discard_any_action_plan("agent_run_failed");
        self.record_error(error);
        let _ = self.database.insert_audit_event(
            "agent_run_failed",
            "agent_run",
            run_id,
            &format!("{{\"errorCode\":\"{}\"}}", error.code()),
            now_ms(),
        );
        if uses_local_runtime {
            self.schedule_idle_stop();
        }
    }

    fn schedule_idle_stop(self: &Arc<Self>) {
        let generation = self.idle_generation.fetch_add(1, Ordering::AcqRel) + 1;
        let idle_timeout = self.idle_timeout;
        let service = Arc::downgrade(self);
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(idle_timeout).await;
            loop {
                let Some(service) = service.upgrade() else {
                    return;
                };
                if service.idle_generation.load(Ordering::Acquire) != generation {
                    return;
                }
                let Ok(_run_guard) = service.run_lock.try_lock() else {
                    drop(service);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                };
                if service.idle_generation.load(Ordering::Acquire) != generation {
                    return;
                }
                if let Err(error) = service.runtime.stop().await {
                    tracing::warn!(
                        error_code = "agent_idle_stop_failed",
                        error = %error,
                        "agent_runtime_idle_stop_failed"
                    );
                } else {
                    tracing::info!("agent_runtime_stopped_after_idle_timeout");
                }
                return;
            }
        });
    }
}

struct SidecarRunInput {
    run_id: String,
    prompt: String,
    requirements: AgentRunRequirements,
    gateway_base_url: String,
    api_key: String,
    model_id: String,
    provider_protocol: AgentProviderProtocol,
    kernel: AgentKernelRunner,
    tools: AgentToolExecutor,
    cancellation: Arc<AtomicBool>,
    runtime_handle: tokio::runtime::Handle,
}

struct SidecarRunOutput {
    answer: String,
    tool_events: Vec<AgentToolEvent>,
    action_plans: Vec<AgentActionPlan>,
}

fn run_sidecar_once(input: SidecarRunInput) -> Result<SidecarRunOutput, AgentServiceError> {
    input.kernel.run(&input.cancellation, |channel| {
        exchange_with_sidecar(&input, channel)
    })
}

fn exchange_with_sidecar(
    input: &SidecarRunInput,
    channel: &mut AgentKernelChannel,
) -> Result<SidecarRunOutput, AgentServiceError> {
    let ping_id = format!("ping-{}", input.run_id);
    channel.send(&AgentRpcEnvelope {
        protocol_version: AGENT_RPC_VERSION,
        id: ping_id.clone(),
        kind: "system.ping".to_owned(),
        payload: json!({}),
    })?;
    let pong = channel.receive(&input.cancellation)?;
    if pong.id != ping_id
        || pong.kind != "system.pong"
        || pong.payload.get("piEnabled").and_then(Value::as_bool) != Some(true)
        || pong
            .payload
            .get("directToolExecutionEnabled")
            .and_then(Value::as_bool)
            != Some(false)
    {
        return Err(AgentServiceError::InvalidProtocol);
    }

    let payload = input.requirements.to_rpc_v9(
        &input.prompt,
        &input.gateway_base_url,
        &input.api_key,
        &input.model_id,
        input.provider_protocol,
    );
    channel.send(&AgentRpcEnvelope {
        protocol_version: AGENT_RPC_VERSION,
        id: input.run_id.clone(),
        kind: "agent.run.start".to_owned(),
        payload: serde_json::to_value(payload).map_err(|_| AgentServiceError::InvalidProtocol)?,
    })?;

    let mut tool_run = input.tools.start_run(
        input.run_id.clone(),
        input.requirements.external_agent_target(),
        input.runtime_handle.clone(),
        input.cancellation.clone(),
    );
    let mut seen_tool_calls = HashSet::new();
    let mut last_tool_failure_code: Option<String> = None;
    loop {
        let envelope = channel.receive(&input.cancellation)?;
        match envelope.kind.as_str() {
            "tool.call.request" => {
                let request: ToolCallRequestPayload = serde_json::from_value(envelope.payload)
                    .map_err(|_| AgentServiceError::InvalidProtocol)?;
                if request.run_id != input.run_id
                    || !seen_tool_calls.insert(request.tool_call_id.clone())
                    || seen_tool_calls.len() > MAX_TOOL_CALLS_PER_RUN
                {
                    return Err(AgentServiceError::InvalidProtocol);
                }
                let result = tool_run.handle(&request)?;
                if let Some(error) = result.error.as_ref() {
                    last_tool_failure_code = Some(error.code.clone());
                }
                channel.send(&AgentRpcEnvelope {
                    protocol_version: AGENT_RPC_VERSION,
                    id: envelope.id,
                    kind: "tool.call.result".to_owned(),
                    payload: serde_json::to_value(result)
                        .map_err(|_| AgentServiceError::InvalidProtocol)?,
                })?;
            }
            "agent.run.completed" => {
                if envelope.id != input.run_id {
                    return Err(AgentServiceError::InvalidProtocol);
                }
                let completed: AgentRunCompletedPayload = serde_json::from_value(envelope.payload)
                    .map_err(|_| AgentServiceError::InvalidProtocol)?;
                let completion_validation = validate_completion(
                    &input.run_id,
                    &input.requirements,
                    tool_run.diagnostic_repair_available(),
                    &completed,
                    tool_run.tool_events(),
                    tool_run.action_plans(),
                );
                if matches!(
                    completion_validation,
                    Err(AgentCoordinationError::RequiredToolMissing(_))
                ) && let Some(code) = last_tool_failure_code.as_deref()
                {
                    return Err(AgentServiceError::KernelRejected(format!(
                        "required_tool_failed:{code}"
                    )));
                }
                completion_validation?;
                channel.request_shutdown(&input.run_id, &input.cancellation)?;
                let tools = tool_run.finish();
                return Ok(SidecarRunOutput {
                    answer: completed.answer,
                    tool_events: tools.tool_events,
                    action_plans: tools.action_plans,
                });
            }
            "system.error" => return Err(kernel_rejection(&envelope.payload)),
            _ => return Err(AgentServiceError::InvalidProtocol),
        }
    }
}
fn kernel_rejection(payload: &Value) -> AgentServiceError {
    let code = payload
        .get("code")
        .and_then(Value::as_str)
        .filter(|code| {
            !code.is_empty()
                && code.len() <= 64
                && code
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
        })
        .unwrap_or("unknown");
    AgentServiceError::KernelRejected(code.to_owned())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}

struct TransientAgentCredential {
    registry: CredentialRegistry,
    client_app_id: &'static str,
}

struct TemporaryAgentRoute {
    gateway: GatewayState,
    alias: String,
}

impl Drop for TransientAgentCredential {
    fn drop(&mut self) {
        let _ = self.registry.remove_client(self.client_app_id);
    }
}

impl Drop for TemporaryAgentRoute {
    fn drop(&mut self) {
        let _ = self.gateway.remove_model_route(&self.alias);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_action::PendingAgentAction;

    fn model_download_fixture(
        database: Arc<Database>,
        model_storage_path: PathBuf,
    ) -> (Arc<RemoteModelCatalog>, Arc<ModelDownloadManager>) {
        let catalog = Arc::new(RemoteModelCatalog::new().expect("test remote model catalog"));
        let downloads = Arc::new(
            ModelDownloadManager::new(database, catalog.clone(), model_storage_path)
                .expect("test model download manager"),
        );
        (catalog, downloads)
    }

    fn external_agent_adapters(
        database: Arc<Database>,
        credentials: CredentialRegistry,
        home: &Path,
        data_dir: &Path,
        gateway_base_url: &str,
    ) -> (
        Arc<PiCodingAgentIntegrationAdapter>,
        Arc<OpenClawIntegrationAdapter>,
        Arc<HermesAgentIntegrationAdapter>,
    ) {
        let profiles = ExternalModelProfileRegistry::conservative_managed_route();
        (
            Arc::new(PiCodingAgentIntegrationAdapter::with_gateway_base_url(
                database.clone(),
                credentials.clone(),
                profiles.clone(),
                PiCodingAgentPaths::for_macos(home, data_dir),
                gateway_base_url.to_owned(),
            )),
            Arc::new(OpenClawIntegrationAdapter::with_gateway_base_url(
                database.clone(),
                credentials.clone(),
                profiles.clone(),
                OpenClawPaths::for_macos(home, data_dir),
                gateway_base_url.to_owned(),
            )),
            Arc::new(HermesAgentIntegrationAdapter::with_gateway_base_url(
                database,
                credentials,
                profiles,
                HermesAgentPaths::for_macos(home, data_dir),
                gateway_base_url.to_owned(),
            )),
        )
    }

    fn action_plan_fixture(expires_at_ms: i64) -> PendingAgentAction {
        PendingAgentAction {
            executor: AgentActionExecutor::StartOrSwitchModel {
                model_id: "managed-model-1".to_owned(),
            },
            plan: AgentActionPlan {
                plan_id: "agent-plan-1".to_owned(),
                run_id: "agent-run-1".to_owned(),
                action_kind: AgentActionKind::StartOrSwitchModel,
                target_id: "managed-model-1".to_owned(),
                target_name: "Qwen 测试模型".to_owned(),
                current_state: None,
                details: vec!["测试计划".to_owned()],
                expires_at_ms,
                action_summary: "安全启动模型".to_owned(),
                requires_native_confirmation: true,
            },
        }
    }

    #[test]
    fn accepts_only_agent_domain_prompts() {
        assert!(validate_prompt("检测这台 Mac 并给出本地模型建议").is_ok());
        assert!(matches!(
            validate_prompt("同时配置 Pi Coding Agent 和 OpenClaw"),
            Err(AgentCoordinationError::InvalidPrompt)
        ));
        assert!(matches!(
            validate_prompt("给我写一首关于春天的诗"),
            Err(AgentCoordinationError::OutsideDomain)
        ));
    }

    #[test]
    fn marks_hardware_prompts_as_requiring_the_rust_tool() {
        assert!(prompt_requires_system_summary("检测电脑配置"));
        assert!(prompt_requires_system_summary("CPU 和内存是多少"));
        assert!(!prompt_requires_system_summary(
            "解释 HAL100 的 Gateway 配置"
        ));
    }

    fn cloud_service_fixture(
        kind: &str,
        credential_id: Option<&str>,
        load_backend: bool,
    ) -> (AgentService, PathBuf) {
        let data_dir = env::temp_dir().join(format!(
            "hal100-agent-cloud-test-{}",
            Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&data_dir).expect("create Agent cloud test directory");
        let database = Arc::new(
            Database::open(data_dir.join("hal100.sqlite")).expect("open Agent cloud test DB"),
        );
        database
            .upsert_backend(&hal100_infra::StoredBackendRecord {
                id: "cloud-provider".to_owned(),
                display_name: "测试云端后端".to_owned(),
                kind: kind.to_owned(),
                api_root: "http://127.0.0.1:48991/v1/".to_owned(),
                auth_style: "bearer".to_owned(),
                credential_id: credential_id.map(str::to_owned),
                enabled: true,
                created_at_ms: 1,
                updated_at_ms: 1,
            })
            .expect("store Agent cloud backend");
        let credentials = CredentialRegistry::new(Vec::new());
        let usage_writer = hal100_infra::UsageWriter::start(database.clone());
        let gateway = GatewayState::new(None, credentials.clone(), usage_writer)
            .expect("create Agent cloud Gateway");
        if load_backend {
            gateway
                .upsert_routed_backend(
                    hal100_infra::BackendConfig::new(
                        "cloud-provider",
                        "http://127.0.0.1:48991/v1/",
                        Some("fixture-upstream-secret".to_owned()),
                    )
                    .expect("cloud backend config"),
                )
                .expect("load Agent cloud backend");
        }
        let engine = Arc::new(
            LlamaCppManager::new(
                database.clone(),
                gateway.clone(),
                data_dir.join("engines/llama.cpp"),
            )
            .expect("Agent cloud test engine"),
        );
        let runtime = Arc::new(
            AgentModelRuntime::new(database.clone(), engine.clone(), gateway.clone())
                .expect("Agent cloud test runtime"),
        );
        let open_code = Arc::new(OpenCodeManager::with_gateway_base_url(
            database.clone(),
            credentials.clone(),
            hal100_infra::OpenCodePaths::for_macos(&data_dir.join("home"), &data_dir),
            "http://127.0.0.1:10100/v1".to_owned(),
        ));
        let (pi_coding_agent, openclaw, hermes_agent) = external_agent_adapters(
            database.clone(),
            credentials.clone(),
            &data_dir.join("home"),
            &data_dir,
            "http://127.0.0.1:10100/v1",
        );
        let model_storage_path = data_dir.join("models");
        let (remote_catalog, model_download) =
            model_download_fixture(database.clone(), model_storage_path.clone());
        let service = AgentService::with_idle_timeout(
            runtime,
            engine,
            open_code,
            pi_coding_agent,
            openclaw,
            hermes_agent,
            Arc::new(ModelRemovalManager::new(
                database.clone(),
                model_storage_path.clone(),
            )),
            remote_catalog,
            model_download,
            Arc::new(ManagedExternalAgentDeploymentManager::new(
                database.clone(),
                data_dir.join("external-agents"),
                Vec::new(),
            )),
            gateway,
            database,
            credentials,
            "http://127.0.0.1:10100/v1".to_owned(),
            model_storage_path,
            &data_dir,
            Duration::from_millis(25),
        )
        .expect("Agent cloud test service");
        (service, data_dir)
    }

    #[test]
    fn cloud_preview_resolves_only_a_loaded_credentialed_supported_backend() {
        let (service, data_dir) =
            cloud_service_fixture("external_openai", Some("keychain-reference"), true);
        let request = AgentPromptRequest {
            prompt: "检查 HAL100 推理后端配置".to_owned(),
            cloud_target: Some(hal100_protocol::AgentCloudTarget {
                backend_id: "cloud-provider".to_owned(),
                model: "gpt-test".to_owned(),
            }),
        };

        let preview = service.preview_cloud_run(&request).expect("cloud preview");
        assert_eq!(preview.backend_kind, BackendKind::ExternalOpenAi);
        assert_eq!(preview.model, "gpt-test");
        assert!(!preview.sends_credentials_to_sidecar);
        assert!(!preview.sends_local_paths);
        assert!(preview.sends_system_instructions);
        assert!(preview.may_send_tool_results);
        let provider = service
            .resolve_agent_provider(&request)
            .expect("cloud provider");
        assert_eq!(provider.protocol, AgentProviderProtocol::CloudOpenAi);
        assert_eq!(provider.client_app_id, CLOUD_AGENT_CLIENT_APP_ID);
        assert!(!provider.uses_local_runtime);

        let route_alias = provider.model_id.clone();
        service
            .gateway
            .set_model_route(&route_alias, "cloud-provider", "gpt-test")
            .expect("temporary cloud route");
        {
            let _guard = TemporaryAgentRoute {
                gateway: service.gateway.clone(),
                alias: route_alias.clone(),
            };
            assert!(
                service
                    .gateway
                    .routing_snapshot()
                    .model_routes
                    .iter()
                    .any(|route| route.alias == route_alias)
            );
        }
        assert!(
            service
                .gateway
                .routing_snapshot()
                .model_routes
                .iter()
                .all(|route| route.alias != route_alias)
        );
        drop(service);
        let _ = fs::remove_dir_all(data_dir);
    }

    #[test]
    fn cloud_preview_rejects_missing_credentials_unsupported_kinds_and_unloaded_backends() {
        let request = AgentPromptRequest {
            prompt: "检查 HAL100 推理后端配置".to_owned(),
            cloud_target: Some(hal100_protocol::AgentCloudTarget {
                backend_id: "cloud-provider".to_owned(),
                model: "test-model".to_owned(),
            }),
        };

        let (missing_key, missing_key_dir) = cloud_service_fixture("external_openai", None, true);
        assert!(matches!(
            missing_key.preview_cloud_run(&request),
            Err(AgentServiceError::CloudCredentialMissing)
        ));
        drop(missing_key);
        let _ = fs::remove_dir_all(missing_key_dir);

        let (unsupported, unsupported_dir) =
            cloud_service_fixture("external_ollama", Some("keychain-reference"), true);
        assert!(matches!(
            unsupported.preview_cloud_run(&request),
            Err(AgentServiceError::CloudBackendUnsupported)
        ));
        drop(unsupported);
        let _ = fs::remove_dir_all(unsupported_dir);

        let (unloaded, unloaded_dir) =
            cloud_service_fixture("external_anthropic", Some("keychain-reference"), false);
        assert!(matches!(
            unloaded.preview_cloud_run(&request),
            Err(AgentServiceError::CloudBackendUnavailable)
        ));
        drop(unloaded);
        let _ = fs::remove_dir_all(unloaded_dir);
    }

    #[test]
    fn cloud_agent_session_key_is_transient_and_has_an_independent_usage_identity() {
        let registry = CredentialRegistry::new(Vec::new());
        let key = "hal100_cloud_agent_fixture_key_123456789";
        registry
            .upsert(
                stored_client_credential(
                    "cloud-agent-session",
                    CLOUD_AGENT_CLIENT_APP_ID,
                    "HAL100 Agent 云端单次任务",
                    key,
                )
                .expect("cloud Agent session credential"),
            )
            .expect("register cloud Agent session key");
        {
            let _credential = TransientAgentCredential {
                registry: registry.clone(),
                client_app_id: CLOUD_AGENT_CLIENT_APP_ID,
            };
            let client = registry
                .authenticate(key)
                .expect("authenticate cloud Agent");
            assert_eq!(client.client_app_id, CLOUD_AGENT_CLIENT_APP_ID);
            assert_ne!(client.client_app_id, AGENT_CLIENT_APP_ID);
        }
        assert!(registry.authenticate(key).is_none());
    }

    #[test]
    fn cloud_session_is_memory_only_explicitly_revocable_and_never_falls_back() {
        let (service, data_dir) =
            cloud_service_fixture("external_anthropic", Some("keychain-reference"), true);
        let target = AgentCloudTarget {
            backend_id: "cloud-provider".to_owned(),
            model: "claude-test".to_owned(),
        };
        assert!(
            !service
                .cloud_session_status()
                .expect("initial session")
                .active
        );
        let preview = service
            .preview_cloud_session(&target)
            .expect("cloud session preview");
        assert!(preview.sends_future_prompts);
        assert!(!preview.stores_conversation_history);
        assert!(!preview.sends_credentials_to_sidecar);

        let active = service
            .start_cloud_session(target.clone())
            .expect("start cloud session");
        assert!(active.active);
        assert!(active.available);
        assert_eq!(
            active.provider_protocol,
            Some(AgentProviderProtocol::CloudAnthropic)
        );
        let (remote_catalog, model_download) =
            model_download_fixture(service.database.clone(), data_dir.join("models"));
        let restarted_service = AgentService::with_idle_timeout(
            service.runtime.clone(),
            service.engine.clone(),
            service.open_code.clone(),
            service.pi_coding_agent.clone(),
            service.openclaw.clone(),
            service.hermes_agent.clone(),
            service.model_removal.clone(),
            remote_catalog,
            model_download,
            service.managed_deployment.clone(),
            service.gateway.clone(),
            service.database.clone(),
            service.credentials.clone(),
            service.gateway_base_url.clone(),
            data_dir.join("models"),
            &data_dir,
            Duration::from_millis(25),
        )
        .expect("recreate Agent service over the same persistent state");
        assert!(
            !restarted_service
                .cloud_session_status()
                .expect("fresh service session state")
                .active
        );
        assert!(
            restarted_service
                .resolve_agent_provider(&AgentPromptRequest {
                    prompt: "说明 HAL100 本地模型".to_owned(),
                    cloud_target: None,
                })
                .expect("fresh service local default")
                .uses_local_runtime
        );
        drop(restarted_service);
        assert!(matches!(
            service.start_cloud_session(target.clone()),
            Err(AgentServiceError::CloudSessionAlreadyActive)
        ));
        let run_guard = service.run_lock.try_lock().expect("hold Agent task lock");
        assert!(matches!(
            service.stop_cloud_session(),
            Err(AgentServiceError::Busy)
        ));
        drop(run_guard);
        let provider = service
            .resolve_agent_provider(&AgentPromptRequest {
                prompt: "说明 HAL100 后端配置".to_owned(),
                cloud_target: None,
            })
            .expect("session-bound provider");
        assert!(provider.session_bound);
        assert!(!provider.uses_local_runtime);
        assert_eq!(provider.protocol, AgentProviderProtocol::CloudAnthropic);
        service.record_cloud_session_error("kernel_rejected");
        let failed_auth = service
            .cloud_session_status()
            .expect("session remains visible after a provider failure");
        assert!(failed_auth.active);
        assert!(failed_auth.available);
        assert_eq!(
            failed_auth.last_error_code.as_deref(),
            Some("kernel_rejected")
        );

        let inactive = service.stop_cloud_session().expect("stop cloud session");
        assert!(!inactive.active);
        let local = service
            .resolve_agent_provider(&AgentPromptRequest {
                prompt: "说明 HAL100 本地模型".to_owned(),
                cloud_target: None,
            })
            .expect("local provider after session exit");
        assert!(local.uses_local_runtime);
        assert!(!local.session_bound);
        assert!(matches!(
            service.stop_cloud_session(),
            Err(AgentServiceError::NoActiveCloudSession)
        ));

        let run_guard = service.run_lock.try_lock().expect("hold Agent task lock");
        assert!(matches!(
            service.start_cloud_session(target.clone()),
            Err(AgentServiceError::Busy)
        ));
        drop(run_guard);
        service
            .start_cloud_session(target)
            .expect("restart cloud session");
        service
            .gateway
            .remove_routed_backend("cloud-provider")
            .expect("unload session backend");
        let unavailable = service
            .cloud_session_status()
            .expect("unavailable cloud session status");
        assert!(unavailable.active);
        assert!(!unavailable.available);
        assert_eq!(
            unavailable.last_error_code.as_deref(),
            Some("cloud_backend_unavailable")
        );
        service
            .stop_cloud_session()
            .expect("revoke unavailable session");

        drop(service);
        let _ = fs::remove_dir_all(data_dir);
    }

    fn spawn_single_openai_upstream() -> (
        std::net::SocketAddr,
        mpsc::Receiver<String>,
        thread::JoinHandle<()>,
    ) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("mock cloud listener");
        let address = listener.local_addr().expect("mock cloud address");
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("mock cloud request");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("mock cloud read timeout");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            let mut expected_len = None;
            loop {
                let read = stream.read(&mut buffer).expect("read mock cloud request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if expected_len.is_none()
                    && let Some(header_end) =
                        request.windows(4).position(|bytes| bytes == b"\r\n\r\n")
                {
                    let headers = String::from_utf8_lossy(&request[..header_end]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    expected_len = Some(header_end + 4 + content_length);
                }
                if expected_len.is_some_and(|expected| request.len() >= expected) {
                    break;
                }
            }
            let captured = String::from_utf8_lossy(&request).into_owned();
            sender.send(captured).expect("capture mock cloud request");
            let body = concat!(
                "data: {\"id\":\"chatcmpl-hal100\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"cloud-test\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"HAL100 云端无网模拟验收完成。\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":5,\"total_tokens\":17}}\n\n",
                "data: [DONE]\n\n"
            );
            write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("write mock cloud response");
            stream.flush().expect("flush mock cloud response");
        });
        (address, receiver, worker)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cloud_agent_session_completes_through_the_real_gateway_without_local_fallback() {
        let (upstream_address, upstream_request, upstream_worker) = spawn_single_openai_upstream();
        let data_dir = env::temp_dir().join(format!(
            "hal100-agent-cloud-e2e-{}",
            Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&data_dir).expect("create cloud Agent e2e directory");
        let database = Arc::new(
            Database::open(data_dir.join("hal100.sqlite")).expect("open cloud Agent e2e DB"),
        );
        database
            .upsert_backend(&hal100_infra::StoredBackendRecord {
                id: "cloud-e2e".to_owned(),
                display_name: "无网模拟 OpenAI".to_owned(),
                kind: "external_openai".to_owned(),
                api_root: format!("http://{upstream_address}/v1/"),
                auth_style: "bearer".to_owned(),
                credential_id: Some("keychain-cloud-e2e".to_owned()),
                enabled: true,
                created_at_ms: 1,
                updated_at_ms: 1,
            })
            .expect("store cloud Agent e2e backend");
        let credentials = CredentialRegistry::new(Vec::new());
        let usage_writer = hal100_infra::UsageWriter::start(database.clone());
        let gateway = GatewayState::new(None, credentials.clone(), usage_writer.clone())
            .expect("create cloud Agent e2e Gateway");
        gateway
            .upsert_routed_backend(
                hal100_infra::BackendConfig::new(
                    "cloud-e2e",
                    &format!("http://{upstream_address}/v1/"),
                    Some("upstream-cloud-only-secret".to_owned()),
                )
                .expect("cloud Agent e2e backend config"),
            )
            .expect("load cloud Agent e2e backend");
        let gateway_listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("cloud Agent Gateway listener");
        let gateway_address = gateway_listener
            .local_addr()
            .expect("cloud Agent Gateway address");
        let gateway_task = tokio::spawn(hal100_infra::serve_gateway(
            gateway_listener,
            gateway.clone(),
        ));
        let engine = Arc::new(
            LlamaCppManager::new(
                database.clone(),
                gateway.clone(),
                data_dir.join("engines/llama.cpp"),
            )
            .expect("cloud Agent e2e engine"),
        );
        let runtime = Arc::new(
            AgentModelRuntime::new(database.clone(), engine.clone(), gateway.clone())
                .expect("cloud Agent e2e runtime"),
        );
        let open_code = Arc::new(OpenCodeManager::with_gateway_base_url(
            database.clone(),
            credentials.clone(),
            hal100_infra::OpenCodePaths::for_macos(&data_dir.join("home"), &data_dir),
            format!("http://{gateway_address}/v1"),
        ));
        let (pi_coding_agent, openclaw, hermes_agent) = external_agent_adapters(
            database.clone(),
            credentials.clone(),
            &data_dir.join("home"),
            &data_dir,
            &format!("http://{gateway_address}/v1"),
        );
        let model_storage_path = data_dir.join("models");
        let (remote_catalog, model_download) =
            model_download_fixture(database.clone(), model_storage_path.clone());
        let service = Arc::new(
            AgentService::with_idle_timeout(
                runtime,
                engine,
                open_code,
                pi_coding_agent,
                openclaw,
                hermes_agent,
                Arc::new(ModelRemovalManager::new(
                    database.clone(),
                    model_storage_path.clone(),
                )),
                remote_catalog,
                model_download,
                Arc::new(ManagedExternalAgentDeploymentManager::new(
                    database.clone(),
                    data_dir.join("external-agents"),
                    Vec::new(),
                )),
                gateway.clone(),
                database.clone(),
                credentials.clone(),
                format!("http://{gateway_address}/v1"),
                model_storage_path,
                &data_dir,
                Duration::from_millis(25),
            )
            .expect("cloud Agent e2e service"),
        );
        let initial_local_runtime_state = service
            .status()
            .expect("cloud Agent initial status")
            .model_runtime_state;

        service
            .start_cloud_session(AgentCloudTarget {
                backend_id: "cloud-e2e".to_owned(),
                model: "cloud-test".to_owned(),
            })
            .expect("activate cloud Agent e2e session");
        let result = service
            .run_prompt(AgentPromptRequest {
                prompt: "说明 HAL100 Gateway 和推理后端配置。".to_owned(),
                cloud_target: None,
            })
            .await
            .expect("complete cloud Agent e2e run");
        assert_eq!(result.model_name, "cloud-test");
        assert!(result.answer.contains("HAL100"));
        assert_eq!(
            service
                .status()
                .expect("cloud Agent final status")
                .model_runtime_state,
            initial_local_runtime_state
        );
        assert!(credentials.is_empty());
        assert!(
            service
                .cloud_session_status()
                .expect("active cloud Agent session")
                .active
        );
        assert!(
            gateway
                .routing_snapshot()
                .model_routes
                .iter()
                .all(|route| !route.alias.starts_with(CLOUD_AGENT_ROUTE_PREFIX))
        );
        usage_writer
            .flush(Duration::from_secs(1))
            .expect("flush cloud Agent usage");
        let dashboard = database.usage_dashboard(10).expect("cloud Agent usage");
        let usage = dashboard
            .recent_requests
            .first()
            .expect("cloud Agent usage record");
        assert_eq!(usage.client_app_id, CLOUD_AGENT_CLIENT_APP_ID);
        assert_eq!(usage.backend_id, "cloud-e2e");
        assert_eq!(usage.resolved_model, "cloud-test");
        assert_eq!(usage.total_tokens, Some(17));
        let captured = upstream_request
            .recv_timeout(Duration::from_secs(1))
            .expect("captured cloud upstream request");
        assert!(
            captured
                .to_ascii_lowercase()
                .contains("authorization: bearer upstream-cloud-only-secret")
        );
        assert!(captured.contains("\"model\":\"cloud-test\""));
        assert!(!captured.contains("hal100_agent_session"));

        service
            .stop_cloud_session()
            .expect("exit cloud Agent e2e session");
        assert!(
            !service
                .cloud_session_status()
                .expect("inactive cloud Agent session")
                .active
        );

        upstream_worker.join().expect("mock cloud worker");
        gateway_task.abort();
        drop(service);
        drop(gateway);
        drop(usage_writer);
        drop(database);
        let _ = fs::remove_dir_all(data_dir);
    }

    #[test]
    fn classifies_runtime_inspection_and_model_start_plans_without_matching_install_requests() {
        assert!(prompt_requires_runtime_catalog("列出可用模型和引擎状态"));
        assert!(prompt_requires_model_start_plan("切换到 Qwen3.5 模型"));
        assert!(prompt_requires_model_start_plan("启动这个 GGUF 模型"));
        assert!(!prompt_requires_model_start_plan("帮我安装一个本地模型"));
        assert!(!prompt_requires_model_start_plan("什么模型适合运行"));
        assert!(prompt_requires_model_removal_plan("删除 Qwen3.5-2B 模型"));
        assert!(prompt_requires_model_removal_plan(
            "把这个外部 GGUF 移出模型库"
        ));
        assert!(!prompt_requires_model_removal_plan(
            "卸载 llama.cpp 推理引擎"
        ));
        assert!(prompt_requires_model_catalog_search("搜索 Qwen GGUF 模型"));
        assert!(prompt_requires_model_repository_inspection(
            "查看模型仓库中的 GGUF 文件"
        ));
        assert!(prompt_requires_model_download_plan(
            "下载 Qwen Q4_K_M GGUF 模型"
        ));
        assert!(!prompt_requires_model_download_plan("查看模型下载状态"));
    }

    #[test]
    fn classifies_only_explicit_engine_and_external_agent_mutation_intents() {
        assert!(prompt_requires_engine_install_plan(
            "检查状态并生成安装 llama.cpp 的计划"
        ));
        assert!(prompt_requires_engine_remove_plan("卸载本地推理引擎"));
        assert!(!prompt_requires_engine_install_plan("帮我安装一个本地模型"));
        assert_eq!(
            prompt_external_agent_target("检查 OpenCode 配置状态"),
            Some(ExternalAgentIntegrationId::OpenCode)
        );
        assert!(!prompt_requires_external_agent_configuration_plan(
            "检查 OpenCode 配置状态"
        ));
        assert!(prompt_requires_external_agent_configuration_plan(
            "检查 OpenCode 状态，并生成接入 HAL100 Gateway 的配置计划"
        ));
        assert!(prompt_requires_external_agent_disconnection_plan(
            "断开 Hermes Agent 与 HAL100 的接入"
        ));
        assert!(prompt_requires_external_agent_installation_plan(
            "安装官方 Pi Coding Agent，但不要修改用户配置"
        ));
        assert!(!prompt_requires_external_agent_installation_plan(
            "检查官方 Pi Coding Agent 安装状态"
        ));
        assert_eq!(
            prompt_external_agent_target("配置官方 Pi Coding Agent"),
            Some(ExternalAgentIntegrationId::PiCodingAgent)
        );
        assert_eq!(
            prompt_external_agent_target("检查 OpenClaw 状态"),
            Some(ExternalAgentIntegrationId::OpenClaw)
        );
    }

    #[test]
    fn classifies_only_explicit_environment_diagnosis_and_repair_intents() {
        assert!(prompt_requires_environment_diagnostics(
            "全面诊断 HAL100 当前运行环境"
        ));
        assert!(prompt_requires_environment_diagnostics(
            "诊断并修复当前最高优先级问题"
        ));
        assert!(prompt_requires_diagnostic_repair_plan(
            "诊断并修复当前最高优先级问题"
        ));
        assert!(!prompt_requires_diagnostic_repair_plan(
            "全面诊断并解释当前运行环境"
        ));
        assert!(!prompt_requires_environment_diagnostics(
            "检查 OpenCode 配置状态"
        ));
        assert!(prompt_requires_operational_history(
            "调试 HAL100 最近一次配置失败原因"
        ));
        assert!(!prompt_requires_operational_history(
            "检查 HAL100 当前模型状态"
        ));
        assert!(prompt_requires_operational_health_observation(
            "执行 HAL100 部署前检查并观察运行稳定性"
        ));
        assert!(!prompt_requires_operational_health_observation(
            "长期后台监控所有系统日志"
        ));
    }

    #[test]
    fn prompt_requirements_expand_capability_prerequisites_without_unrelated_tools() {
        let model_start = AgentRunRequirements::for_prompt("启动这个 GGUF 模型");
        assert!(model_start.requires(AgentCapabilityId::PlanModelStart));
        assert!(model_start.requires(AgentCapabilityId::InspectRuntimeCatalog));
        assert_eq!(model_start.len(), 2);

        let opencode = AgentRunRequirements::for_prompt(
            "检查 OpenCode 状态，并生成接入 HAL100 Gateway 的配置计划",
        );
        assert!(opencode.requires(AgentCapabilityId::PlanExternalAgentConfiguration));
        assert!(opencode.requires(AgentCapabilityId::InspectExternalAgent));
        assert_eq!(
            opencode.external_agent_target(),
            Some(ExternalAgentIntegrationId::OpenCode)
        );
        assert_eq!(opencode.len(), 2);

        let download =
            AgentRunRequirements::for_prompt("搜索 Qwen GGUF 并为一个 Q4_K_M 文件生成下载计划");
        assert!(download.requires(AgentCapabilityId::SearchModelCatalog));
        assert!(download.requires(AgentCapabilityId::InspectModelRepository));
        assert!(download.requires(AgentCapabilityId::PlanModelDownload));
        assert!(!download.requires(AgentCapabilityId::PlanModelStart));
        assert_eq!(download.len(), 3);

        let debug = AgentRunRequirements::for_prompt("调试 HAL100 最近失败原因");
        assert!(debug.requires(AgentCapabilityId::InspectOperationalHistory));
        assert_eq!(debug.len(), 1);

        let observation =
            AgentRunRequirements::for_prompt("执行 HAL100 部署前检查并观察运行稳定性");
        assert!(observation.requires(AgentCapabilityId::ObserveOperationalHealth));
        assert_eq!(observation.len(), 1);

        let repair_with_explicit_action =
            AgentRunRequirements::for_prompt("诊断并修复问题，同时卸载本地推理引擎");
        assert!(repair_with_explicit_action.requires(AgentCapabilityId::PlanEngineRemove));
        assert!(!repair_with_explicit_action.requires(AgentCapabilityId::PlanDiagnosticRepair));

        let conflicting =
            AgentRunRequirements::for_prompt("下载 Qwen GGUF 模型，同时安装 llama.cpp 推理引擎");
        assert_eq!(
            conflicting.validate(),
            Err(AgentCoordinationError::MultipleActionPlans)
        );
    }

    #[test]
    fn capability_requirements_adapt_to_rpc_v9_capability_set() {
        let payload = AgentRunRequirements::requiring([
            AgentCapabilityId::PlanModelRemoval,
            AgentCapabilityId::InspectSystemSummary,
        ])
        .to_rpc_v9(
            "移除模型并报告硬件",
            "http://127.0.0.1:39000/v1",
            "temporary-key",
            "hal100-agent",
            AgentProviderProtocol::LocalOpenAi,
        );

        assert_eq!(
            payload.required_tools,
            vec![
                SYSTEM_SUMMARY_TOOL.to_owned(),
                RUNTIME_CATALOG_TOOL.to_owned(),
                PLAN_MODEL_REMOVAL_TOOL.to_owned(),
            ]
        );
        assert_eq!(payload.prompt, "移除模型并报告硬件");
        assert_eq!(payload.gateway_base_url, "http://127.0.0.1:39000/v1");
        assert_eq!(payload.model_id, "hal100-agent");
        assert_eq!(
            payload.provider_protocol,
            AgentProviderProtocol::LocalOpenAi
        );
    }

    #[test]
    fn gateway_evidence_accepts_a_chinese_paraphrase_but_not_a_generic_answer() {
        assert!(answer_contains_gateway_evidence(
            "HAL100 本地网关接收客户端请求，再路由到当前推理后端。"
        ));
        assert!(answer_contains_gateway_evidence(
            "这个中转层会把请求转发给已经配置的模型。"
        ));
        assert!(!answer_contains_gateway_evidence(
            "配置完成后即可正常使用。"
        ));
    }

    #[test]
    fn rejects_completion_that_skips_a_required_probe() {
        let completed = AgentRunCompletedPayload {
            run_id: "run-1".to_owned(),
            answer: "猜测的回答".to_owned(),
            registered_tool_count: AGENT_CAPABILITY_COUNT,
            completed_tool_calls: 0,
            tool_names: Vec::new(),
        };
        assert!(matches!(
            validate_completion(
                "run-1",
                &AgentRunRequirements::requiring([AgentCapabilityId::InspectSystemSummary]),
                false,
                &completed,
                &[],
                &[]
            ),
            Err(AgentCoordinationError::RequiredToolMissing(
                SYSTEM_SUMMARY_TOOL
            ))
        ));
    }

    #[test]
    fn accepts_one_allowlisted_repair_plan_after_the_required_diagnosis() {
        let completed = AgentRunCompletedPayload {
            run_id: "run-diagnostic".to_owned(),
            answer: "已生成一项修复计划，尚未执行。".to_owned(),
            registered_tool_count: AGENT_CAPABILITY_COUNT,
            completed_tool_calls: 2,
            tool_names: vec![
                ENVIRONMENT_DIAGNOSTICS_TOOL.to_owned(),
                PLAN_DIAGNOSTIC_REPAIR_TOOL.to_owned(),
            ],
        };
        let events = vec![
            AgentToolEvent {
                tool_call_id: "tool-diagnostic".to_owned(),
                tool_name: ENVIRONMENT_DIAGNOSTICS_TOOL.to_owned(),
                label: "诊断".to_owned(),
                status: "completed".to_owned(),
                summary: "安全诊断".to_owned(),
            },
            AgentToolEvent {
                tool_call_id: "tool-repair".to_owned(),
                tool_name: PLAN_DIAGNOSTIC_REPAIR_TOOL.to_owned(),
                label: "修复计划".to_owned(),
                status: "awaiting_confirmation".to_owned(),
                summary: "尚未执行".to_owned(),
            },
        ];
        let mut plan = action_plan_fixture(200).plan;
        plan.action_kind = AgentActionKind::RemoveModel;
        assert!(
            validate_completion(
                "run-diagnostic",
                &AgentRunRequirements::requiring([AgentCapabilityId::PlanDiagnosticRepair]),
                true,
                &completed,
                &events,
                &[plan],
            )
            .is_ok()
        );
    }

    #[test]
    fn accepts_a_diagnosis_without_a_plan_when_no_safe_repair_exists() {
        let completed = AgentRunCompletedPayload {
            run_id: "run-clean".to_owned(),
            answer: "当前没有可安全自动修复的问题。".to_owned(),
            registered_tool_count: AGENT_CAPABILITY_COUNT,
            completed_tool_calls: 1,
            tool_names: vec![ENVIRONMENT_DIAGNOSTICS_TOOL.to_owned()],
        };
        let events = [AgentToolEvent {
            tool_call_id: "tool-diagnostic".to_owned(),
            tool_name: ENVIRONMENT_DIAGNOSTICS_TOOL.to_owned(),
            label: "诊断".to_owned(),
            status: "completed".to_owned(),
            summary: "没有可修复项".to_owned(),
        }];
        assert!(
            validate_completion(
                "run-clean",
                &AgentRunRequirements::requiring([AgentCapabilityId::PlanDiagnosticRepair]),
                false,
                &completed,
                &events,
                &[],
            )
            .is_ok()
        );
    }

    #[test]
    fn rejects_an_external_agent_plan_for_a_different_prompt_target() {
        let requirements = AgentRunRequirements::for_prompt("配置 Pi Coding Agent 接入 HAL100");
        let completed = AgentRunCompletedPayload {
            run_id: "run-external".to_owned(),
            answer: "已生成配置计划，尚未执行。".to_owned(),
            registered_tool_count: AGENT_CAPABILITY_COUNT,
            completed_tool_calls: 2,
            tool_names: vec![
                EXTERNAL_AGENT_STATUS_TOOL.to_owned(),
                PLAN_EXTERNAL_AGENT_CONFIGURATION_TOOL.to_owned(),
            ],
        };
        let events = vec![
            AgentToolEvent {
                tool_call_id: "tool-status".to_owned(),
                tool_name: EXTERNAL_AGENT_STATUS_TOOL.to_owned(),
                label: "检查外部 Agent".to_owned(),
                status: "completed".to_owned(),
                summary: "已检查".to_owned(),
            },
            AgentToolEvent {
                tool_call_id: "tool-plan".to_owned(),
                tool_name: PLAN_EXTERNAL_AGENT_CONFIGURATION_TOOL.to_owned(),
                label: "配置计划".to_owned(),
                status: "awaiting_confirmation".to_owned(),
                summary: "尚未执行".to_owned(),
            },
        ];
        let mut plan = action_plan_fixture(200).plan;
        plan.run_id = "run-external".to_owned();
        plan.action_kind = AgentActionKind::ConfigureExternalAgent;
        plan.target_id = "openclaw".to_owned();
        assert_eq!(
            validate_completion(
                "run-external",
                &requirements,
                false,
                &completed,
                &events,
                &[plan],
            ),
            Err(AgentCoordinationError::InvalidProtocol)
        );
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    struct LiveAgentFixture {
        service: Arc<AgentService>,
        engine: Arc<LlamaCppManager>,
        ready_model: hal100_protocol::LocalModelSummary,
        gateway_task: tokio::task::JoinHandle<()>,
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn live_agent_fixture(idle_timeout: Duration) -> LiveAgentFixture {
        let home = PathBuf::from(env::var_os("HOME").expect("HOME for explicit live test"));
        let data_dir = home.join("Library/Application Support/com.hal100.desktop");
        let database = Arc::new(
            Database::open(data_dir.join("hal100.sqlite")).expect("open HAL100 development DB"),
        );
        let ready_model = database
            .local_models()
            .expect("local model catalog")
            .into_iter()
            .find(|model| {
                model.id == hal100_infra::AGENT_MODEL_ID && model.state == LocalModelState::Ready
            })
            .expect("a ready managed model for action-plan acceptance");
        let credentials = CredentialRegistry::new(
            database
                .load_client_credentials()
                .expect("load client credentials"),
        );
        let usage_writer = hal100_infra::UsageWriter::start(database.clone());
        let gateway = hal100_infra::GatewayState::new(None, credentials.clone(), usage_writer)
            .expect("create test Gateway");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("test loopback port");
        let gateway_address = listener.local_addr().expect("Gateway address");
        let gateway_for_task = gateway.clone();
        let gateway_task = tokio::spawn(async move {
            let _ = hal100_infra::serve_gateway(listener, gateway_for_task).await;
        });
        let engine = Arc::new(
            hal100_infra::LlamaCppManager::new(
                database.clone(),
                gateway.clone(),
                data_dir.join("engines/llama.cpp"),
            )
            .expect("llama.cpp manager"),
        );
        let runtime = Arc::new(
            AgentModelRuntime::new(database.clone(), engine.clone(), gateway.clone())
                .expect("Agent runtime"),
        );
        let open_code = Arc::new(OpenCodeManager::with_gateway_base_url(
            database.clone(),
            credentials.clone(),
            hal100_infra::OpenCodePaths::for_macos(&home, &data_dir),
            format!("http://{gateway_address}/v1"),
        ));
        let (pi_coding_agent, openclaw, hermes_agent) = external_agent_adapters(
            database.clone(),
            credentials.clone(),
            &home,
            &data_dir,
            &format!("http://{gateway_address}/v1"),
        );
        let model_storage_path = data_dir.join("models");
        let (remote_catalog, model_download) =
            model_download_fixture(database.clone(), model_storage_path.clone());
        let service = Arc::new(
            AgentService::with_idle_timeout(
                runtime,
                engine.clone(),
                open_code,
                pi_coding_agent,
                openclaw,
                hermes_agent,
                Arc::new(ModelRemovalManager::new(
                    database.clone(),
                    model_storage_path.clone(),
                )),
                remote_catalog,
                model_download,
                Arc::new(ManagedExternalAgentDeploymentManager::new(
                    database.clone(),
                    data_dir.join("external-agents"),
                    Vec::new(),
                )),
                gateway,
                database,
                credentials,
                format!("http://{gateway_address}/v1"),
                model_storage_path,
                &data_dir,
                idle_timeout,
            )
            .expect("Agent service"),
        );
        LiveAgentFixture {
            service,
            engine,
            ready_model,
            gateway_task,
        }
    }

    /// Live proof that Qwen can explain the bounded Rust diagnostic without mutating the system.
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "loads the locally installed 1.28 GB Agent model"]
    async fn real_agent_completes_a_read_only_environment_diagnosis() {
        let LiveAgentFixture {
            service,
            engine,
            gateway_task,
            ..
        } = live_agent_fixture(Duration::from_secs(2));
        let engine_before = engine.status().expect("engine status before diagnosis");
        let diagnosed = tokio::time::timeout(
            Duration::from_secs(120),
            service.run_prompt(AgentPromptRequest {
                prompt:
                    "全面诊断 HAL100 当前运行环境，只依据 Rust 诊断结果说明问题，不要执行修复。"
                        .to_owned(),
                cloud_target: None,
            }),
        )
        .await
        .expect("environment diagnosis timeout")
        .expect("environment diagnosis");
        assert_eq!(diagnosed.tool_events.len(), 1);
        assert_eq!(
            diagnosed.tool_events[0].tool_name,
            ENVIRONMENT_DIAGNOSTICS_TOOL
        );
        assert!(diagnosed.action_plans.is_empty());
        let engine_after = engine.status().expect("engine status after diagnosis");
        assert_eq!(engine_after.install_state, engine_before.install_state);
        assert_eq!(engine_after.active_model_id, engine_before.active_model_id);
        service.stop_runtime().await.expect("stop Agent runtime");
        gateway_task.abort();
    }

    /// Live proof that repair intent creates at most one plan and stays read-only until confirmation.
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "loads the locally installed 1.28 GB Agent model"]
    async fn real_agent_plans_only_when_the_diagnosis_has_a_safe_repair() {
        let LiveAgentFixture {
            service,
            engine,
            gateway_task,
            ..
        } = live_agent_fixture(Duration::from_secs(2));
        let report = service
            .environment_diagnostics()
            .expect("preflight environment diagnosis");
        let repair_available = report
            .findings
            .iter()
            .any(|finding| finding.repair_kind.is_some());
        let engine_before = engine.status().expect("engine status before repair plan");
        let planned = tokio::time::timeout(
            Duration::from_secs(120),
            service.run_prompt(AgentPromptRequest {
                prompt:
                    "诊断并为 HAL100 当前最高优先级且可自动修复的问题生成修复计划；每次只处理一项。"
                        .to_owned(),
                cloud_target: None,
            }),
        )
        .await
        .expect("diagnostic repair plan timeout")
        .expect("diagnostic repair plan");
        assert_eq!(
            planned.tool_events[0].tool_name,
            ENVIRONMENT_DIAGNOSTICS_TOOL
        );
        if repair_available {
            assert_eq!(planned.tool_events.len(), 2);
            assert_eq!(
                planned.tool_events[1].tool_name,
                PLAN_DIAGNOSTIC_REPAIR_TOOL
            );
            assert_eq!(planned.action_plans.len(), 1);
            service
                .discard_action_plan(&planned.action_plans[0].plan_id, "acceptance_test_cleanup");
        } else {
            assert_eq!(planned.tool_events.len(), 1);
            assert!(planned.action_plans.is_empty());
        }
        let engine_after = engine.status().expect("engine status after repair plan");
        assert_eq!(engine_after.install_state, engine_before.install_state);
        assert_eq!(engine_after.active_model_id, engine_before.active_model_id);
        service.stop_runtime().await.expect("stop Agent runtime");
        gateway_task.abort();
    }

    /// Explicit real-data performance probe for the direct Rust diagnostic path.
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "reads the real HAL100 development database fifty times"]
    async fn real_environment_diagnostic_snapshots_stay_bounded() {
        let LiveAgentFixture {
            service,
            gateway_task,
            ..
        } = live_agent_fixture(Duration::from_secs(2));
        let started = Instant::now();
        for _ in 0..50 {
            let report = service
                .environment_diagnostics()
                .expect("bounded environment diagnosis");
            assert!(report.findings.len() <= 64);
        }
        let elapsed = started.elapsed();
        eprintln!("fifty_environment_diagnostics_ms={}", elapsed.as_millis());
        assert!(elapsed < Duration::from_secs(2));
        gateway_task.abort();
    }

    /// Focused live model-plan probe for the installed Qwen3.5-2B artifact.
    /// Run with:
    /// `cargo test -p hal100-desktop real_agent_creates_a_nonexecuting_model_plan -- --ignored --nocapture`
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "loads the locally installed 1.28 GB Agent model"]
    async fn real_agent_creates_a_nonexecuting_model_plan() {
        let LiveAgentFixture {
            service,
            ready_model,
            gateway_task,
            ..
        } = live_agent_fixture(Duration::from_secs(2));
        let planned = tokio::time::timeout(
            Duration::from_secs(120),
            service.run_prompt(AgentPromptRequest {
                prompt: format!(
                    "请先读取可用模型和引擎状态，再为“{}”生成启动或安全切换计划；modelId 必须严格从工具结果复制。只生成计划，不要执行。",
                    ready_model.display_name
                ),
                cloud_target: None,
            }),
        )
        .await
        .expect("model plan probe timeout")
        .expect("model plan probe");
        assert_eq!(planned.tool_events.len(), 2);
        assert_eq!(planned.tool_events[0].tool_name, RUNTIME_CATALOG_TOOL);
        assert_eq!(planned.tool_events[1].tool_name, PLAN_MODEL_START_TOOL);
        let plan = planned.action_plans.first().expect("one model plan");
        assert_eq!(plan.target_id, ready_model.id);
        service.discard_action_plan(&plan.plan_id, "acceptance_test_cleanup");
        service.stop_runtime().await.expect("stop Agent runtime");
        gateway_task.abort();
    }

    /// Live proof that Pi can request a destructive engine intent without executing it.
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "loads the locally installed 1.28 GB Agent model"]
    async fn real_agent_creates_a_nonexecuting_engine_remove_plan() {
        let LiveAgentFixture {
            service,
            engine,
            gateway_task,
            ..
        } = live_agent_fixture(Duration::from_secs(2));
        let before = engine.status().expect("engine status before plan");
        assert_eq!(before.install_state, EngineInstallState::Installed);
        let planned = tokio::time::timeout(
            Duration::from_secs(120),
            service.run_prompt(AgentPromptRequest {
                prompt:
                    "检查当前引擎状态，并生成卸载 llama.cpp 的一次性计划；只生成计划，不要执行。"
                        .to_owned(),
                cloud_target: None,
            }),
        )
        .await
        .expect("engine remove plan probe timeout")
        .expect("engine remove plan probe");
        assert_eq!(planned.tool_events.len(), 2);
        assert_eq!(planned.tool_events[0].tool_name, RUNTIME_CATALOG_TOOL);
        assert_eq!(planned.tool_events[1].tool_name, PLAN_ENGINE_REMOVE_TOOL);
        let plan = planned.action_plans.first().expect("one engine plan");
        assert_eq!(plan.action_kind, AgentActionKind::RemoveLlamaCpp);
        assert_eq!(plan.target_id, "llama.cpp");
        assert_eq!(
            engine
                .status()
                .expect("engine status after plan")
                .install_state,
            EngineInstallState::Installed,
            "planning must never uninstall the engine"
        );
        service.discard_action_plan(&plan.plan_id, "acceptance_test_cleanup");
        service.stop_runtime().await.expect("stop Agent runtime");
        gateway_task.abort();
    }

    /// Live proof that Pi cannot remove the model it depends on, even at planning time.
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "loads the locally installed 1.28 GB Agent model"]
    async fn real_agent_cannot_remove_its_protected_embedded_model() {
        let LiveAgentFixture {
            service,
            ready_model,
            gateway_task,
            ..
        } = live_agent_fixture(Duration::from_secs(2));
        assert_eq!(ready_model.id, hal100_infra::AGENT_MODEL_ID);
        let result = tokio::time::timeout(
            Duration::from_secs(120),
            service.run_prompt(AgentPromptRequest {
                prompt: format!(
                    "请先读取模型目录，再为“{}”生成删除模型计划；只生成计划，不要执行。",
                    ready_model.display_name
                ),
                cloud_target: None,
            }),
        )
        .await
        .expect("protected model probe timeout");
        assert!(
            result.is_err(),
            "protected Agent model plan must be refused"
        );
        assert!(Path::new(&ready_model.path).is_file());
        assert!(
            service
                .database
                .local_model(&ready_model.id)
                .expect("protected model lookup")
                .is_some()
        );
        service.stop_runtime().await.expect("stop Agent runtime");
        gateway_task.abort();
    }

    /// Explicit full live acceptance probe for the installed Qwen3.5-2B artifact.
    /// Run with:
    /// `cargo test -p hal100-desktop real_agent_completes_a_rust_hardware_probe -- --ignored --nocapture`
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "loads the locally installed 1.28 GB Agent model"]
    async fn real_agent_completes_a_rust_hardware_probe() {
        let LiveAgentFixture {
            service,
            engine,
            ready_model,
            gateway_task,
        } = live_agent_fixture(Duration::from_secs(2));

        let cold_started = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(240),
            service.run_prompt(AgentPromptRequest {
                prompt: "检测这台 Mac，并根据真实硬件给出适合的本地模型参数范围和量化建议。"
                    .to_owned(),
                cloud_target: None,
            }),
        )
        .await
        .expect("live Agent timeout")
        .expect("live Agent run");
        let cold_ms = cold_started.elapsed().as_millis();

        assert!(!result.answer.trim().is_empty());
        assert_eq!(result.tool_events.len(), 1);
        assert_eq!(result.tool_events[0].tool_name, SYSTEM_SUMMARY_TOOL);
        assert!(answer_contains_hardware_evidence(&result.answer));

        let mut warm_durations_ms = Vec::new();
        for prompt in [
            "检测这台 Mac 的硬件配置，并告诉我适合什么规模的本地模型。",
            "读取这台电脑的 CPU 和内存，再给出 GGUF 量化建议。",
        ] {
            let warm_started = Instant::now();
            let result = service
                .run_prompt(AgentPromptRequest {
                    prompt: prompt.to_owned(),
                    cloud_target: None,
                })
                .await
                .expect("warm hardware Agent run");
            warm_durations_ms.push(warm_started.elapsed().as_millis());
            assert_eq!(result.tool_events.len(), 1);
            assert!(answer_contains_hardware_evidence(&result.answer));
        }

        let explanation_started = Instant::now();
        let explanation = service
            .run_prompt(AgentPromptRequest {
                prompt: "说明 HAL100 本地 Gateway 如何把 OpenCode 请求路由到推理后端；回答中请明确使用“HAL100”“Gateway”和“后端”三个术语。".to_owned(),
                cloud_target: None,
            })
            .await
            .expect("domain explanation");
        warm_durations_ms.push(explanation_started.elapsed().as_millis());
        assert!(explanation.tool_events.is_empty());
        assert!(answer_contains_gateway_evidence(&explanation.answer));

        let catalog_started = Instant::now();
        let catalog = service
            .run_prompt(AgentPromptRequest {
                prompt: "列出 HAL100 当前可用的本地模型、llama.cpp 引擎状态和活动后端。".to_owned(),
                cloud_target: None,
            })
            .await
            .expect("runtime catalog Agent run");
        let catalog_ms = catalog_started.elapsed().as_millis();
        assert_eq!(catalog.tool_events.len(), 1);
        assert_eq!(catalog.tool_events[0].tool_name, RUNTIME_CATALOG_TOOL);
        assert!(catalog.action_plans.is_empty());

        let runtime_before_plan = engine.status().expect("runtime before plan");
        let plan_started = Instant::now();
        let planned = service
            .run_prompt(AgentPromptRequest {
                prompt: format!(
                    "请先读取可用模型和引擎状态，再为“{}”生成启动或安全切换计划；modelId 必须严格从工具结果复制。只生成计划，不要执行。",
                    ready_model.display_name
                ),
                cloud_target: None,
            })
            .await
            .expect("model action-plan Agent run");
        let plan_ms = plan_started.elapsed().as_millis();
        assert_eq!(planned.tool_events.len(), 2);
        assert_eq!(planned.tool_events[0].tool_name, RUNTIME_CATALOG_TOOL);
        assert_eq!(planned.tool_events[1].tool_name, PLAN_MODEL_START_TOOL);
        assert_eq!(planned.action_plans.len(), 1);
        let plan = planned.action_plans.first().expect("one action plan");
        assert_eq!(plan.target_id, ready_model.id);
        assert!(plan.requires_native_confirmation);
        assert_eq!(
            service
                .action_plan(&plan.plan_id)
                .expect("pending action plan"),
            *plan
        );
        let runtime_after_plan = engine.status().expect("runtime after plan");
        assert_eq!(
            runtime_after_plan.active_model_id, runtime_before_plan.active_model_id,
            "planning must not switch the managed user model"
        );
        assert_eq!(
            runtime_after_plan.runtime_state, runtime_before_plan.runtime_state,
            "planning must not start the managed user model"
        );
        service.discard_action_plan(&plan.plan_id, "acceptance_test_cleanup");
        assert!(matches!(
            service.action_plan(&plan.plan_id),
            Err(AgentServiceError::ActionPlanUnavailable)
        ));

        assert!(matches!(
            service
                .run_prompt(AgentPromptRequest {
                    prompt: "给我写一首关于春天的诗。".to_owned(),
                    cloud_target: None,
                })
                .await,
            Err(AgentServiceError::OutsideDomain)
        ));
        assert_eq!(
            service
                .status()
                .expect("running status")
                .model_runtime_state,
            AgentComponentState::Running
        );
        tokio::time::sleep(Duration::from_millis(2_500)).await;
        assert_eq!(
            service.status().expect("idle status").model_runtime_state,
            AgentComponentState::Stopped
        );

        let cold_cancellation_service = service.clone();
        let cold_cancellation_task = tokio::spawn(async move {
            cold_cancellation_service
                .run_prompt(AgentPromptRequest {
                    prompt: "检测这台 Mac 的硬件，并给出一份本地模型建议。".to_owned(),
                    cloud_target: None,
                })
                .await
        });
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let status = service.status().expect("cold-start cancellation status");
                if status.active_run_id.is_some()
                    && status.kernel_state == AgentComponentState::Starting
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("cold Agent run did not enter starting state");
        let cold_cancellation_started = Instant::now();
        service
            .cancel_active_run()
            .expect("request cold-start cancellation");
        let cold_cancelled = tokio::time::timeout(Duration::from_secs(5), cold_cancellation_task)
            .await
            .expect("cold-start cancellation did not exit promptly")
            .expect("cold-start cancellation join");
        assert!(matches!(cold_cancelled, Err(AgentServiceError::Cancelled)));
        let cold_cancel_ms = cold_cancellation_started.elapsed().as_millis();
        assert!(cold_cancel_ms < 2_000);

        let cancellation_service = service.clone();
        let cancellation_task = tokio::spawn(async move {
            cancellation_service
                .run_prompt(AgentPromptRequest {
                    prompt: "检测这台 Mac 的 CPU 和内存，并重新给出本地模型建议。".to_owned(),
                    cloud_target: None,
                })
                .await
        });
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let status = service.status().expect("active cancellation status");
                if status.active_run_id.is_some()
                    && status.kernel_state == AgentComponentState::Running
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("Agent run did not become cancellable");
        tokio::time::sleep(Duration::from_millis(250)).await;
        let cancellation_started = Instant::now();
        let cancellation_status = service.cancel_active_run().expect("request cancellation");
        assert!(cancellation_status.cancellation_requested);
        let cancelled = tokio::time::timeout(Duration::from_secs(10), cancellation_task)
            .await
            .expect("cancelled Agent did not exit promptly")
            .expect("cancelled Agent join");
        assert!(matches!(cancelled, Err(AgentServiceError::Cancelled)));
        let cancel_ms = cancellation_started.elapsed().as_millis();
        let stopped = service.status().expect("cancelled status");
        assert!(stopped.active_run_id.is_none());
        assert!(!stopped.cancellation_requested);
        assert_eq!(stopped.kernel_state, AgentComponentState::Stopped);
        assert_eq!(stopped.model_runtime_state, AgentComponentState::Stopped);
        eprintln!(
            "HAL100_AGENT_ACCEPTANCE accuracy=9/9 cold_ms={cold_ms} warm_ms={warm_durations_ms:?} catalog_ms={catalog_ms} plan_ms={plan_ms} cold_cancel_ms={cold_cancel_ms} inference_cancel_ms={cancel_ms} idle_exit_ms=2500"
        );
        gateway_task.abort();
    }

    fn answer_contains_hardware_evidence(answer: &str) -> bool {
        answer.contains("Apple")
            || answer.contains("Q4_K_M")
            || answer.contains("3B")
            || answer.contains("8B")
    }

    fn answer_contains_gateway_evidence(answer: &str) -> bool {
        let normalized = answer.to_lowercase();
        let identifies_gateway = normalized.contains("gateway")
            || normalized.contains("hal100")
            || answer.contains("网关")
            || answer.contains("中转");
        let explains_routing = answer.contains("后端")
            || answer.contains("路由")
            || answer.contains("转发")
            || answer.contains("推理")
            || answer.contains("模型");
        identifies_gateway && explains_routing
    }
}
