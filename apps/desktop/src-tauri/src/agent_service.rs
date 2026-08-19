use std::{
    collections::HashSet,
    env, fs,
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use hal100_core::{AgentToolPolicy, AuthorizedAgentTool};
use hal100_infra::{
    AGENT_IDLE_TIMEOUT, AGENT_MODEL_ALIAS, AgentModelRuntime, AgentRuntimeError,
    ClientCredentialError, CredentialRegistry, Database, DatabaseError, EngineManagerError,
    EnvironmentDiagnosticError, EnvironmentDiagnostics, GatewayRouteError, GatewayState,
    LlamaCppManager, ModelRemovalError, ModelRemovalManager, OpenCodeIntegrationError,
    OpenCodeManager, stored_client_credential,
};
use hal100_platform::{
    AgentKernelLaunchSpec, HardwareProbeError, MacOsSystemProbe, SidecarIsolation,
    SidecarLaunchError, prepare_agent_kernel_command,
};
use hal100_protocol::{
    AGENT_RPC_MAX_FRAME_BYTES, AGENT_RPC_VERSION, AgentActionKind, AgentActionPlan,
    AgentActionResult, AgentCloudRunPreview, AgentCloudSessionPreview, AgentCloudSessionStatus,
    AgentCloudTarget, AgentComponentState, AgentPromptRequest, AgentProviderProtocol,
    AgentRpcEnvelope, AgentRpcFrameError, AgentRunCompletedPayload, AgentRunResult,
    AgentRunStartPayload, AgentRuntimeCatalog, AgentRuntimeModel, AgentStatus, AgentSystemSummary,
    AgentToolEvent, BackendKind, DiagnosticRepairKind, ENVIRONMENT_DIAGNOSTICS_TOOL,
    EngineInstallState, EnvironmentDiagnosticReport, LocalModelState, ModelRemovalKind,
    OPENCODE_STATUS_TOOL, OpenCodeIntegrationState, PLAN_DIAGNOSTIC_REPAIR_TOOL,
    PLAN_ENGINE_INSTALL_TOOL, PLAN_ENGINE_REMOVE_TOOL, PLAN_MODEL_REMOVAL_TOOL,
    PLAN_MODEL_START_TOOL, PLAN_OPENCODE_CONFIGURATION_TOOL, RUNTIME_CATALOG_TOOL,
    SYSTEM_SUMMARY_TOOL, ToolCallRequestPayload, ToolCallResultPayload, encode_agent_rpc_frame,
};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

const PINNED_NODE_VERSION: &str = "v24.18.0";
const AGENT_CLIENT_APP_ID: &str = "hal100-agent";
const CLOUD_AGENT_CLIENT_APP_ID: &str = "hal100-agent-cloud";
const CLOUD_AGENT_ROUTE_PREFIX: &str = "hal100-agent-cloud-";
const MAX_PROMPT_BYTES: usize = 4 * 1024;
const MAX_ANSWER_BYTES: usize = 64 * 1024;
const MAX_TOOL_CALLS_PER_RUN: usize = 4;
const MAX_ACTION_PLAN_ID_CHARS: usize = 128;
const SIDECAR_RESPONSE_TIMEOUT: Duration = Duration::from_secs(180);
const SIDECAR_EXIT_TIMEOUT: Duration = Duration::from_secs(5);
const SIDECAR_CANCELLATION_POLL: Duration = Duration::from_millis(100);
const ACTION_PLAN_TTL_MS: i64 = 5 * 60 * 1_000;

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
    ModelRemoval(#[from] ModelRemovalError),
    #[error(transparent)]
    Diagnostics(#[from] EnvironmentDiagnosticError),
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
            Self::ModelRemoval(_) => "model_removal_failed",
            Self::Diagnostics(_) => "environment_diagnostics_failed",
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

pub struct AgentService {
    runtime: Arc<AgentModelRuntime>,
    engine: Arc<LlamaCppManager>,
    open_code: Arc<OpenCodeManager>,
    model_removal: Arc<ModelRemovalManager>,
    diagnostics: Arc<EnvironmentDiagnostics>,
    gateway: GatewayState,
    database: Arc<Database>,
    credentials: CredentialRegistry,
    gateway_base_url: String,
    model_storage_path: PathBuf,
    workspace_root: PathBuf,
    session_root: PathBuf,
    node_binary: PathBuf,
    entrypoint: PathBuf,
    run_lock: AsyncMutex<()>,
    kernel_state: Mutex<KernelState>,
    active_run: Arc<Mutex<Option<ActiveRun>>>,
    pending_action_plan: Arc<Mutex<Option<PendingAgentAction>>>,
    cloud_session: Mutex<Option<ActiveCloudSession>>,
    idle_generation: AtomicU64,
    idle_timeout: Duration,
}

struct KernelState {
    state: AgentComponentState,
    last_error_code: Option<String>,
}

struct ActiveRun {
    run_id: String,
    cancelled: Arc<AtomicBool>,
}

#[derive(Clone)]
struct PendingAgentAction {
    plan: AgentActionPlan,
    executor: AgentActionExecutor,
}

#[derive(Clone)]
enum AgentActionExecutor {
    StartOrSwitchModel {
        model_id: String,
    },
    InstallLlamaCpp {
        engine_plan_id: String,
    },
    RemoveLlamaCpp {
        engine_plan_id: String,
    },
    RemoveModel {
        removal_plan_id: String,
        model_id: String,
    },
    ConfigureOpenCode {
        configuration_plan_id: String,
    },
}

#[derive(Clone)]
struct ActiveCloudSession {
    target: AgentCloudTarget,
    activated_at_ms: i64,
    last_error_code: Option<String>,
}

struct ResolvedAgentProvider {
    protocol: AgentProviderProtocol,
    model_id: String,
    model_name: String,
    client_app_id: &'static str,
    backend_id: Option<String>,
    uses_local_runtime: bool,
    session_bound: bool,
}

impl AgentService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        runtime: Arc<AgentModelRuntime>,
        engine: Arc<LlamaCppManager>,
        open_code: Arc<OpenCodeManager>,
        model_removal: Arc<ModelRemovalManager>,
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
            model_removal,
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
        model_removal: Arc<ModelRemovalManager>,
        gateway: GatewayState,
        database: Arc<Database>,
        credentials: CredentialRegistry,
        gateway_base_url: String,
        model_storage_path: PathBuf,
        data_dir: &Path,
        idle_timeout: Duration,
    ) -> Result<Self, AgentServiceError> {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .map_err(|_| AgentServiceError::KernelUnavailable)?;
        let entrypoint = workspace_root.join("sidecars/agent-kernel/dist/index.js");
        if !entrypoint.is_file() {
            return Err(AgentServiceError::KernelUnavailable);
        }
        let node_binary = resolve_node_binary(&workspace_root)?;
        let session_root = data_dir.join("agent").join("sessions");
        fs::create_dir_all(&session_root).map_err(AgentServiceError::Io)?;
        set_owner_only_directory(&session_root).map_err(AgentServiceError::Io)?;

        let diagnostics = Arc::new(EnvironmentDiagnostics::new(
            database.clone(),
            engine.clone(),
            open_code.clone(),
            gateway.clone(),
        ));
        Ok(Self {
            runtime,
            engine,
            open_code,
            model_removal,
            diagnostics,
            gateway,
            database,
            credentials,
            gateway_base_url,
            model_storage_path,
            workspace_root,
            session_root,
            node_binary,
            entrypoint,
            run_lock: AsyncMutex::new(()),
            kernel_state: Mutex::new(KernelState {
                state: AgentComponentState::Stopped,
                last_error_code: None,
            }),
            active_run: Arc::new(Mutex::new(None)),
            pending_action_plan: Arc::new(Mutex::new(None)),
            cloud_session: Mutex::new(None),
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
        let active_run = self
            .active_run
            .lock()
            .map_err(|_| AgentServiceError::KernelUnavailable)?;
        status.active_run_id = active_run.as_ref().map(|run| run.run_id.clone());
        status.cancellation_requested = active_run
            .as_ref()
            .is_some_and(|run| run.cancelled.load(Ordering::Acquire));
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
        let (record, kind, _) = self.resolve_cloud_target(target)?;
        Ok(AgentCloudRunPreview {
            backend_id: record.id,
            backend_name: record.display_name,
            backend_kind: kind,
            api_root: record.api_root,
            model: target.model.trim().to_owned(),
            prompt_bytes: u32::try_from(prompt.len()).unwrap_or(u32::MAX),
            sends_system_instructions: true,
            may_send_tool_results: true,
            sends_credentials_to_sidecar: false,
            sends_local_paths: false,
            confirmation_summary:
                "本次任务会把任务文字、HAL100 固定系统指令及本次任务需要的只读工具结果发送给所选云端后端；云端 API Key 与本地文件路径不会发送给 Agent Sidecar。"
                    .to_owned(),
        })
    }

    pub fn preview_cloud_session(
        &self,
        target: &AgentCloudTarget,
    ) -> Result<AgentCloudSessionPreview, AgentServiceError> {
        let (record, kind, _) = self.resolve_cloud_target(target)?;
        Ok(AgentCloudSessionPreview {
            backend_id: record.id,
            backend_name: record.display_name,
            backend_kind: kind,
            api_root: record.api_root,
            model: target.model.clone(),
            sends_future_prompts: true,
            sends_system_instructions: true,
            may_send_tool_results: true,
            stores_conversation_history: false,
            sends_credentials_to_sidecar: false,
            sends_local_paths: false,
            confirmation_summary:
                "启用后，当前应用会话中后续每项 HAL100 Agent 任务的文字、固定系统指令及任务需要的只读工具结果会发送到所选云端后端；不会保存对话历史，不会把云端 API Key 或本地文件路径交给 Sidecar。明确退出或重启 HAL100 后恢复本地默认。"
                    .to_owned(),
        })
    }

    pub fn cloud_session_status(&self) -> Result<AgentCloudSessionStatus, AgentServiceError> {
        let session = self
            .cloud_session
            .lock()
            .map_err(|_| AgentServiceError::KernelUnavailable)?
            .clone();
        let Some(session) = session else {
            return Ok(inactive_cloud_session_status());
        };
        match self.resolve_cloud_target(&session.target) {
            Ok((record, kind, protocol)) => Ok(AgentCloudSessionStatus {
                active: true,
                available: true,
                backend_id: Some(record.id),
                backend_name: Some(record.display_name),
                backend_kind: Some(kind),
                api_root: Some(record.api_root),
                model: Some(session.target.model),
                provider_protocol: Some(protocol),
                activated_at_ms: Some(session.activated_at_ms),
                last_error_code: session.last_error_code,
            }),
            Err(error) => Ok(AgentCloudSessionStatus {
                active: true,
                available: false,
                backend_id: Some(session.target.backend_id),
                backend_name: None,
                backend_kind: None,
                api_root: None,
                model: Some(session.target.model),
                provider_protocol: None,
                activated_at_ms: Some(session.activated_at_ms),
                last_error_code: session
                    .last_error_code
                    .or_else(|| Some(error.code().to_owned())),
            }),
        }
    }

    pub fn start_cloud_session(
        &self,
        target: AgentCloudTarget,
    ) -> Result<AgentCloudSessionStatus, AgentServiceError> {
        let _run_guard = self
            .run_lock
            .try_lock()
            .map_err(|_| AgentServiceError::Busy)?;
        let (record, kind, protocol) = self.resolve_cloud_target(&target)?;
        let activated_at_ms = now_ms();
        let mut session = self
            .cloud_session
            .lock()
            .map_err(|_| AgentServiceError::KernelUnavailable)?;
        if session.is_some() {
            return Err(AgentServiceError::CloudSessionAlreadyActive);
        }
        self.database.insert_audit_event(
            "agent_cloud_session_started",
            "agent_cloud_session",
            &record.id,
            &json!({
                "provider": "cloud_session",
                "backendId": &record.id,
                "model": &target.model,
            })
            .to_string(),
            activated_at_ms,
        )?;
        *session = Some(ActiveCloudSession {
            target: target.clone(),
            activated_at_ms,
            last_error_code: None,
        });
        Ok(AgentCloudSessionStatus {
            active: true,
            available: true,
            backend_id: Some(record.id),
            backend_name: Some(record.display_name),
            backend_kind: Some(kind),
            api_root: Some(record.api_root),
            model: Some(target.model),
            provider_protocol: Some(protocol),
            activated_at_ms: Some(activated_at_ms),
            last_error_code: None,
        })
    }

    pub fn stop_cloud_session(&self) -> Result<AgentCloudSessionStatus, AgentServiceError> {
        let _run_guard = self
            .run_lock
            .try_lock()
            .map_err(|_| AgentServiceError::Busy)?;
        let session = self
            .cloud_session
            .lock()
            .map_err(|_| AgentServiceError::KernelUnavailable)?
            .take()
            .ok_or(AgentServiceError::NoActiveCloudSession)?;
        let _ = self.database.insert_audit_event(
            "agent_cloud_session_stopped",
            "agent_cloud_session",
            &session.target.backend_id,
            &json!({
                "provider": "cloud_session",
                "backendId": session.target.backend_id,
                "model": session.target.model,
            })
            .to_string(),
            now_ms(),
        );
        Ok(inactive_cloud_session_status())
    }

    fn record_cloud_session_error(&self, error_code: &'static str) {
        if let Ok(mut session) = self.cloud_session.lock()
            && let Some(session) = session.as_mut()
        {
            session.last_error_code = Some(error_code.to_owned());
        }
    }

    fn clear_cloud_session_error(&self) {
        if let Ok(mut session) = self.cloud_session.lock()
            && let Some(session) = session.as_mut()
        {
            session.last_error_code = None;
        }
    }

    fn resolve_cloud_target(
        &self,
        target: &AgentCloudTarget,
    ) -> Result<
        (
            hal100_infra::StoredBackendRecord,
            BackendKind,
            AgentProviderProtocol,
        ),
        AgentServiceError,
    > {
        let backend_id = target.backend_id.trim();
        let model = target.model.trim();
        if backend_id != target.backend_id
            || model != target.model
            || backend_id.is_empty()
            || backend_id.len() > 128
            || !backend_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
            || model.is_empty()
            || model.len() > 256
            || model.chars().any(char::is_control)
        {
            return Err(AgentServiceError::InvalidCloudTarget);
        }
        let record = self
            .database
            .backends()?
            .into_iter()
            .find(|record| record.id == backend_id && record.enabled)
            .ok_or(AgentServiceError::CloudBackendUnavailable)?;
        let (kind, protocol) = match record.kind.as_str() {
            "external_openai" => (
                BackendKind::ExternalOpenAi,
                AgentProviderProtocol::CloudOpenAi,
            ),
            "external_anthropic" => (
                BackendKind::ExternalAnthropic,
                AgentProviderProtocol::CloudAnthropic,
            ),
            _ => return Err(AgentServiceError::CloudBackendUnsupported),
        };
        if record.credential_id.as_deref().is_none_or(str::is_empty) {
            return Err(AgentServiceError::CloudCredentialMissing);
        }
        if !self
            .gateway
            .routing_snapshot()
            .backend_ids
            .iter()
            .any(|loaded_id| loaded_id == backend_id)
        {
            return Err(AgentServiceError::CloudBackendUnavailable);
        }
        Ok((record, kind, protocol))
    }

    fn resolve_agent_provider(
        &self,
        request: &AgentPromptRequest,
    ) -> Result<ResolvedAgentProvider, AgentServiceError> {
        let (target, session_bound) = if let Some(target) = request.cloud_target.as_ref() {
            (Some(target.clone()), false)
        } else {
            (
                self.cloud_session
                    .lock()
                    .map_err(|_| AgentServiceError::KernelUnavailable)?
                    .as_ref()
                    .map(|session| session.target.clone()),
                true,
            )
        };
        let Some(target) = target else {
            return Ok(ResolvedAgentProvider {
                protocol: AgentProviderProtocol::LocalOpenAi,
                model_id: AGENT_MODEL_ALIAS.to_owned(),
                model_name: "Qwen3.5-2B Q4_K_M".to_owned(),
                client_app_id: AGENT_CLIENT_APP_ID,
                backend_id: None,
                uses_local_runtime: true,
                session_bound: false,
            });
        };
        let (record, _, protocol) = match self.resolve_cloud_target(&target) {
            Ok(resolved) => resolved,
            Err(error) => {
                if session_bound {
                    self.record_cloud_session_error(error.code());
                }
                return Err(error);
            }
        };
        Ok(ResolvedAgentProvider {
            protocol,
            model_id: format!("{CLOUD_AGENT_ROUTE_PREFIX}{}", Uuid::new_v4().simple()),
            model_name: target.model,
            client_app_id: CLOUD_AGENT_CLIENT_APP_ID,
            backend_id: Some(record.id),
            uses_local_runtime: false,
            session_bound,
        })
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
        let provider = self.resolve_agent_provider(&request)?;
        let requires_system_summary = prompt_requires_system_summary(&prompt);
        let requires_model_start_plan = prompt_requires_model_start_plan(&prompt);
        let requires_model_removal_plan = prompt_requires_model_removal_plan(&prompt);
        let requires_engine_install_plan = prompt_requires_engine_install_plan(&prompt);
        let requires_engine_remove_plan = prompt_requires_engine_remove_plan(&prompt);
        let requires_opencode_configuration_plan =
            prompt_requires_opencode_configuration_plan(&prompt);
        let has_explicit_action = requires_model_start_plan
            || requires_model_removal_plan
            || requires_engine_install_plan
            || requires_engine_remove_plan
            || requires_opencode_configuration_plan;
        let requires_diagnostic_repair_plan =
            !has_explicit_action && prompt_requires_diagnostic_repair_plan(&prompt);
        let requires_environment_diagnostics =
            requires_diagnostic_repair_plan || prompt_requires_environment_diagnostics(&prompt);
        let requires_opencode_status =
            requires_opencode_configuration_plan || prompt_requires_opencode_status(&prompt);
        let requires_runtime_catalog = requires_model_start_plan
            || requires_model_removal_plan
            || requires_engine_install_plan
            || requires_engine_remove_plan
            || prompt_requires_runtime_catalog(&prompt);
        if provider.uses_local_runtime {
            self.idle_generation.fetch_add(1, Ordering::AcqRel);
        }
        let started_at_ms = now_ms();
        let run_id = format!("agent-run-{}", Uuid::new_v4().simple());
        self.discard_any_action_plan("superseded_by_new_run");
        let cancellation = Arc::new(AtomicBool::new(false));
        {
            let mut active_run = self
                .active_run
                .lock()
                .map_err(|_| AgentServiceError::KernelUnavailable)?;
            *active_run = Some(ActiveRun {
                run_id: run_id.clone(),
                cancelled: cancellation.clone(),
            });
        }
        let _active_run = ActiveRunGuard {
            active_run: self.active_run.clone(),
            run_id: run_id.clone(),
        };
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
            requirements: AgentRunRequirements {
                system_summary: requires_system_summary,
                runtime_catalog: requires_runtime_catalog,
                model_start_plan: requires_model_start_plan,
                model_removal_plan: requires_model_removal_plan,
                environment_diagnostics: requires_environment_diagnostics,
                diagnostic_repair_plan: requires_diagnostic_repair_plan,
                engine_install_plan: requires_engine_install_plan,
                engine_remove_plan: requires_engine_remove_plan,
                opencode_status: requires_opencode_status,
                opencode_configuration_plan: requires_opencode_configuration_plan,
            },
            gateway_base_url: self.gateway_base_url.clone(),
            api_key: client_key,
            model_id: provider.model_id,
            provider_protocol: provider.protocol,
            model_storage_path: self.model_storage_path.clone(),
            workspace_root: self.workspace_root.clone(),
            session_base: self.session_root.clone(),
            node_binary: self.node_binary.clone(),
            entrypoint: self.entrypoint.clone(),
            database: self.database.clone(),
            engine: self.engine.clone(),
            open_code: self.open_code.clone(),
            model_removal: self.model_removal.clone(),
            diagnostics: self.diagnostics.clone(),
            gateway: self.gateway.clone(),
            pending_action_plan: self.pending_action_plan.clone(),
            cancellation,
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
        {
            let active_run = self
                .active_run
                .lock()
                .map_err(|_| AgentServiceError::KernelUnavailable)?;
            let active_run = active_run.as_ref().ok_or(AgentServiceError::NoActiveRun)?;
            active_run.cancelled.store(true, Ordering::Release);
        }
        self.status()
    }

    pub fn action_plan(&self, plan_id: &str) -> Result<AgentActionPlan, AgentServiceError> {
        let pending = self
            .pending_action_plan
            .lock()
            .map_err(|_| AgentServiceError::ActionPlanUnavailable)?;
        clone_valid_action_plan(&pending, plan_id, now_ms())
    }

    pub fn discard_action_plan(&self, plan_id: &str, reason: &'static str) {
        let discarded = if let Ok(mut pending) = self.pending_action_plan.lock()
            && pending
                .as_ref()
                .is_some_and(|pending| pending.plan.plan_id == plan_id)
        {
            pending.take()
        } else {
            None
        };
        if let Some(discarded) = discarded {
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
        let discarded = self
            .pending_action_plan
            .lock()
            .ok()
            .and_then(|mut pending| pending.take());
        if let Some(discarded) = discarded {
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
            AgentActionExecutor::ConfigureOpenCode {
                configuration_plan_id,
            } => {
                let _ = self
                    .open_code
                    .discard_configuration_plan(configuration_plan_id);
            }
        }
    }

    pub async fn apply_action_plan(
        self: &Arc<Self>,
        plan_id: &str,
    ) -> Result<AgentActionResult, AgentServiceError> {
        let _run_guard = self
            .run_lock
            .try_lock()
            .map_err(|_| AgentServiceError::Busy)?;
        let pending = {
            let mut pending = self
                .pending_action_plan
                .lock()
                .map_err(|_| AgentServiceError::ActionPlanUnavailable)?;
            take_valid_action_plan(&mut pending, plan_id, now_ms())?
        };
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
            AgentActionExecutor::ConfigureOpenCode {
                configuration_plan_id,
            } => {
                let open_code = self.open_code.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    open_code.apply_configuration(&configuration_plan_id)
                })
                .await
                .map_err(|_| AgentServiceError::Join)
                .and_then(|result| result.map_err(AgentServiceError::from))
                .map(|_| ("OpenCode 已通过 HAL100 Gateway 接入".to_owned(), None))
            }
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

fn clone_valid_action_plan(
    pending: &Option<PendingAgentAction>,
    plan_id: &str,
    current_time_ms: i64,
) -> Result<AgentActionPlan, AgentServiceError> {
    if plan_id.is_empty() || plan_id.chars().count() > MAX_ACTION_PLAN_ID_CHARS {
        return Err(AgentServiceError::ActionPlanUnavailable);
    }
    let plan = pending
        .as_ref()
        .map(|pending| &pending.plan)
        .filter(|plan| plan.plan_id == plan_id && plan.requires_native_confirmation)
        .cloned()
        .ok_or(AgentServiceError::ActionPlanUnavailable)?;
    if current_time_ms > plan.expires_at_ms {
        return Err(AgentServiceError::ActionPlanExpired);
    }
    Ok(plan)
}

fn take_valid_action_plan(
    pending: &mut Option<PendingAgentAction>,
    plan_id: &str,
    current_time_ms: i64,
) -> Result<PendingAgentAction, AgentServiceError> {
    clone_valid_action_plan(pending, plan_id, current_time_ms)?;
    Ok(pending.take().expect("validated Agent action plan"))
}

fn action_kind_key(kind: AgentActionKind) -> &'static str {
    match kind {
        AgentActionKind::StartOrSwitchModel => "start_or_switch_model",
        AgentActionKind::InstallLlamaCpp => "install_llama_cpp",
        AgentActionKind::RemoveLlamaCpp => "remove_llama_cpp",
        AgentActionKind::RemoveModel => "remove_model",
        AgentActionKind::ConfigureOpenCode => "configure_opencode",
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
    model_storage_path: PathBuf,
    workspace_root: PathBuf,
    session_base: PathBuf,
    node_binary: PathBuf,
    entrypoint: PathBuf,
    database: Arc<Database>,
    engine: Arc<LlamaCppManager>,
    open_code: Arc<OpenCodeManager>,
    model_removal: Arc<ModelRemovalManager>,
    diagnostics: Arc<EnvironmentDiagnostics>,
    gateway: GatewayState,
    pending_action_plan: Arc<Mutex<Option<PendingAgentAction>>>,
    cancellation: Arc<AtomicBool>,
}

#[derive(Clone, Copy)]
struct AgentRunRequirements {
    system_summary: bool,
    runtime_catalog: bool,
    model_start_plan: bool,
    model_removal_plan: bool,
    environment_diagnostics: bool,
    diagnostic_repair_plan: bool,
    engine_install_plan: bool,
    engine_remove_plan: bool,
    opencode_status: bool,
    opencode_configuration_plan: bool,
}

struct PendingActionPresentation {
    tool_name: &'static str,
    label: &'static str,
    summary: String,
}

struct SidecarRunOutput {
    answer: String,
    tool_events: Vec<AgentToolEvent>,
    action_plans: Vec<AgentActionPlan>,
}

fn run_sidecar_once(input: SidecarRunInput) -> Result<SidecarRunOutput, AgentServiceError> {
    if input.cancellation.load(Ordering::Acquire) {
        return Err(AgentServiceError::Cancelled);
    }
    let session_directory = input
        .session_base
        .join(format!("session-{}", Uuid::new_v4().simple()));
    let _session = SessionDirectory::create(session_directory.clone())?;
    let spec = AgentKernelLaunchSpec {
        runtime_binary: input.node_binary.clone(),
        entrypoint: input.entrypoint.clone(),
        working_directory: input.workspace_root.join("sidecars/agent-kernel"),
        workspace_root: input.workspace_root.clone(),
        session_root: session_directory,
        isolation: SidecarIsolation::ProcessBoundaryOnly,
        arguments: Vec::new(),
    };
    let mut command = prepare_agent_kernel_command(&spec)?;
    let mut child = command
        .spawn()
        .map_err(|_| AgentServiceError::KernelStart)?;
    let mut stdin = child.stdin.take().ok_or(AgentServiceError::KernelStart)?;
    let stdout = child.stdout.take().ok_or(AgentServiceError::KernelStart)?;
    let stderr = child.stderr.take().ok_or(AgentServiceError::KernelStart)?;
    let (sender, receiver) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut stdout = stdout;
        loop {
            match read_envelope(&mut stdout) {
                Ok(envelope) => {
                    if sender.send(Ok(envelope)).is_err() {
                        break;
                    }
                }
                Err(error) if error.kind() == ErrorKind::UnexpectedEof => break,
                Err(error) => {
                    let _ = sender.send(Err(error));
                    break;
                }
            }
        }
    });
    let stderr_reader = thread::spawn(move || {
        let mut stderr = stderr;
        let _ = std::io::copy(&mut stderr, &mut std::io::sink());
    });

    let exchange = exchange_with_sidecar(&input, &mut stdin, &receiver);
    drop(stdin);
    if exchange.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        let _ = reader.join();
        let _ = stderr_reader.join();
        return exchange;
    }
    let child_result = wait_for_child(&mut child);
    let _ = reader.join();
    let _ = stderr_reader.join();
    child_result?;
    exchange
}

fn exchange_with_sidecar(
    input: &SidecarRunInput,
    stdin: &mut ChildStdin,
    receiver: &mpsc::Receiver<Result<AgentRpcEnvelope, std::io::Error>>,
) -> Result<SidecarRunOutput, AgentServiceError> {
    let ping_id = format!("ping-{}", input.run_id);
    write_envelope(
        stdin,
        &AgentRpcEnvelope {
            protocol_version: AGENT_RPC_VERSION,
            id: ping_id.clone(),
            kind: "system.ping".to_owned(),
            payload: json!({}),
        },
    )?;
    let pong = receive_envelope(receiver, &input.cancellation)?;
    validate_envelope(&pong)?;
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

    let payload = AgentRunStartPayload {
        prompt: input.prompt.clone(),
        requires_system_summary: input.requirements.system_summary,
        requires_runtime_catalog: input.requirements.runtime_catalog,
        requires_model_start_plan: input.requirements.model_start_plan,
        requires_model_removal_plan: input.requirements.model_removal_plan,
        requires_environment_diagnostics: input.requirements.environment_diagnostics,
        requires_diagnostic_repair_plan: input.requirements.diagnostic_repair_plan,
        requires_engine_install_plan: input.requirements.engine_install_plan,
        requires_engine_remove_plan: input.requirements.engine_remove_plan,
        requires_opencode_status: input.requirements.opencode_status,
        requires_opencode_configuration_plan: input.requirements.opencode_configuration_plan,
        gateway_base_url: input.gateway_base_url.clone(),
        api_key: input.api_key.clone(),
        model_id: input.model_id.clone(),
        provider_protocol: input.provider_protocol,
    };
    write_envelope(
        stdin,
        &AgentRpcEnvelope {
            protocol_version: AGENT_RPC_VERSION,
            id: input.run_id.clone(),
            kind: "agent.run.start".to_owned(),
            payload: serde_json::to_value(payload)
                .map_err(|_| AgentServiceError::InvalidProtocol)?,
        },
    )?;

    let policy = AgentToolPolicy;
    let mut tool_events = Vec::new();
    let mut action_plans = Vec::new();
    let mut diagnostic_report: Option<EnvironmentDiagnosticReport> = None;
    let mut seen_tool_calls = HashSet::new();
    let mut last_tool_failure_code: Option<String> = None;
    loop {
        let envelope = receive_envelope(receiver, &input.cancellation)?;
        validate_envelope(&envelope)?;
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
                let result = match policy.authorize(&request) {
                    Ok(AuthorizedAgentTool::InspectSystemSummary) => {
                        let profile =
                            MacOsSystemProbe.hardware_profile(&input.model_storage_path)?;
                        let summary = AgentSystemSummary {
                            source: "rust_macos_probe".to_owned(),
                            platform: "macOS".to_owned(),
                            architecture: "Apple Silicon".to_owned(),
                            chip: profile.chip,
                            model_identifier: profile.model_identifier,
                            total_unified_memory_bytes: profile.total_unified_memory_bytes,
                            physical_cpu_cores: profile.physical_cpu_cores,
                            logical_cpu_cores: profile.logical_cpu_cores,
                            model_storage_available_bytes: profile.model_storage_available_bytes,
                            recommendation_summary: profile.recommendation.summary,
                            recommended_parameter_range: profile.recommendation.parameter_range,
                            recommended_quantization: profile.recommendation.quantization,
                        };
                        tool_events.push(AgentToolEvent {
                            tool_call_id: request.tool_call_id.clone(),
                            tool_name: SYSTEM_SUMMARY_TOOL.to_owned(),
                            label: "检测这台 Mac".to_owned(),
                            status: "completed".to_owned(),
                            summary: "Rust 已按需读取芯片、统一内存、CPU 与模型目录可用空间。"
                                .to_owned(),
                        });
                        ToolCallResultPayload::success(
                            &request.tool_call_id,
                            serde_json::to_value(summary)
                                .map_err(|_| AgentServiceError::InvalidProtocol)?,
                        )
                    }
                    Ok(AuthorizedAgentTool::InspectRuntimeCatalog) => {
                        let catalog = build_runtime_catalog(input)?;
                        tool_events.push(AgentToolEvent {
                            tool_call_id: request.tool_call_id.clone(),
                            tool_name: RUNTIME_CATALOG_TOOL.to_owned(),
                            label: "读取 HAL100 运行环境".to_owned(),
                            status: "completed".to_owned(),
                            summary: format!(
                                "Rust 已读取引擎、活动路由和 {} 个本地模型的脱敏状态。",
                                catalog.models.len()
                            ),
                        });
                        ToolCallResultPayload::success(
                            &request.tool_call_id,
                            serde_json::to_value(catalog)
                                .map_err(|_| AgentServiceError::InvalidProtocol)?,
                        )
                    }
                    Ok(AuthorizedAgentTool::InspectEnvironmentDiagnostics) => {
                        match input.diagnostics.run() {
                            Ok(report) => {
                                tool_events.push(AgentToolEvent {
                                    tool_call_id: request.tool_call_id.clone(),
                                    tool_name: ENVIRONMENT_DIAGNOSTICS_TOOL.to_owned(),
                                    label: "诊断 HAL100 运行环境".to_owned(),
                                    status: "completed".to_owned(),
                                    summary: format!(
                                        "Rust 已完成一次按需诊断：{} 个错误、{} 个警告；未读取原始日志或执行完整模型哈希。",
                                        report.error_count, report.warning_count
                                    ),
                                });
                                diagnostic_report = Some(report.clone());
                                ToolCallResultPayload::success(
                                    &request.tool_call_id,
                                    serde_json::to_value(report)
                                        .map_err(|_| AgentServiceError::InvalidProtocol)?,
                                )
                            }
                            Err(error) => ToolCallResultPayload::error(
                                &request.tool_call_id,
                                AgentServiceError::from(error).code(),
                                "Rust could not complete the bounded environment diagnosis",
                            ),
                        }
                    }
                    Ok(AuthorizedAgentTool::PlanModelStart { model_id }) => {
                        let catalog_completed = tool_events
                            .iter()
                            .any(|event| event.tool_name == RUNTIME_CATALOG_TOOL);
                        if !catalog_completed {
                            ToolCallResultPayload::error(
                                &request.tool_call_id,
                                "runtime_catalog_required",
                                "inspect_runtime_catalog must complete before planning a model start",
                            )
                        } else if !action_plans.is_empty() {
                            ToolCallResultPayload::error(
                                &request.tool_call_id,
                                "action_already_planned",
                                "only one mutating action plan is allowed per Agent run",
                            )
                        } else if let Some(pending) = build_model_start_plan(input, &model_id)? {
                            let target_name = pending.plan.target_name.clone();
                            register_pending_action(
                                input,
                                pending,
                                &request,
                                PendingActionPresentation {
                                    tool_name: PLAN_MODEL_START_TOOL,
                                    label: "生成模型启动或切换计划",
                                    summary: format!(
                                        "已为“{target_name}”生成一次性计划；尚未执行，必须通过 Rust 原生确认。"
                                    ),
                                },
                                &mut tool_events,
                                &mut action_plans,
                            )?
                        } else {
                            ToolCallResultPayload::error(
                                &request.tool_call_id,
                                "model_start_unavailable",
                                "the requested model is not ready or llama.cpp is not installed",
                            )
                        }
                    }
                    Ok(AuthorizedAgentTool::PlanModelRemoval { model_id }) => {
                        let catalog_completed = tool_events
                            .iter()
                            .any(|event| event.tool_name == RUNTIME_CATALOG_TOOL);
                        if !catalog_completed {
                            ToolCallResultPayload::error(
                                &request.tool_call_id,
                                "runtime_catalog_required",
                                "inspect_runtime_catalog must complete before planning model removal",
                            )
                        } else if !action_plans.is_empty() {
                            ToolCallResultPayload::error(
                                &request.tool_call_id,
                                "action_already_planned",
                                "only one mutating action plan is allowed per Agent run",
                            )
                        } else {
                            match build_model_removal_plan(input, &model_id) {
                                Ok(pending) => {
                                    let target_name = pending.plan.target_name.clone();
                                    register_pending_action(
                                        input,
                                        pending,
                                        &request,
                                        PendingActionPresentation {
                                            tool_name: PLAN_MODEL_REMOVAL_TOOL,
                                            label: "生成模型移除计划",
                                            summary: format!(
                                                "已为“{target_name}”生成一次性移除计划；尚未移动文件或删除索引，必须通过 Rust 原生确认。"
                                            ),
                                        },
                                        &mut tool_events,
                                        &mut action_plans,
                                    )?
                                }
                                Err(error) => ToolCallResultPayload::error(
                                    &request.tool_call_id,
                                    error.code(),
                                    "Rust refused to create an unsafe model removal plan",
                                ),
                            }
                        }
                    }
                    Ok(AuthorizedAgentTool::PlanDiagnosticRepair {
                        report_id,
                        finding_id,
                    }) => {
                        let report_completed = tool_events
                            .iter()
                            .any(|event| event.tool_name == ENVIRONMENT_DIAGNOSTICS_TOOL);
                        if !report_completed {
                            ToolCallResultPayload::error(
                                &request.tool_call_id,
                                "environment_diagnostics_required",
                                "inspect_environment_diagnostics must complete before planning a repair",
                            )
                        } else if !action_plans.is_empty() {
                            ToolCallResultPayload::error(
                                &request.tool_call_id,
                                "action_already_planned",
                                "only one mutating action plan is allowed per Agent run",
                            )
                        } else if diagnostic_report
                            .as_ref()
                            .is_none_or(|report| report.report_id != report_id)
                        {
                            ToolCallResultPayload::error(
                                &request.tool_call_id,
                                "diagnostic_report_mismatch",
                                "reportId must match the diagnosis completed in this Agent run",
                            )
                        } else {
                            let report = diagnostic_report
                                .as_ref()
                                .expect("validated diagnostic report");
                            match build_diagnostic_repair_plan(input, report, &finding_id) {
                                Ok(pending) => {
                                    let target_name = pending.plan.target_name.clone();
                                    register_pending_action(
                                        input,
                                        pending,
                                        &request,
                                        PendingActionPresentation {
                                            tool_name: PLAN_DIAGNOSTIC_REPAIR_TOOL,
                                            label: "生成单项诊断修复计划",
                                            summary: format!(
                                                "已为“{target_name}”生成一次性修复计划；尚未执行，必须通过 Rust 原生确认，执行后会重新诊断。"
                                            ),
                                        },
                                        &mut tool_events,
                                        &mut action_plans,
                                    )?
                                }
                                Err(error) => ToolCallResultPayload::error(
                                    &request.tool_call_id,
                                    error.code(),
                                    "Rust refused to create a stale or unsafe diagnostic repair plan",
                                ),
                            }
                        }
                    }
                    Ok(AuthorizedAgentTool::PlanEngineInstall) => {
                        let catalog_completed = tool_events
                            .iter()
                            .any(|event| event.tool_name == RUNTIME_CATALOG_TOOL);
                        if !catalog_completed {
                            ToolCallResultPayload::error(
                                &request.tool_call_id,
                                "runtime_catalog_required",
                                "inspect_runtime_catalog must complete before planning an engine install",
                            )
                        } else if !action_plans.is_empty() {
                            ToolCallResultPayload::error(
                                &request.tool_call_id,
                                "action_already_planned",
                                "only one mutating action plan is allowed per Agent run",
                            )
                        } else {
                            match build_engine_install_plan(input) {
                                Ok(Some(pending)) => register_pending_action(
                                    input,
                                    pending,
                                    &request,
                                    PendingActionPresentation {
                                        tool_name: PLAN_ENGINE_INSTALL_TOOL,
                                        label: "生成 llama.cpp 安装计划",
                                        summary: "llama.cpp 安装计划已生成；尚未下载或安装，必须通过 Rust 原生确认。".to_owned(),
                                    },
                                    &mut tool_events,
                                    &mut action_plans,
                                )?,
                                Ok(None) => ToolCallResultPayload::error(
                                    &request.tool_call_id,
                                    "engine_already_installed",
                                    "HAL100 managed llama.cpp is already installed",
                                ),
                                Err(error) => ToolCallResultPayload::error(
                                    &request.tool_call_id,
                                    error.code(),
                                    "Rust could not create the engine install plan",
                                ),
                            }
                        }
                    }
                    Ok(AuthorizedAgentTool::PlanEngineRemove) => {
                        let catalog_completed = tool_events
                            .iter()
                            .any(|event| event.tool_name == RUNTIME_CATALOG_TOOL);
                        if !catalog_completed {
                            ToolCallResultPayload::error(
                                &request.tool_call_id,
                                "runtime_catalog_required",
                                "inspect_runtime_catalog must complete before planning an engine removal",
                            )
                        } else if !action_plans.is_empty() {
                            ToolCallResultPayload::error(
                                &request.tool_call_id,
                                "action_already_planned",
                                "only one mutating action plan is allowed per Agent run",
                            )
                        } else {
                            match build_engine_remove_plan(input) {
                                Ok(Some(pending)) => register_pending_action(
                                    input,
                                    pending,
                                    &request,
                                    PendingActionPresentation {
                                        tool_name: PLAN_ENGINE_REMOVE_TOOL,
                                        label: "生成 llama.cpp 卸载计划",
                                        summary: "llama.cpp 卸载计划已生成；尚未停止或删除引擎，必须通过 Rust 原生确认。".to_owned(),
                                    },
                                    &mut tool_events,
                                    &mut action_plans,
                                )?,
                                Ok(None) => ToolCallResultPayload::error(
                                    &request.tool_call_id,
                                    "engine_not_installed",
                                    "HAL100 managed llama.cpp is not installed",
                                ),
                                Err(error) => ToolCallResultPayload::error(
                                    &request.tool_call_id,
                                    error.code(),
                                    "Rust could not create the engine removal plan",
                                ),
                            }
                        }
                    }
                    Ok(AuthorizedAgentTool::InspectOpenCodeStatus) => {
                        match input.open_code.detect() {
                            Ok(detection) => {
                                tool_events.push(AgentToolEvent {
                                    tool_call_id: request.tool_call_id.clone(),
                                    tool_name: OPENCODE_STATUS_TOOL.to_owned(),
                                    label: "检查 OpenCode 接入状态".to_owned(),
                                    status: "completed".to_owned(),
                                    summary: "Rust 已检查 OpenCode 安装、全局配置和 HAL100 Provider 所有权。"
                                        .to_owned(),
                                });
                                ToolCallResultPayload::success(
                                    &request.tool_call_id,
                                    serde_json::to_value(detection)
                                        .map_err(|_| AgentServiceError::InvalidProtocol)?,
                                )
                            }
                            Err(error) => ToolCallResultPayload::error(
                                &request.tool_call_id,
                                AgentServiceError::from(error).code(),
                                "Rust could not inspect OpenCode safely",
                            ),
                        }
                    }
                    Ok(AuthorizedAgentTool::PlanOpenCodeConfiguration) => {
                        let status_completed = tool_events
                            .iter()
                            .any(|event| event.tool_name == OPENCODE_STATUS_TOOL);
                        if !status_completed {
                            ToolCallResultPayload::error(
                                &request.tool_call_id,
                                "opencode_status_required",
                                "inspect_opencode_status must complete before planning configuration",
                            )
                        } else if !action_plans.is_empty() {
                            ToolCallResultPayload::error(
                                &request.tool_call_id,
                                "action_already_planned",
                                "only one mutating action plan is allowed per Agent run",
                            )
                        } else {
                            match build_opencode_configuration_plan(input) {
                                Ok(pending) => register_pending_action(
                                    input,
                                    pending,
                                    &request,
                                    PendingActionPresentation {
                                        tool_name: PLAN_OPENCODE_CONFIGURATION_TOOL,
                                        label: "生成 OpenCode 配置计划",
                                        summary: "OpenCode 配置计划已生成；尚未写入配置，必须通过 Rust 原生确认。".to_owned(),
                                    },
                                    &mut tool_events,
                                    &mut action_plans,
                                )?,
                                Err(error) => ToolCallResultPayload::error(
                                    &request.tool_call_id,
                                    error.code(),
                                    "Rust refused to create an unsafe or conflicting OpenCode plan",
                                ),
                            }
                        }
                    }
                    Err(error) => ToolCallResultPayload::error(
                        &request.tool_call_id,
                        error.code,
                        error.message,
                    ),
                };
                if let Some(error) = result.error.as_ref() {
                    last_tool_failure_code = Some(error.code.clone());
                }
                write_envelope(
                    stdin,
                    &AgentRpcEnvelope {
                        protocol_version: AGENT_RPC_VERSION,
                        id: envelope.id,
                        kind: "tool.call.result".to_owned(),
                        payload: serde_json::to_value(result)
                            .map_err(|_| AgentServiceError::InvalidProtocol)?,
                    },
                )?;
            }
            "agent.run.completed" => {
                if envelope.id != input.run_id {
                    return Err(AgentServiceError::InvalidProtocol);
                }
                let completed: AgentRunCompletedPayload = serde_json::from_value(envelope.payload)
                    .map_err(|_| AgentServiceError::InvalidProtocol)?;
                let diagnostic_repair_available =
                    diagnostic_report.as_ref().is_some_and(|report| {
                        report
                            .findings
                            .iter()
                            .any(|finding| finding.repair_kind.is_some())
                    });
                let completion_validation = validate_completion(
                    &input.run_id,
                    input.requirements,
                    diagnostic_repair_available,
                    &completed,
                    &tool_events,
                    &action_plans,
                );
                if matches!(
                    completion_validation,
                    Err(AgentServiceError::RequiredToolMissing(_))
                ) && let Some(code) = last_tool_failure_code.as_deref()
                {
                    return Err(AgentServiceError::KernelRejected(format!(
                        "required_tool_failed:{code}"
                    )));
                }
                completion_validation?;
                request_shutdown(stdin, receiver, &input.run_id, &input.cancellation)?;
                return Ok(SidecarRunOutput {
                    answer: completed.answer,
                    tool_events,
                    action_plans,
                });
            }
            "system.error" => return Err(kernel_rejection(&envelope.payload)),
            _ => return Err(AgentServiceError::InvalidProtocol),
        }
    }
}

fn build_runtime_catalog(
    input: &SidecarRunInput,
) -> Result<AgentRuntimeCatalog, AgentServiceError> {
    input.database.refresh_local_model_states()?;
    let engine = input.engine.status()?;
    let routing = input.gateway.routing_snapshot();
    let mut models = input
        .database
        .local_models()?
        .into_iter()
        .map(|model| AgentRuntimeModel {
            active: engine.active_model_id.as_deref() == Some(model.id.as_str()),
            id: model.id,
            display_name: model.display_name,
            quantization: model.quantization,
            size_bytes: model.size_bytes,
            ready: model.state == LocalModelState::Ready,
        })
        .collect::<Vec<_>>();
    models.sort_by(|left, right| left.display_name.cmp(&right.display_name));
    let configured_backend_count = routing
        .backend_ids
        .iter()
        .filter(|backend_id| backend_id.as_str() != "hal100-agent-runtime")
        .count()
        .try_into()
        .unwrap_or(u32::MAX);
    Ok(AgentRuntimeCatalog {
        engine_install_state: engine.install_state,
        engine_runtime_state: engine.runtime_state,
        active_model_id: engine.active_model_id,
        active_model_name: engine.active_model_name,
        active_backend_id: routing.active_backend_id,
        configured_backend_count,
        models,
    })
}

fn register_pending_action(
    input: &SidecarRunInput,
    pending: PendingAgentAction,
    request: &ToolCallRequestPayload,
    presentation: PendingActionPresentation,
    tool_events: &mut Vec<AgentToolEvent>,
    action_plans: &mut Vec<AgentActionPlan>,
) -> Result<ToolCallResultPayload, AgentServiceError> {
    let plan = pending.plan.clone();
    {
        let mut slot = input
            .pending_action_plan
            .lock()
            .map_err(|_| AgentServiceError::ActionPlanUnavailable)?;
        if slot.is_some() {
            return Err(AgentServiceError::ActionPlanUnavailable);
        }
        *slot = Some(pending);
    }
    input.database.insert_audit_event(
        "agent_action_planned",
        "agent_action_plan",
        &plan.plan_id,
        &json!({
            "action": action_kind_key(plan.action_kind),
            "targetId": plan.target_id,
        })
        .to_string(),
        now_ms(),
    )?;
    tool_events.push(AgentToolEvent {
        tool_call_id: request.tool_call_id.clone(),
        tool_name: presentation.tool_name.to_owned(),
        label: presentation.label.to_owned(),
        status: "awaiting_confirmation".to_owned(),
        summary: presentation.summary,
    });
    action_plans.push(plan.clone());
    Ok(ToolCallResultPayload::success(
        &request.tool_call_id,
        serde_json::to_value(plan).map_err(|_| AgentServiceError::InvalidProtocol)?,
    ))
}

fn build_model_start_plan(
    input: &SidecarRunInput,
    model_id: &str,
) -> Result<Option<PendingAgentAction>, AgentServiceError> {
    input.database.refresh_local_model_states()?;
    let engine = input.engine.status()?;
    if engine.install_state != EngineInstallState::Installed {
        return Ok(None);
    }
    let Some(model) = input
        .database
        .local_model(model_id)?
        .filter(|model| model.state == LocalModelState::Ready)
    else {
        return Ok(None);
    };
    let plan_id = format!("agent-plan-{}", Uuid::new_v4().simple());
    let current_state = engine.active_model_name.as_ref().map_or_else(
        || "当前没有运行中的托管模型".to_owned(),
        |name| format!("当前模型：{name}"),
    );
    let target_id = model.id.clone();
    Ok(Some(PendingAgentAction {
        executor: AgentActionExecutor::StartOrSwitchModel {
            model_id: target_id.clone(),
        },
        plan: AgentActionPlan {
            plan_id,
            run_id: input.run_id.clone(),
            action_kind: AgentActionKind::StartOrSwitchModel,
            target_id,
            target_name: model.display_name,
            current_state: Some(current_state),
            details: vec![
                "启动前重新校验模型文件完整性".to_owned(),
                "等待现有请求安全排空，不执行强制切换".to_owned(),
            ],
            expires_at_ms: now_ms().saturating_add(ACTION_PLAN_TTL_MS),
            action_summary:
                "等待当前推理请求安全排空后，启动所选本地模型并将 hal100-active 切换到该模型"
                    .to_owned(),
            requires_native_confirmation: true,
        },
    }))
}

fn build_model_removal_plan(
    input: &SidecarRunInput,
    model_id: &str,
) -> Result<PendingAgentAction, AgentServiceError> {
    input.database.refresh_local_model_states()?;
    let engine = input.engine.status()?;
    let removal_plan = input
        .model_removal
        .plan_removal(model_id, engine.active_model_id.as_deref())?;
    let current_state = match removal_plan.removal_kind {
        ModelRemovalKind::MoveManagedFileToTrash => {
            "HAL100 托管模型；文件存在于受控模型目录".to_owned()
        }
        ModelRemovalKind::RemoveMissingManagedIndex => {
            "HAL100 托管模型；源文件已经不存在".to_owned()
        }
        ModelRemovalKind::RemoveExternalIndex => "外部模型索引；源文件不归 HAL100 所有".to_owned(),
    };
    let details = match removal_plan.removal_kind {
        ModelRemovalKind::MoveManagedFileToTrash => vec![
            "执行前再次校验模型所有权、路径边界和文件大小".to_owned(),
            "模型文件移到系统废纸篓，不做不可恢复删除".to_owned(),
        ],
        ModelRemovalKind::RemoveMissingManagedIndex => vec![
            "执行前确认文件仍然缺失".to_owned(),
            "只清理 HAL100 数据库中的失效索引".to_owned(),
        ],
        ModelRemovalKind::RemoveExternalIndex => vec![
            "外部模型源文件不会移动、修改或删除".to_owned(),
            "只移除 HAL100 数据库中的模型索引".to_owned(),
        ],
    };
    Ok(PendingAgentAction {
        executor: AgentActionExecutor::RemoveModel {
            removal_plan_id: removal_plan.plan_id.clone(),
            model_id: removal_plan.model_id.clone(),
        },
        plan: AgentActionPlan {
            plan_id: format!("agent-plan-{}", Uuid::new_v4().simple()),
            run_id: input.run_id.clone(),
            action_kind: AgentActionKind::RemoveModel,
            target_id: removal_plan.model_id,
            target_name: removal_plan.display_name,
            current_state: Some(current_state),
            details,
            expires_at_ms: removal_plan.expires_at_ms,
            action_summary: removal_plan.action_summary,
            requires_native_confirmation: true,
        },
    })
}

fn build_engine_install_plan(
    input: &SidecarRunInput,
) -> Result<Option<PendingAgentAction>, AgentServiceError> {
    let status = input.engine.status()?;
    if status.install_state == EngineInstallState::Installed {
        return Ok(None);
    }
    let engine_plan = input.engine.plan_install()?;
    Ok(Some(PendingAgentAction {
        executor: AgentActionExecutor::InstallLlamaCpp {
            engine_plan_id: engine_plan.plan_id.clone(),
        },
        plan: AgentActionPlan {
            plan_id: format!("agent-plan-{}", Uuid::new_v4().simple()),
            run_id: input.run_id.clone(),
            action_kind: AgentActionKind::InstallLlamaCpp,
            target_id: "llama.cpp".to_owned(),
            target_name: format!("llama.cpp {}", engine_plan.version),
            current_state: Some("当前尚未安装 HAL100 托管的 llama.cpp".to_owned()),
            details: vec![
                format!("发布方：{}", engine_plan.publisher),
                format!("下载大小：{} 字节", engine_plan.archive_size_bytes),
                "安装前校验固定 SHA-256 与 llama-server 二进制".to_owned(),
            ],
            expires_at_ms: engine_plan.expires_at_ms,
            action_summary: engine_plan.action_summary,
            requires_native_confirmation: true,
        },
    }))
}

fn build_engine_remove_plan(
    input: &SidecarRunInput,
) -> Result<Option<PendingAgentAction>, AgentServiceError> {
    let status = input.engine.status()?;
    if status.install_state == EngineInstallState::NotInstalled {
        return Ok(None);
    }
    let engine_plan = input.engine.plan_remove()?;
    Ok(Some(PendingAgentAction {
        executor: AgentActionExecutor::RemoveLlamaCpp {
            engine_plan_id: engine_plan.plan_id.clone(),
        },
        plan: AgentActionPlan {
            plan_id: format!("agent-plan-{}", Uuid::new_v4().simple()),
            run_id: input.run_id.clone(),
            action_kind: AgentActionKind::RemoveLlamaCpp,
            target_id: "llama.cpp".to_owned(),
            target_name: format!("llama.cpp {}", engine_plan.version),
            current_state: Some(format!("当前引擎状态：{:?}", status.runtime_state)),
            details: vec![
                "执行前停止 HAL100 托管的 llama-server".to_owned(),
                "只删除 HAL100 托管引擎目录，不删除任何模型".to_owned(),
            ],
            expires_at_ms: engine_plan.expires_at_ms,
            action_summary: engine_plan.action_summary,
            requires_native_confirmation: true,
        },
    }))
}

fn build_opencode_configuration_plan(
    input: &SidecarRunInput,
) -> Result<PendingAgentAction, AgentServiceError> {
    let configuration_plan = input.open_code.plan_configuration()?;
    Ok(PendingAgentAction {
        executor: AgentActionExecutor::ConfigureOpenCode {
            configuration_plan_id: configuration_plan.plan_id.clone(),
        },
        plan: AgentActionPlan {
            plan_id: format!("agent-plan-{}", Uuid::new_v4().simple()),
            run_id: input.run_id.clone(),
            action_kind: AgentActionKind::ConfigureOpenCode,
            target_id: "opencode".to_owned(),
            target_name: "OpenCode".to_owned(),
            current_state: Some("Rust 已检查现有全局配置与 HAL100 Provider 所有权".to_owned()),
            details: vec![
                format!("配置文件：{}", configuration_plan.config_path),
                "保留用户默认模型，不覆盖冲突 Provider".to_owned(),
                if configuration_plan.creates_backup {
                    "写入前创建原配置备份".to_owned()
                } else {
                    "当前无需创建旧配置备份".to_owned()
                },
            ],
            expires_at_ms: configuration_plan.expires_at_ms,
            action_summary: "向 OpenCode 写入由 HAL100 管理的 Gateway Provider 和独立凭据引用"
                .to_owned(),
            requires_native_confirmation: true,
        },
    })
}

fn build_diagnostic_repair_plan(
    input: &SidecarRunInput,
    report: &EnvironmentDiagnosticReport,
    finding_id: &str,
) -> Result<PendingAgentAction, AgentServiceError> {
    let finding = report
        .findings
        .iter()
        .find(|finding| finding.finding_id == finding_id)
        .ok_or(AgentServiceError::InvalidProtocol)?;
    let repair_kind = finding
        .repair_kind
        .ok_or(AgentServiceError::InvalidProtocol)?;
    let mut pending = match repair_kind {
        DiagnosticRepairKind::InstallLlamaCpp => {
            build_engine_install_plan(input)?.ok_or(AgentServiceError::ActionPlanUnavailable)?
        }
        DiagnosticRepairKind::ConfigureOpenCode => {
            let detection = input.open_code.detect()?;
            if !detection.installed
                || detection.integration_state != OpenCodeIntegrationState::NotConfigured
            {
                return Err(AgentServiceError::ActionPlanUnavailable);
            }
            build_opencode_configuration_plan(input)?
        }
        DiagnosticRepairKind::RemoveModelIndex => {
            let target_id = finding
                .target_id
                .as_deref()
                .ok_or(AgentServiceError::InvalidProtocol)?;
            input.database.refresh_local_model_states()?;
            let model = input
                .database
                .local_model(target_id)?
                .ok_or(AgentServiceError::ActionPlanUnavailable)?;
            if model.state != LocalModelState::Missing {
                return Err(AgentServiceError::ActionPlanUnavailable);
            }
            build_model_removal_plan(input, target_id)?
        }
    };
    pending.plan.current_state = Some(format!(
        "诊断 {}（{}）：{}",
        finding.finding_id, finding.code, finding.summary
    ));
    pending
        .plan
        .details
        .push("执行前由 Rust 重新校验当前状态；执行完成后返回一份新的环境诊断报告。".to_owned());
    Ok(pending)
}

fn validate_completion(
    run_id: &str,
    requirements: AgentRunRequirements,
    diagnostic_repair_available: bool,
    completed: &AgentRunCompletedPayload,
    tool_events: &[AgentToolEvent],
    action_plans: &[AgentActionPlan],
) -> Result<(), AgentServiceError> {
    let expected_action_kind = if requirements.model_start_plan {
        Some(AgentActionKind::StartOrSwitchModel)
    } else if requirements.model_removal_plan {
        Some(AgentActionKind::RemoveModel)
    } else if requirements.engine_install_plan {
        Some(AgentActionKind::InstallLlamaCpp)
    } else if requirements.engine_remove_plan {
        Some(AgentActionKind::RemoveLlamaCpp)
    } else if requirements.opencode_configuration_plan {
        Some(AgentActionKind::ConfigureOpenCode)
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
        || completed.registered_tool_count != 10
        || completed.completed_tool_calls as usize != completed.tool_names.len()
        || completed.completed_tool_calls as usize != tool_events.len()
        || completed.tool_names.iter().any(|name| {
            !matches!(
                name.as_str(),
                SYSTEM_SUMMARY_TOOL
                    | RUNTIME_CATALOG_TOOL
                    | PLAN_MODEL_START_TOOL
                    | PLAN_MODEL_REMOVAL_TOOL
                    | ENVIRONMENT_DIAGNOSTICS_TOOL
                    | PLAN_DIAGNOSTIC_REPAIR_TOOL
                    | PLAN_ENGINE_INSTALL_TOOL
                    | PLAN_ENGINE_REMOVE_TOOL
                    | OPENCODE_STATUS_TOOL
                    | PLAN_OPENCODE_CONFIGURATION_TOOL
            )
        })
        || completed.answer.trim().is_empty()
        || action_plans.len() > 1
        || expected_action_kind
            .is_some_and(|kind| action_plans.len() != 1 || action_plans[0].action_kind != kind)
        || (requirements.diagnostic_repair_plan
            && diagnostic_repair_available
            && (action_plans.len() != 1
                || !diagnostic_action_kind_is_allowed(action_plans[0].action_kind)))
        || (requirements.diagnostic_repair_plan
            && !diagnostic_repair_available
            && !action_plans.is_empty())
    {
        return Err(AgentServiceError::InvalidProtocol);
    }
    if requirements.system_summary
        && !tool_events
            .iter()
            .any(|event| event.tool_name == SYSTEM_SUMMARY_TOOL)
    {
        return Err(AgentServiceError::RequiredToolMissing(SYSTEM_SUMMARY_TOOL));
    }
    if requirements.runtime_catalog
        && !tool_events
            .iter()
            .any(|event| event.tool_name == RUNTIME_CATALOG_TOOL)
    {
        return Err(AgentServiceError::RequiredToolMissing(RUNTIME_CATALOG_TOOL));
    }
    if requirements.environment_diagnostics
        && !tool_events
            .iter()
            .any(|event| event.tool_name == ENVIRONMENT_DIAGNOSTICS_TOOL)
    {
        return Err(AgentServiceError::RequiredToolMissing(
            ENVIRONMENT_DIAGNOSTICS_TOOL,
        ));
    }
    if requirements.diagnostic_repair_plan
        && diagnostic_repair_available
        && !tool_events
            .iter()
            .any(|event| event.tool_name == PLAN_DIAGNOSTIC_REPAIR_TOOL)
    {
        return Err(AgentServiceError::RequiredToolMissing(
            PLAN_DIAGNOSTIC_REPAIR_TOOL,
        ));
    }
    if requirements.model_start_plan
        && (!tool_events
            .iter()
            .any(|event| event.tool_name == PLAN_MODEL_START_TOOL))
    {
        return Err(AgentServiceError::RequiredToolMissing(
            PLAN_MODEL_START_TOOL,
        ));
    }
    if requirements.model_removal_plan
        && !tool_events
            .iter()
            .any(|event| event.tool_name == PLAN_MODEL_REMOVAL_TOOL)
    {
        return Err(AgentServiceError::RequiredToolMissing(
            PLAN_MODEL_REMOVAL_TOOL,
        ));
    }
    for (required, tool_name) in [
        (requirements.engine_install_plan, PLAN_ENGINE_INSTALL_TOOL),
        (requirements.engine_remove_plan, PLAN_ENGINE_REMOVE_TOOL),
        (requirements.opencode_status, OPENCODE_STATUS_TOOL),
        (
            requirements.opencode_configuration_plan,
            PLAN_OPENCODE_CONFIGURATION_TOOL,
        ),
    ] {
        if required && !tool_events.iter().any(|event| event.tool_name == tool_name) {
            return Err(AgentServiceError::RequiredToolMissing(tool_name));
        }
    }
    if completed.answer.len() > MAX_ANSWER_BYTES {
        return Err(AgentServiceError::AnswerTooLarge);
    }
    Ok(())
}

fn request_shutdown(
    stdin: &mut ChildStdin,
    receiver: &mpsc::Receiver<Result<AgentRpcEnvelope, std::io::Error>>,
    run_id: &str,
    cancellation: &AtomicBool,
) -> Result<(), AgentServiceError> {
    let shutdown_id = format!("shutdown-{run_id}");
    write_envelope(
        stdin,
        &AgentRpcEnvelope {
            protocol_version: AGENT_RPC_VERSION,
            id: shutdown_id.clone(),
            kind: "system.shutdown".to_owned(),
            payload: json!({}),
        },
    )?;
    let acknowledgement = receive_envelope(receiver, cancellation)?;
    validate_envelope(&acknowledgement)?;
    if acknowledgement.id != shutdown_id || acknowledgement.kind != "system.shutdown.ack" {
        return Err(AgentServiceError::InvalidProtocol);
    }
    Ok(())
}

fn validate_envelope(envelope: &AgentRpcEnvelope) -> Result<(), AgentServiceError> {
    if envelope.protocol_version != AGENT_RPC_VERSION
        || envelope.id.is_empty()
        || envelope.id.len() > 128
    {
        return Err(AgentServiceError::InvalidProtocol);
    }
    Ok(())
}

fn receive_envelope(
    receiver: &mpsc::Receiver<Result<AgentRpcEnvelope, std::io::Error>>,
    cancellation: &AtomicBool,
) -> Result<AgentRpcEnvelope, AgentServiceError> {
    let started = Instant::now();
    loop {
        if cancellation.load(Ordering::Acquire) {
            return Err(AgentServiceError::Cancelled);
        }
        let remaining = SIDECAR_RESPONSE_TIMEOUT.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(AgentServiceError::KernelTimeout);
        }
        match receiver.recv_timeout(remaining.min(SIDECAR_CANCELLATION_POLL)) {
            Ok(result) => return result.map_err(AgentServiceError::Io),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(AgentServiceError::Io(std::io::Error::new(
                    ErrorKind::UnexpectedEof,
                    "HAL100 Agent Kernel closed its RPC stream",
                )));
            }
        }
    }
}

fn write_envelope(
    stdin: &mut ChildStdin,
    envelope: &AgentRpcEnvelope,
) -> Result<(), AgentServiceError> {
    let frame = encode_agent_rpc_frame(envelope)?;
    stdin.write_all(&frame).map_err(AgentServiceError::Io)?;
    stdin.flush().map_err(AgentServiceError::Io)
}

fn read_envelope(reader: &mut impl Read) -> Result<AgentRpcEnvelope, std::io::Error> {
    let mut prefix = [0_u8; 4];
    reader.read_exact(&mut prefix)?;
    let payload_length = u32::from_be_bytes(prefix) as usize;
    if payload_length > AGENT_RPC_MAX_FRAME_BYTES {
        return Err(std::io::Error::new(
            ErrorKind::InvalidData,
            "Agent RPC frame exceeds limit",
        ));
    }
    let mut payload = vec![0_u8; payload_length];
    reader.read_exact(&mut payload)?;
    serde_json::from_slice(&payload)
        .map_err(|error| std::io::Error::new(ErrorKind::InvalidData, error))
}

fn wait_for_child(child: &mut Child) -> Result<(), AgentServiceError> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(AgentServiceError::Io)? {
            return status
                .success()
                .then_some(())
                .ok_or(AgentServiceError::KernelStart);
        }
        if started.elapsed() >= SIDECAR_EXIT_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AgentServiceError::KernelTimeout);
        }
        thread::sleep(Duration::from_millis(20));
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

fn resolve_node_binary(workspace_root: &Path) -> Result<PathBuf, AgentServiceError> {
    let candidate = env::var_os("HAL100_AGENT_NODE_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root.join("node_modules/node/bin/node"));
    let candidate = candidate
        .canonicalize()
        .map_err(|_| AgentServiceError::KernelUnavailable)?;
    let output = Command::new(&candidate)
        .arg("--version")
        .env_clear()
        .stdin(Stdio::null())
        .output()
        .map_err(|_| AgentServiceError::KernelUnavailable)?;
    if !output.status.success()
        || String::from_utf8_lossy(&output.stdout).trim() != PINNED_NODE_VERSION
    {
        return Err(AgentServiceError::KernelRuntimeVersion);
    }
    Ok(candidate)
}

fn validate_prompt(prompt: &str) -> Result<String, AgentServiceError> {
    let prompt = prompt.trim();
    if prompt.is_empty()
        || prompt.len() > MAX_PROMPT_BYTES
        || prompt
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(AgentServiceError::InvalidPrompt);
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
        return Err(AgentServiceError::OutsideDomain);
    }
    Ok(prompt.to_owned())
}

fn prompt_requires_system_summary(prompt: &str) -> bool {
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

fn prompt_requires_runtime_catalog(prompt: &str) -> bool {
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

fn prompt_requires_environment_diagnostics(prompt: &str) -> bool {
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

fn prompt_requires_diagnostic_repair_plan(prompt: &str) -> bool {
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

fn prompt_requires_model_start_plan(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    let refers_to_model = ["模型", "qwen", "gguf", "llama"]
        .iter()
        .any(|marker| normalized.contains(marker));
    let requests_start_or_switch = ["启动", "切换", "换成", "改用", "设为当前"]
        .iter()
        .any(|marker| normalized.contains(marker));
    refers_to_model && requests_start_or_switch
}

fn prompt_requires_model_removal_plan(prompt: &str) -> bool {
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

fn prompt_requires_engine_install_plan(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    prompt_refers_to_llama_cpp_engine(prompt)
        && ["安装", "部署", "装上"]
            .iter()
            .any(|marker| normalized.contains(marker))
        && !["卸载", "移除", "删除"]
            .iter()
            .any(|marker| normalized.contains(marker))
}

fn prompt_requires_engine_remove_plan(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    prompt_refers_to_llama_cpp_engine(prompt)
        && ["卸载", "移除", "删除引擎"]
            .iter()
            .any(|marker| normalized.contains(marker))
}

fn prompt_requires_opencode_status(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    normalized.contains("opencode")
        && ["状态", "检测", "检查", "配置", "接入", "连接"]
            .iter()
            .any(|marker| normalized.contains(marker))
}

fn prompt_requires_opencode_configuration_plan(prompt: &str) -> bool {
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

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}

fn inactive_cloud_session_status() -> AgentCloudSessionStatus {
    AgentCloudSessionStatus {
        active: false,
        available: false,
        backend_id: None,
        backend_name: None,
        backend_kind: None,
        api_root: None,
        model: None,
        provider_protocol: None,
        activated_at_ms: None,
        last_error_code: None,
    }
}

fn set_owner_only_directory(path: &Path) -> Result<(), std::io::Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

struct SessionDirectory(PathBuf);

struct TransientAgentCredential {
    registry: CredentialRegistry,
    client_app_id: &'static str,
}

struct TemporaryAgentRoute {
    gateway: GatewayState,
    alias: String,
}

struct ActiveRunGuard {
    active_run: Arc<Mutex<Option<ActiveRun>>>,
    run_id: String,
}

impl Drop for ActiveRunGuard {
    fn drop(&mut self) {
        if let Ok(mut active_run) = self.active_run.lock()
            && active_run
                .as_ref()
                .is_some_and(|run| run.run_id == self.run_id)
        {
            active_run.take();
        }
    }
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

impl SessionDirectory {
    fn create(path: PathBuf) -> Result<Self, AgentServiceError> {
        fs::create_dir_all(&path).map_err(AgentServiceError::Io)?;
        set_owner_only_directory(&path).map_err(AgentServiceError::Io)?;
        Ok(Self(path))
    }
}

impl Drop for SessionDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            validate_prompt("给我写一首关于春天的诗"),
            Err(AgentServiceError::OutsideDomain)
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
        let service = AgentService::with_idle_timeout(
            runtime,
            engine,
            open_code,
            Arc::new(ModelRemovalManager::new(
                database.clone(),
                data_dir.join("models"),
            )),
            gateway,
            database,
            credentials,
            "http://127.0.0.1:10100/v1".to_owned(),
            data_dir.join("models"),
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
        let restarted_service = AgentService::with_idle_timeout(
            service.runtime.clone(),
            service.engine.clone(),
            service.open_code.clone(),
            service.model_removal.clone(),
            service.gateway.clone(),
            service.database.clone(),
            service.credentials.clone(),
            service.gateway_base_url.clone(),
            service.model_storage_path.clone(),
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
        let service = Arc::new(
            AgentService::with_idle_timeout(
                runtime,
                engine,
                open_code,
                Arc::new(ModelRemovalManager::new(
                    database.clone(),
                    data_dir.join("models"),
                )),
                gateway.clone(),
                database.clone(),
                credentials.clone(),
                format!("http://{gateway_address}/v1"),
                data_dir.join("models"),
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
    }

    #[test]
    fn classifies_only_explicit_engine_and_opencode_mutation_intents() {
        assert!(prompt_requires_engine_install_plan(
            "检查状态并生成安装 llama.cpp 的计划"
        ));
        assert!(prompt_requires_engine_remove_plan("卸载本地推理引擎"));
        assert!(!prompt_requires_engine_install_plan("帮我安装一个本地模型"));
        assert!(prompt_requires_opencode_status("检查 OpenCode 配置状态"));
        assert!(!prompt_requires_opencode_configuration_plan(
            "检查 OpenCode 配置状态"
        ));
        assert!(prompt_requires_opencode_configuration_plan(
            "检查 OpenCode 状态，并生成接入 HAL100 Gateway 的配置计划"
        ));
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
    }

    #[test]
    fn action_plan_requires_the_exact_latest_unexpired_id_and_is_consumed_once() {
        let mut pending = Some(action_plan_fixture(200));
        assert!(matches!(
            take_valid_action_plan(&mut pending, "forged-plan", 100),
            Err(AgentServiceError::ActionPlanUnavailable)
        ));
        assert!(pending.is_some());
        let plan =
            take_valid_action_plan(&mut pending, "agent-plan-1", 100).expect("valid action plan");
        assert_eq!(plan.plan.target_id, "managed-model-1");
        assert!(pending.is_none());
        assert!(matches!(
            take_valid_action_plan(&mut pending, "agent-plan-1", 100),
            Err(AgentServiceError::ActionPlanUnavailable)
        ));

        let mut expired = Some(action_plan_fixture(99));
        assert!(matches!(
            take_valid_action_plan(&mut expired, "agent-plan-1", 100),
            Err(AgentServiceError::ActionPlanExpired)
        ));
        assert!(expired.is_some());

        let mut confirmation_bypass = action_plan_fixture(200);
        confirmation_bypass.plan.requires_native_confirmation = false;
        let mut confirmation_bypass = Some(confirmation_bypass);
        assert!(matches!(
            take_valid_action_plan(&mut confirmation_bypass, "agent-plan-1", 100),
            Err(AgentServiceError::ActionPlanUnavailable)
        ));
        assert!(confirmation_bypass.is_some());

        let oversized_id = "x".repeat(MAX_ACTION_PLAN_ID_CHARS + 1);
        assert!(matches!(
            take_valid_action_plan(&mut confirmation_bypass, &oversized_id, 100),
            Err(AgentServiceError::ActionPlanUnavailable)
        ));
    }

    #[test]
    fn rpc_receive_observes_cancellation_without_waiting_for_the_model_timeout() {
        let (_sender, receiver) = mpsc::channel();
        let cancellation = AtomicBool::new(true);
        let started = Instant::now();
        assert!(matches!(
            receive_envelope(&receiver, &cancellation),
            Err(AgentServiceError::Cancelled)
        ));
        assert!(started.elapsed() < Duration::from_millis(50));
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
            registered_tool_count: 10,
            completed_tool_calls: 0,
            tool_names: Vec::new(),
        };
        assert!(matches!(
            validate_completion(
                "run-1",
                AgentRunRequirements {
                    system_summary: true,
                    runtime_catalog: false,
                    model_start_plan: false,
                    model_removal_plan: false,
                    environment_diagnostics: false,
                    diagnostic_repair_plan: false,
                    engine_install_plan: false,
                    engine_remove_plan: false,
                    opencode_status: false,
                    opencode_configuration_plan: false,
                },
                false,
                &completed,
                &[],
                &[]
            ),
            Err(AgentServiceError::RequiredToolMissing(SYSTEM_SUMMARY_TOOL))
        ));
    }

    #[test]
    fn accepts_one_allowlisted_repair_plan_after_the_required_diagnosis() {
        let completed = AgentRunCompletedPayload {
            run_id: "run-diagnostic".to_owned(),
            answer: "已生成一项修复计划，尚未执行。".to_owned(),
            registered_tool_count: 10,
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
                AgentRunRequirements {
                    system_summary: false,
                    runtime_catalog: false,
                    model_start_plan: false,
                    model_removal_plan: false,
                    environment_diagnostics: true,
                    diagnostic_repair_plan: true,
                    engine_install_plan: false,
                    engine_remove_plan: false,
                    opencode_status: false,
                    opencode_configuration_plan: false,
                },
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
            registered_tool_count: 10,
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
                AgentRunRequirements {
                    system_summary: false,
                    runtime_catalog: false,
                    model_start_plan: false,
                    model_removal_plan: false,
                    environment_diagnostics: true,
                    diagnostic_repair_plan: true,
                    engine_install_plan: false,
                    engine_remove_plan: false,
                    opencode_status: false,
                    opencode_configuration_plan: false,
                },
                false,
                &completed,
                &events,
                &[],
            )
            .is_ok()
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
        let service = Arc::new(
            AgentService::with_idle_timeout(
                runtime,
                engine.clone(),
                open_code,
                Arc::new(ModelRemovalManager::new(
                    database.clone(),
                    data_dir.join("models"),
                )),
                gateway,
                database,
                credentials,
                format!("http://{gateway_address}/v1"),
                data_dir.join("models"),
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
