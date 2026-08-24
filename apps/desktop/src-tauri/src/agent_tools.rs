use std::{
    future::Future,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use hal100_core::{
    AgentToolPolicy, AuthorizedAgentTool, BUILT_IN_AGENT_RUNTIME, ExternalAgentIntegrationId,
    ExternalAgentIntegrationRegistry,
};
use hal100_infra::{
    Database, DatabaseError, EngineManagerError, EnvironmentDiagnosticError,
    EnvironmentDiagnostics, GatewayState, HermesAgentIntegrationAdapter,
    HermesAgentIntegrationError, LlamaCppManager, ManagedExternalAgentDeploymentError,
    ManagedExternalAgentDeploymentManager, ModelDownloadError, ModelDownloadManager,
    ModelRemovalError, ModelRemovalManager, OpenClawIntegrationAdapter, OpenClawIntegrationError,
    OpenCodeIntegrationError, OpenCodeManager, PiCodingAgentIntegrationAdapter,
    PiCodingAgentIntegrationError, RemoteModelCatalog, RemoteModelCatalogError,
};
use hal100_platform::{HardwareProbeError, MacOsSystemProbe};
use hal100_protocol::{
    AGENT_RPC_MAX_TOOL_RESULT_BYTES, AgentActionKind, AgentActionPlan,
    AgentExternalIntegrationStatus, AgentOperationalEvent, AgentOperationalHealthObservation,
    AgentOperationalHealthSample, AgentOperationalHealthStatus, AgentOperationalHistory,
    AgentRuntimeCatalog, AgentRuntimeModel, AgentSystemSummary, AgentToolEvent,
    DiagnosticRepairKind, DiagnosticSeverity, ENVIRONMENT_DIAGNOSTICS_TOOL,
    EXTERNAL_AGENT_STATUS_TOOL, EngineInstallState, EnvironmentDiagnosticReport,
    ExternalAgentGatewayProtocol, ExternalAgentIntegrationState, LocalModelState,
    MODEL_CATALOG_SEARCH_TOOL, MODEL_REPOSITORY_INSPECTION_TOOL, ModelDownloadSnapshot,
    ModelRemovalKind, OPERATIONAL_HEALTH_OBSERVATION_TOOL, OPERATIONAL_HISTORY_TOOL,
    OpenCodeIntegrationState, PLAN_DIAGNOSTIC_REPAIR_TOOL, PLAN_ENGINE_INSTALL_TOOL,
    PLAN_ENGINE_REMOVE_TOOL, PLAN_EXTERNAL_AGENT_CONFIGURATION_TOOL,
    PLAN_EXTERNAL_AGENT_DISCONNECTION_TOOL, PLAN_EXTERNAL_AGENT_INSTALLATION_TOOL,
    PLAN_MANAGED_EXTERNAL_AGENT_REMOVAL_TOOL, PLAN_MODEL_DOWNLOAD_TOOL, PLAN_MODEL_REMOVAL_TOOL,
    PLAN_MODEL_START_TOOL, RUNTIME_CATALOG_TOOL, RemoteModelRepository, RemoteModelSearchResults,
    SYSTEM_SUMMARY_TOOL, ToolCallRequestPayload, ToolCallResultPayload,
};
use serde_json::json;
use thiserror::Error;
use uuid::Uuid;

use crate::agent_action::{
    AgentActionExecutor, AgentActionPlanError, AgentActionPlanStore, PendingAgentAction,
    action_kind_key,
};

const ACTION_PLAN_TTL_MS: i64 = 5 * 60 * 1_000;
const TOOL_CANCELLATION_POLL: Duration = Duration::from_millis(100);
const OPERATIONAL_OBSERVATION_INTERVAL: Duration = Duration::from_millis(100);
const OPERATIONAL_OBSERVATION_SAMPLES: usize = 3;
const INTERNAL_CLOUD_BACKEND_PREFIX: &str = "hal100-agent-cloud-";

#[derive(Debug, Error)]
pub(super) enum AgentToolExecutionError {
    #[error("Agent 工具协议数据无效")]
    InvalidProtocol,
    #[error("Agent 工具结果超过 RPC 预算")]
    ResultTooLarge,
    #[error("Agent 工具执行已取消")]
    Cancelled,
    #[error("Agent 操作计划状态无效：{0:?}")]
    ActionPlan(AgentActionPlanError),
    #[error(transparent)]
    Probe(#[from] HardwareProbeError),
    #[error(transparent)]
    Database(#[from] DatabaseError),
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
    Diagnostics(#[from] EnvironmentDiagnosticError),
    #[error(transparent)]
    RemoteCatalog(#[from] RemoteModelCatalogError),
    #[error(transparent)]
    ModelDownload(#[from] ModelDownloadError),
    #[error(transparent)]
    ManagedDeployment(#[from] ManagedExternalAgentDeploymentError),
}

impl AgentToolExecutionError {
    pub(super) const fn code(&self) -> &'static str {
        match self {
            Self::InvalidProtocol => "invalid_protocol",
            Self::ResultTooLarge => "tool_result_too_large",
            Self::Cancelled => "agent_cancelled",
            Self::ActionPlan(AgentActionPlanError::Unavailable) => "agent_action_plan_unavailable",
            Self::ActionPlan(AgentActionPlanError::Expired) => "agent_action_plan_expired",
            Self::Probe(_) => "hardware_probe_failed",
            Self::Database(_) => "database_failed",
            Self::Engine(_) => "managed_model_operation_failed",
            Self::OpenCode(_) => "opencode_configuration_failed",
            Self::PiCodingAgent(_) => "pi_coding_agent_integration_failed",
            Self::OpenClaw(_) => "openclaw_integration_failed",
            Self::HermesAgent(_) => "hermes_agent_integration_failed",
            Self::ModelRemoval(_) => "model_removal_failed",
            Self::Diagnostics(_) => "environment_diagnostics_failed",
            Self::RemoteCatalog(error) => error.code(),
            Self::ModelDownload(error) => error.code(),
            Self::ManagedDeployment(error) => match error {
                ManagedExternalAgentDeploymentError::RecipeUnavailable => {
                    "deployment_recipe_unavailable"
                }
                ManagedExternalAgentDeploymentError::AlreadyInstalled => {
                    "external_agent_already_installed"
                }
                ManagedExternalAgentDeploymentError::ManagedInstallationNotFound => {
                    "managed_external_agent_not_installed"
                }
                ManagedExternalAgentDeploymentError::PackageManagerUnavailable => "npm_unavailable",
                ManagedExternalAgentDeploymentError::PackageMetadataMismatch => {
                    "deployment_recipe_drift"
                }
                ManagedExternalAgentDeploymentError::DependencyClosureMismatch => {
                    "deployment_dependency_drift"
                }
                ManagedExternalAgentDeploymentError::VerificationFailed => {
                    "deployment_verification_failed"
                }
                ManagedExternalAgentDeploymentError::UnsafeInstallRoot => "unsafe_deployment_root",
                ManagedExternalAgentDeploymentError::ManagedInstallationChanged => {
                    "managed_external_agent_changed"
                }
                ManagedExternalAgentDeploymentError::TrashFailed => {
                    "managed_external_agent_trash_failed"
                }
                ManagedExternalAgentDeploymentError::RemovalRollbackFailed => {
                    "managed_external_agent_restore_failed"
                }
                ManagedExternalAgentDeploymentError::InvalidPlan => {
                    "managed_deployment_plan_unavailable"
                }
                ManagedExternalAgentDeploymentError::CommandFailed(_) => {
                    "managed_deployment_command_failed"
                }
                ManagedExternalAgentDeploymentError::Database(_) => "database_failed",
                ManagedExternalAgentDeploymentError::Io(_) => "managed_deployment_io_failed",
            },
        }
    }
}

impl From<AgentActionPlanError> for AgentToolExecutionError {
    fn from(error: AgentActionPlanError) -> Self {
        Self::ActionPlan(error)
    }
}

#[derive(Clone)]
pub(super) struct AgentToolExecutor {
    model_storage_path: PathBuf,
    database: Arc<Database>,
    engine: Arc<LlamaCppManager>,
    open_code: Arc<OpenCodeManager>,
    pi_coding_agent: Arc<PiCodingAgentIntegrationAdapter>,
    openclaw: Arc<OpenClawIntegrationAdapter>,
    hermes_agent: Arc<HermesAgentIntegrationAdapter>,
    model_removal: Arc<ModelRemovalManager>,
    diagnostics: Arc<EnvironmentDiagnostics>,
    remote_catalog: Arc<RemoteModelCatalog>,
    model_download: Arc<ModelDownloadManager>,
    managed_deployment: Arc<ManagedExternalAgentDeploymentManager>,
    gateway: GatewayState,
    action_plans: AgentActionPlanStore,
}

impl AgentToolExecutor {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        model_storage_path: PathBuf,
        database: Arc<Database>,
        engine: Arc<LlamaCppManager>,
        open_code: Arc<OpenCodeManager>,
        pi_coding_agent: Arc<PiCodingAgentIntegrationAdapter>,
        openclaw: Arc<OpenClawIntegrationAdapter>,
        hermes_agent: Arc<HermesAgentIntegrationAdapter>,
        model_removal: Arc<ModelRemovalManager>,
        diagnostics: Arc<EnvironmentDiagnostics>,
        remote_catalog: Arc<RemoteModelCatalog>,
        model_download: Arc<ModelDownloadManager>,
        managed_deployment: Arc<ManagedExternalAgentDeploymentManager>,
        gateway: GatewayState,
        action_plans: AgentActionPlanStore,
    ) -> Self {
        Self {
            model_storage_path,
            database,
            engine,
            open_code,
            pi_coding_agent,
            openclaw,
            hermes_agent,
            model_removal,
            diagnostics,
            remote_catalog,
            model_download,
            managed_deployment,
            gateway,
            action_plans,
        }
    }

    pub(super) fn start_run(
        &self,
        run_id: String,
        external_agent_target: Option<ExternalAgentIntegrationId>,
        runtime_handle: tokio::runtime::Handle,
        cancellation: Arc<AtomicBool>,
    ) -> AgentToolRun {
        AgentToolRun {
            executor: self.clone(),
            run_id,
            external_agent_target,
            tool_events: Vec::new(),
            action_plans: Vec::new(),
            diagnostic_report: None,
            external_agent_inspection: None,
            model_search: None,
            model_repository: None,
            runtime_handle,
            cancellation,
        }
    }

    pub(super) fn start_model_download(
        &self,
        download_plan_id: &str,
    ) -> Result<ModelDownloadSnapshot, AgentToolExecutionError> {
        let available = MacOsSystemProbe.model_storage_available_bytes(&self.model_storage_path)?;
        Ok(self
            .model_download
            .start_download(download_plan_id, available)?)
    }

    pub(super) fn discard_model_download_plan(
        &self,
        download_plan_id: &str,
    ) -> Result<bool, AgentToolExecutionError> {
        Ok(self.model_download.discard_plan(download_plan_id)?)
    }
}

pub(super) struct AgentToolRun {
    executor: AgentToolExecutor,
    run_id: String,
    external_agent_target: Option<ExternalAgentIntegrationId>,
    tool_events: Vec<AgentToolEvent>,
    action_plans: Vec<AgentActionPlan>,
    diagnostic_report: Option<EnvironmentDiagnosticReport>,
    external_agent_inspection: Option<ExternalAgentIntegrationId>,
    model_search: Option<RemoteModelSearchResults>,
    model_repository: Option<RemoteModelRepository>,
    runtime_handle: tokio::runtime::Handle,
    cancellation: Arc<AtomicBool>,
}

pub(super) struct AgentToolRunOutput {
    pub(super) tool_events: Vec<AgentToolEvent>,
    pub(super) action_plans: Vec<AgentActionPlan>,
}

struct PendingActionPresentation {
    tool_name: &'static str,
    label: &'static str,
    summary: String,
}

impl AgentToolRun {
    pub(super) fn handle(
        &mut self,
        request: &ToolCallRequestPayload,
    ) -> Result<ToolCallResultPayload, AgentToolExecutionError> {
        let result = match AgentToolPolicy.authorize(request) {
            Ok(AuthorizedAgentTool::InspectSystemSummary) => self.inspect_system(request)?,
            Ok(AuthorizedAgentTool::InspectRuntimeCatalog) => self.inspect_runtime(request)?,
            Ok(AuthorizedAgentTool::InspectEnvironmentDiagnostics) => {
                self.inspect_environment(request)?
            }
            Ok(AuthorizedAgentTool::InspectOperationalHistory) => {
                self.inspect_operational_history(request)?
            }
            Ok(AuthorizedAgentTool::ObserveOperationalHealth) => {
                self.observe_operational_health(request)?
            }
            Ok(AuthorizedAgentTool::PlanModelStart { model_id }) => {
                self.plan_model_start(request, &model_id)?
            }
            Ok(AuthorizedAgentTool::PlanModelRemoval { model_id }) => {
                self.plan_model_removal(request, &model_id)?
            }
            Ok(AuthorizedAgentTool::PlanDiagnosticRepair {
                report_id,
                finding_id,
            }) => self.plan_diagnostic_repair(request, &report_id, &finding_id)?,
            Ok(AuthorizedAgentTool::PlanEngineInstall) => self.plan_engine_install(request)?,
            Ok(AuthorizedAgentTool::PlanEngineRemove) => self.plan_engine_remove(request)?,
            Ok(AuthorizedAgentTool::InspectExternalAgent { integration_id }) => {
                self.inspect_external_agent(request, integration_id)?
            }
            Ok(AuthorizedAgentTool::PlanExternalAgentConfiguration { integration_id }) => {
                self.plan_external_agent_configuration(request, integration_id)?
            }
            Ok(AuthorizedAgentTool::PlanExternalAgentDisconnection { integration_id }) => {
                self.plan_external_agent_disconnection(request, integration_id)?
            }
            Ok(AuthorizedAgentTool::PlanExternalAgentInstallation { integration_id }) => {
                self.plan_external_agent_installation(request, integration_id)?
            }
            Ok(AuthorizedAgentTool::PlanManagedExternalAgentRemoval { integration_id }) => {
                self.plan_managed_external_agent_removal(request, integration_id)?
            }
            Ok(AuthorizedAgentTool::SearchModelCatalog { query }) => {
                self.search_model_catalog(request, &query)?
            }
            Ok(AuthorizedAgentTool::InspectModelRepository { repository }) => {
                self.inspect_model_repository(request, &repository)?
            }
            Ok(AuthorizedAgentTool::PlanModelDownload { remote_path }) => {
                self.plan_model_download(request, &remote_path)?
            }
            Err(error) => {
                ToolCallResultPayload::error(&request.tool_call_id, error.code, error.message)
            }
        };
        Ok(result)
    }

    pub(super) fn tool_events(&self) -> &[AgentToolEvent] {
        &self.tool_events
    }

    pub(super) fn action_plans(&self) -> &[AgentActionPlan] {
        &self.action_plans
    }

    pub(super) fn diagnostic_repair_available(&self) -> bool {
        self.diagnostic_report.as_ref().is_some_and(|report| {
            report
                .findings
                .iter()
                .any(|finding| finding.repair_kind.is_some())
        })
    }

    pub(super) fn finish(self) -> AgentToolRunOutput {
        AgentToolRunOutput {
            tool_events: self.tool_events,
            action_plans: self.action_plans,
        }
    }

    fn inspect_system(
        &mut self,
        request: &ToolCallRequestPayload,
    ) -> Result<ToolCallResultPayload, AgentToolExecutionError> {
        let profile = MacOsSystemProbe.hardware_profile(&self.executor.model_storage_path)?;
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
        self.record_completed(
            request,
            SYSTEM_SUMMARY_TOOL,
            "检测这台 Mac",
            "Rust 已按需读取芯片、统一内存、CPU 与模型目录可用空间。".to_owned(),
        );
        success(request, summary)
    }

    fn inspect_runtime(
        &mut self,
        request: &ToolCallRequestPayload,
    ) -> Result<ToolCallResultPayload, AgentToolExecutionError> {
        let catalog = build_runtime_catalog(&self.executor)?;
        self.record_completed(
            request,
            RUNTIME_CATALOG_TOOL,
            "读取 HAL100 运行环境",
            format!(
                "Rust 已读取引擎、活动路由和 {} 个本地模型的脱敏状态。",
                catalog.models.len()
            ),
        );
        success(request, catalog)
    }

    fn inspect_environment(
        &mut self,
        request: &ToolCallRequestPayload,
    ) -> Result<ToolCallResultPayload, AgentToolExecutionError> {
        match self.executor.diagnostics.run() {
            Ok(report) => {
                self.record_completed(
                    request,
                    ENVIRONMENT_DIAGNOSTICS_TOOL,
                    "诊断 HAL100 运行环境",
                    format!(
                        "Rust 已完成一次按需诊断：{} 个错误、{} 个警告；未读取原始日志或执行完整模型哈希。",
                        report.error_count, report.warning_count
                    ),
                );
                self.diagnostic_report = Some(report.clone());
                success(request, report)
            }
            Err(error) => {
                let error = AgentToolExecutionError::from(error);
                Ok(ToolCallResultPayload::error(
                    &request.tool_call_id,
                    error.code(),
                    "Rust could not complete the bounded environment diagnosis",
                ))
            }
        }
    }

    fn inspect_operational_history(
        &mut self,
        request: &ToolCallRequestPayload,
    ) -> Result<ToolCallResultPayload, AgentToolExecutionError> {
        const MAX_OPERATIONAL_EVENTS: u32 = 24;
        let audit = self.executor.database.audit_log(MAX_OPERATIONAL_EVENTS)?;
        let events = audit
            .events
            .into_iter()
            .map(|event| AgentOperationalEvent {
                event_type: safe_operational_identifier(&event.event_type),
                target_type: safe_operational_identifier(&event.target_type),
                occurred_at_ms: event.created_at_ms,
                error_code: safe_operational_detail(&event.details, "errorCode"),
                action: safe_operational_detail(&event.details, "action"),
                reason: safe_operational_detail(&event.details, "reason"),
            })
            .collect::<Vec<_>>();
        let history = AgentOperationalHistory {
            generated_at_ms: now_ms(),
            total_event_count: audit.total_count,
            returned_event_count: saturating_u32(events.len()),
            events,
        };
        self.record_completed(
            request,
            OPERATIONAL_HISTORY_TOOL,
            "读取近期运维事件",
            format!(
                "Rust 已返回最近 {} 条脱敏事件；不包含提示词、回答、凭据、本地路径或目标ID。",
                history.returned_event_count
            ),
        );
        success(request, history)
    }

    fn observe_operational_health(
        &mut self,
        request: &ToolCallRequestPayload,
    ) -> Result<ToolCallResultPayload, AgentToolExecutionError> {
        let report = match self.executor.diagnostics.run() {
            Ok(report) => report,
            Err(error) => {
                let error = AgentToolExecutionError::from(error);
                return Ok(ToolCallResultPayload::error(
                    &request.tool_call_id,
                    error.code(),
                    "Rust could not complete the bounded deployment readiness observation",
                ));
            }
        };
        let mut samples = Vec::with_capacity(OPERATIONAL_OBSERVATION_SAMPLES);
        for index in 0..OPERATIONAL_OBSERVATION_SAMPLES {
            if self.cancellation.load(Ordering::Acquire) {
                return Err(AgentToolExecutionError::Cancelled);
            }
            let engine = self.executor.engine.status()?;
            let routing = self.executor.gateway.routing_snapshot();
            let user_backends = routing
                .backend_ids
                .iter()
                .filter(|backend_id| is_user_backend(backend_id))
                .count();
            let open_circuits = routing
                .backend_health
                .iter()
                .filter(|health| is_user_backend(&health.backend_id) && health.circuit_open)
                .count();
            samples.push(AgentOperationalHealthSample {
                observed_at_ms: now_ms(),
                engine_runtime_state: engine.runtime_state,
                active_route: routing
                    .active_backend_id
                    .as_deref()
                    .is_some_and(is_user_backend),
                registered_backend_count: saturating_u32(user_backends),
                open_circuit_count: saturating_u32(open_circuits),
            });
            if index + 1 < OPERATIONAL_OBSERVATION_SAMPLES {
                self.await_remote(async {
                    tokio::time::sleep(OPERATIONAL_OBSERVATION_INTERVAL).await;
                })?;
            }
        }
        let stable = samples
            .windows(2)
            .all(|pair| sample_state(&pair[0]) == sample_state(&pair[1]));
        let mut blocking_codes = report
            .findings
            .iter()
            .filter(|finding| finding.severity != DiagnosticSeverity::Info)
            .map(|finding| finding.code.clone())
            .collect::<Vec<_>>();
        blocking_codes.sort();
        blocking_codes.dedup();
        blocking_codes.truncate(16);
        let window_ms = samples
            .last()
            .zip(samples.first())
            .map(|(last, first)| last.observed_at_ms.saturating_sub(first.observed_at_ms))
            .unwrap_or_default();
        let observation = AgentOperationalHealthObservation {
            generated_at_ms: now_ms(),
            window_ms: window_ms.clamp(0, i64::from(u32::MAX)) as u32,
            sample_count: saturating_u32(samples.len()),
            stable,
            status: if report.error_count > 0 {
                AgentOperationalHealthStatus::Blocked
            } else if report.warning_count > 0 {
                AgentOperationalHealthStatus::NeedsAttention
            } else {
                AgentOperationalHealthStatus::Ready
            },
            engine_install_state: report.engine_install_state,
            ready_model_count: report.ready_model_count,
            configured_backend_count: report.configured_backend_count,
            installed_external_agent_count: report.installed_external_agent_count,
            configured_external_agent_count: report.configured_external_agent_count,
            attention_external_agent_count: report.attention_external_agent_count,
            blocking_codes,
            samples,
        };
        self.record_completed(
            request,
            OPERATIONAL_HEALTH_OBSERVATION_TOOL,
            "检查部署就绪与运行稳定性",
            format!(
                "Rust 已在 {} 毫秒固定窗口内完成 {} 次脱敏采样；未读取原始日志、路径或凭据。",
                observation.window_ms, observation.sample_count
            ),
        );
        success(request, observation)
    }

    fn plan_model_start(
        &mut self,
        request: &ToolCallRequestPayload,
        model_id: &str,
    ) -> Result<ToolCallResultPayload, AgentToolExecutionError> {
        if !self.completed(RUNTIME_CATALOG_TOOL) {
            return Ok(tool_error(
                request,
                "runtime_catalog_required",
                "inspect_runtime_catalog must complete before planning a model start",
            ));
        }
        if self.has_action_plan() {
            return Ok(action_already_planned(request));
        }
        let Some(pending) = build_model_start_plan(&self.executor, &self.run_id, model_id)? else {
            return Ok(tool_error(
                request,
                "model_start_unavailable",
                "the requested model is not ready or llama.cpp is not installed",
            ));
        };
        let target_name = pending.plan.target_name.clone();
        self.register_pending_action(
            pending,
            request,
            PendingActionPresentation {
                tool_name: PLAN_MODEL_START_TOOL,
                label: "生成模型启动或切换计划",
                summary: format!(
                    "已为“{target_name}”生成一次性计划；尚未执行，必须通过 Rust 原生确认。"
                ),
            },
        )
    }

    fn plan_model_removal(
        &mut self,
        request: &ToolCallRequestPayload,
        model_id: &str,
    ) -> Result<ToolCallResultPayload, AgentToolExecutionError> {
        if !self.completed(RUNTIME_CATALOG_TOOL) {
            return Ok(tool_error(
                request,
                "runtime_catalog_required",
                "inspect_runtime_catalog must complete before planning model removal",
            ));
        }
        if self.has_action_plan() {
            return Ok(action_already_planned(request));
        }
        match build_model_removal_plan(&self.executor, &self.run_id, model_id) {
            Ok(pending) => {
                let target_name = pending.plan.target_name.clone();
                self.register_pending_action(
                    pending,
                    request,
                    PendingActionPresentation {
                        tool_name: PLAN_MODEL_REMOVAL_TOOL,
                        label: "生成模型移除计划",
                        summary: format!(
                            "已为“{target_name}”生成一次性移除计划；尚未移动文件或删除索引，必须通过 Rust 原生确认。"
                        ),
                    },
                )
            }
            Err(error) => Ok(tool_error(
                request,
                error.code(),
                "Rust refused to create an unsafe model removal plan",
            )),
        }
    }

    fn plan_diagnostic_repair(
        &mut self,
        request: &ToolCallRequestPayload,
        report_id: &str,
        finding_id: &str,
    ) -> Result<ToolCallResultPayload, AgentToolExecutionError> {
        if !self.completed(ENVIRONMENT_DIAGNOSTICS_TOOL) {
            return Ok(tool_error(
                request,
                "environment_diagnostics_required",
                "inspect_environment_diagnostics must complete before planning a repair",
            ));
        }
        if self.has_action_plan() {
            return Ok(action_already_planned(request));
        }
        let Some(report) = self
            .diagnostic_report
            .as_ref()
            .filter(|report| report.report_id == report_id)
        else {
            return Ok(tool_error(
                request,
                "diagnostic_report_mismatch",
                "reportId must match the diagnosis completed in this Agent run",
            ));
        };
        match build_diagnostic_repair_plan(&self.executor, &self.run_id, report, finding_id) {
            Ok(pending) => {
                let target_name = pending.plan.target_name.clone();
                self.register_pending_action(
                    pending,
                    request,
                    PendingActionPresentation {
                        tool_name: PLAN_DIAGNOSTIC_REPAIR_TOOL,
                        label: "生成单项诊断修复计划",
                        summary: format!(
                            "已为“{target_name}”生成一次性修复计划；尚未执行，必须通过 Rust 原生确认，执行后会重新诊断。"
                        ),
                    },
                )
            }
            Err(error) => Ok(tool_error(
                request,
                error.code(),
                "Rust refused to create a stale or unsafe diagnostic repair plan",
            )),
        }
    }

    fn plan_engine_install(
        &mut self,
        request: &ToolCallRequestPayload,
    ) -> Result<ToolCallResultPayload, AgentToolExecutionError> {
        if !self.completed(RUNTIME_CATALOG_TOOL) {
            return Ok(tool_error(
                request,
                "runtime_catalog_required",
                "inspect_runtime_catalog must complete before planning an engine install",
            ));
        }
        if self.has_action_plan() {
            return Ok(action_already_planned(request));
        }
        match build_engine_install_plan(&self.executor, &self.run_id) {
            Ok(Some(pending)) => self.register_pending_action(
                pending,
                request,
                PendingActionPresentation {
                    tool_name: PLAN_ENGINE_INSTALL_TOOL,
                    label: "生成 llama.cpp 安装计划",
                    summary: "llama.cpp 安装计划已生成；尚未下载或安装，必须通过 Rust 原生确认。"
                        .to_owned(),
                },
            ),
            Ok(None) => Ok(tool_error(
                request,
                "engine_already_installed",
                "HAL100 managed llama.cpp is already installed",
            )),
            Err(error) => Ok(tool_error(
                request,
                error.code(),
                "Rust could not create the engine install plan",
            )),
        }
    }

    fn plan_engine_remove(
        &mut self,
        request: &ToolCallRequestPayload,
    ) -> Result<ToolCallResultPayload, AgentToolExecutionError> {
        if !self.completed(RUNTIME_CATALOG_TOOL) {
            return Ok(tool_error(
                request,
                "runtime_catalog_required",
                "inspect_runtime_catalog must complete before planning an engine removal",
            ));
        }
        if self.has_action_plan() {
            return Ok(action_already_planned(request));
        }
        match build_engine_remove_plan(&self.executor, &self.run_id) {
            Ok(Some(pending)) => self.register_pending_action(
                pending,
                request,
                PendingActionPresentation {
                    tool_name: PLAN_ENGINE_REMOVE_TOOL,
                    label: "生成 llama.cpp 卸载计划",
                    summary:
                        "llama.cpp 卸载计划已生成；尚未停止或删除引擎，必须通过 Rust 原生确认。"
                            .to_owned(),
                },
            ),
            Ok(None) => Ok(tool_error(
                request,
                "engine_not_installed",
                "HAL100 managed llama.cpp is not installed",
            )),
            Err(error) => Ok(tool_error(
                request,
                error.code(),
                "Rust could not create the engine removal plan",
            )),
        }
    }

    fn inspect_external_agent(
        &mut self,
        request: &ToolCallRequestPayload,
        integration_id: ExternalAgentIntegrationId,
    ) -> Result<ToolCallResultPayload, AgentToolExecutionError> {
        if self.external_agent_target != Some(integration_id) {
            return Ok(tool_error(
                request,
                "external_agent_target_mismatch",
                "integrationId must match the external Agent named in the validated user prompt",
            ));
        }
        let descriptor = ExternalAgentIntegrationRegistry::descriptor(integration_id);
        match build_external_agent_status(&self.executor, integration_id) {
            Ok(status) => {
                self.external_agent_inspection = Some(integration_id);
                self.record_completed(
                    request,
                    EXTERNAL_AGENT_STATUS_TOOL,
                    "检查外部 Agent 接入状态",
                    format!(
                        "Rust 已检查 {} 的安装、受管配置和所有权；路径与配置内容未发送给 Agent。",
                        descriptor.display_name
                    ),
                );
                success(request, status)
            }
            Err(error) => Ok(tool_error(
                request,
                error.code(),
                "Rust could not inspect the requested external Agent safely",
            )),
        }
    }

    fn plan_external_agent_configuration(
        &mut self,
        request: &ToolCallRequestPayload,
        integration_id: ExternalAgentIntegrationId,
    ) -> Result<ToolCallResultPayload, AgentToolExecutionError> {
        if self.external_agent_inspection != Some(integration_id) {
            return Ok(tool_error(
                request,
                "external_agent_status_required",
                "inspect_external_agent must complete for the same integrationId before planning configuration",
            ));
        }
        if self.has_action_plan() {
            return Ok(action_already_planned(request));
        }
        let descriptor = ExternalAgentIntegrationRegistry::descriptor(integration_id);
        match build_external_agent_configuration_plan(&self.executor, &self.run_id, integration_id)
        {
            Ok(pending) => {
                let display_name = descriptor.display_name;
                self.register_pending_action(
                    pending,
                    request,
                    PendingActionPresentation {
                        tool_name: PLAN_EXTERNAL_AGENT_CONFIGURATION_TOOL,
                        label: "生成外部 Agent 配置计划",
                        summary: format!(
                            "{display_name} 配置事务计划已生成；尚未写入配置，必须通过 Rust 原生确认。"
                        ),
                    },
                )
            }
            Err(error) => Ok(tool_error(
                request,
                error.code(),
                "Rust refused to create an unsafe or conflicting external Agent configuration plan",
            )),
        }
    }

    fn plan_external_agent_installation(
        &mut self,
        request: &ToolCallRequestPayload,
        integration_id: ExternalAgentIntegrationId,
    ) -> Result<ToolCallResultPayload, AgentToolExecutionError> {
        if self.external_agent_inspection != Some(integration_id) {
            return Ok(tool_error(
                request,
                "external_agent_status_required",
                "inspect_external_agent must complete for the same integrationId before planning installation",
            ));
        }
        if self.has_action_plan() {
            return Ok(action_already_planned(request));
        }
        let status = build_external_agent_status(&self.executor, integration_id)?;
        if status.installed {
            return Ok(tool_error(
                request,
                "external_agent_already_installed",
                "HAL100 will not replace an existing user or managed external Agent installation",
            ));
        }
        let descriptor = ExternalAgentIntegrationRegistry::descriptor(integration_id);
        match self
            .executor
            .managed_deployment
            .plan_install(integration_id)
        {
            Ok(deployment) => {
                let pending = PendingAgentAction {
                    executor: AgentActionExecutor::InstallExternalAgent {
                        integration_id,
                        deployment_plan_id: deployment.plan_id,
                    },
                    plan: AgentActionPlan {
                        plan_id: next_plan_id(),
                        run_id: self.run_id.clone(),
                        action_kind: AgentActionKind::InstallExternalAgent,
                        target_id: descriptor.integration_id.to_owned(),
                        target_name: descriptor.display_name.to_owned(),
                        current_state: Some(
                            "Rust 已确认未检测到现有安装，并核对固定官方包元数据".to_owned(),
                        ),
                        details: vec![
                            format!(
                                "固定包：{}@{}",
                                deployment.package_name, deployment.package_version
                            ),
                            format!("安装范围：{}", deployment.install_scope),
                        ]
                        .into_iter()
                        .chain(deployment.lifecycle_notes.into_iter().map(str::to_owned))
                        .collect(),
                        expires_at_ms: deployment.expires_at_ms,
                        action_summary: format!(
                            "将 {} 安装为 HAL100 私有、固定版本的外部 Agent，不改动用户现有环境",
                            descriptor.display_name
                        ),
                        requires_native_confirmation: true,
                    },
                };
                self.register_pending_action(
                    pending,
                    request,
                    PendingActionPresentation {
                        tool_name: PLAN_EXTERNAL_AGENT_INSTALLATION_TOOL,
                        label: "生成外部 Agent 私有安装计划",
                        summary: format!(
                            "{} 私有安装计划已生成；尚未安装，必须通过 Rust 原生确认。",
                            descriptor.display_name
                        ),
                    },
                )
            }
            Err(error) => {
                let error = AgentToolExecutionError::from(error);
                Ok(tool_error(
                    request,
                    error.code(),
                    "Rust refused to create an unavailable, conflicting, or unverified deployment plan",
                ))
            }
        }
    }

    fn plan_managed_external_agent_removal(
        &mut self,
        request: &ToolCallRequestPayload,
        integration_id: ExternalAgentIntegrationId,
    ) -> Result<ToolCallResultPayload, AgentToolExecutionError> {
        if self.external_agent_inspection != Some(integration_id) {
            return Ok(tool_error(
                request,
                "external_agent_status_required",
                "inspect_external_agent must complete for the same integrationId before planning managed removal",
            ));
        }
        if self.has_action_plan() {
            return Ok(action_already_planned(request));
        }
        let status = build_external_agent_status(&self.executor, integration_id)?;
        if !status.managed_installation {
            return Ok(tool_error(
                request,
                "managed_external_agent_not_installed",
                "HAL100 will not remove a user-installed external Agent",
            ));
        }
        let descriptor = ExternalAgentIntegrationRegistry::descriptor(integration_id);
        match self
            .executor
            .managed_deployment
            .plan_removal(integration_id)
        {
            Ok(removal) => {
                let pending = PendingAgentAction {
                    executor: AgentActionExecutor::RemoveExternalAgent {
                        integration_id,
                        deployment_plan_id: removal.plan_id,
                    },
                    plan: AgentActionPlan {
                        plan_id: next_plan_id(),
                        run_id: self.run_id.clone(),
                        action_kind: AgentActionKind::RemoveExternalAgent,
                        target_id: descriptor.integration_id.to_owned(),
                        target_name: descriptor.display_name.to_owned(),
                        current_state: Some(format!(
                            "Rust 已确认存在 HAL100 私有 {} {}",
                            descriptor.display_name, removal.package_version
                        )),
                        details: std::iter::once(format!("移除范围：{}", removal.removal_scope))
                            .chain(removal.lifecycle_notes.into_iter().map(str::to_owned))
                            .collect(),
                        expires_at_ms: removal.expires_at_ms,
                        action_summary: format!(
                            "仅将 HAL100 私有 {} 运行时移入系统废纸篓；用户安装、配置和会话保持不变",
                            descriptor.display_name
                        ),
                        requires_native_confirmation: true,
                    },
                };
                self.register_pending_action(
                    pending,
                    request,
                    PendingActionPresentation {
                        tool_name: PLAN_MANAGED_EXTERNAL_AGENT_REMOVAL_TOOL,
                        label: "生成外部 Agent 私有卸载计划",
                        summary: format!(
                            "{} 私有卸载计划已生成；尚未移动任何文件，必须通过 Rust 原生确认。",
                            descriptor.display_name
                        ),
                    },
                )
            }
            Err(error) => {
                let error = AgentToolExecutionError::from(error);
                Ok(tool_error(
                    request,
                    error.code(),
                    "Rust refused to remove a missing, changed, user-owned, or unsafe external Agent runtime",
                ))
            }
        }
    }

    fn plan_external_agent_disconnection(
        &mut self,
        request: &ToolCallRequestPayload,
        integration_id: ExternalAgentIntegrationId,
    ) -> Result<ToolCallResultPayload, AgentToolExecutionError> {
        if self.external_agent_inspection != Some(integration_id) {
            return Ok(tool_error(
                request,
                "external_agent_status_required",
                "inspect_external_agent must complete for the same integrationId before planning disconnection",
            ));
        }
        if self.has_action_plan() {
            return Ok(action_already_planned(request));
        }
        let descriptor = ExternalAgentIntegrationRegistry::descriptor(integration_id);
        match build_external_agent_disconnection_plan(&self.executor, &self.run_id, integration_id)
        {
            Ok(pending) => {
                let display_name = descriptor.display_name;
                self.register_pending_action(
                    pending,
                    request,
                    PendingActionPresentation {
                        tool_name: PLAN_EXTERNAL_AGENT_DISCONNECTION_TOOL,
                        label: "生成外部 Agent 断开计划",
                        summary: format!(
                            "{display_name} 断开事务计划已生成；尚未修改配置或撤销凭据，必须通过 Rust 原生确认。"
                        ),
                    },
                )
            }
            Err(error) => Ok(tool_error(
                request,
                error.code(),
                "Rust refused to create an unsafe external Agent disconnection plan",
            )),
        }
    }

    fn search_model_catalog(
        &mut self,
        request: &ToolCallRequestPayload,
        query: &str,
    ) -> Result<ToolCallResultPayload, AgentToolExecutionError> {
        let source = match self.executor.database.default_download_source() {
            Ok(Some(source)) => source,
            Ok(None) => {
                return Ok(tool_error(
                    request,
                    "default_download_source_required",
                    "choose a default model source in HAL100 before Agent catalog search",
                ));
            }
            Err(error) => {
                let error = AgentToolExecutionError::from(error);
                return Ok(tool_error(
                    request,
                    error.code(),
                    "Rust could not read the configured model source",
                ));
            }
        };
        let catalog = self.executor.remote_catalog.clone();
        let mut results = match self.await_remote(catalog.search(source, query))? {
            Ok(results) => results,
            Err(error) => {
                let error = AgentToolExecutionError::from(error);
                return Ok(tool_error(
                    request,
                    error.code(),
                    "Rust could not complete the bounded public model search",
                ));
            }
        };
        results.items.retain(|item| !item.gated && !item.private);
        results.items.truncate(8);
        self.record_completed(
            request,
            MODEL_CATALOG_SEARCH_TOOL,
            "搜索公开模型目录",
            format!(
                "Rust 已使用 HAL100 当前默认来源返回 {} 个公开仓库摘要。",
                results.items.len()
            ),
        );
        self.model_repository = None;
        self.model_search = Some(results.clone());
        success(request, results)
    }

    fn inspect_model_repository(
        &mut self,
        request: &ToolCallRequestPayload,
        repository: &str,
    ) -> Result<ToolCallResultPayload, AgentToolExecutionError> {
        let Some(search) = self.model_search.as_ref() else {
            return Ok(tool_error(
                request,
                "model_catalog_search_required",
                "search_model_catalog must complete before inspecting a repository",
            ));
        };
        if !search
            .items
            .iter()
            .any(|item| item.repository == repository)
        {
            return Ok(tool_error(
                request,
                "repository_not_in_search_results",
                "repository must exactly match one result from this Agent run",
            ));
        }
        let source = search.source;
        let catalog = self.executor.remote_catalog.clone();
        let mut detail = match self.await_remote(catalog.repository(source, repository))? {
            Ok(detail) => detail,
            Err(error) => {
                let error = AgentToolExecutionError::from(error);
                return Ok(tool_error(
                    request,
                    error.code(),
                    "Rust could not inspect the selected public model repository",
                ));
            }
        };
        if detail.gated || detail.private {
            return Ok(tool_error(
                request,
                "repository_requires_authorization",
                "Agent downloads are limited to public ungated repositories",
            ));
        }
        detail.files.retain(|file| {
            file.sha256.is_some() && file.path.chars().count() <= 512 && file.revision.len() <= 200
        });
        detail.files.truncate(12);
        if detail.files.is_empty() {
            return Ok(tool_error(
                request,
                "repository_has_no_verifiable_gguf",
                "repository has no GGUF file with a trusted SHA-256",
            ));
        }
        self.record_completed(
            request,
            MODEL_REPOSITORY_INSPECTION_TOOL,
            "检查模型仓库",
            format!(
                "Rust 已返回“{}”中 {} 个带可信 SHA-256 的 GGUF 文件。",
                detail.display_name,
                detail.files.len()
            ),
        );
        self.model_repository = Some(detail.clone());
        success(request, detail)
    }

    fn plan_model_download(
        &mut self,
        request: &ToolCallRequestPayload,
        remote_path: &str,
    ) -> Result<ToolCallResultPayload, AgentToolExecutionError> {
        if self.has_action_plan() {
            return Ok(action_already_planned(request));
        }
        let Some(repository) = self.model_repository.as_ref() else {
            return Ok(tool_error(
                request,
                "model_repository_inspection_required",
                "inspect_model_repository must complete before planning a download",
            ));
        };
        if !repository
            .files
            .iter()
            .any(|file| file.path == remote_path && file.sha256.is_some())
        {
            return Ok(tool_error(
                request,
                "remote_file_not_in_repository_snapshot",
                "remotePath must exactly match a verifiable GGUF from this Agent run",
            ));
        }
        let available = match MacOsSystemProbe
            .model_storage_available_bytes(&self.executor.model_storage_path)
        {
            Ok(available) => available,
            Err(error) => {
                let error = AgentToolExecutionError::from(error);
                return Ok(tool_error(
                    request,
                    error.code(),
                    "Rust could not recheck model storage capacity",
                ));
            }
        };
        let manager = self.executor.model_download.clone();
        let download = match self.await_remote(manager.plan_download(
            repository.source,
            &repository.repository,
            remote_path,
            available,
        ))? {
            Ok(plan) => plan,
            Err(error) => {
                let error = AgentToolExecutionError::from(error);
                return Ok(tool_error(
                    request,
                    error.code(),
                    "Rust refused to create a stale or unverifiable model download plan",
                ));
            }
        };
        let hash = download
            .file
            .sha256
            .clone()
            .ok_or(AgentToolExecutionError::InvalidProtocol)?;
        let pending = PendingAgentAction {
            executor: AgentActionExecutor::DownloadModel {
                download_plan_id: download.plan_id.clone(),
            },
            plan: AgentActionPlan {
                plan_id: next_plan_id(),
                run_id: self.run_id.clone(),
                action_kind: AgentActionKind::DownloadModel,
                target_id: format!(
                    "{}@{}:{}",
                    download.repository, download.file.revision, download.file.path
                ),
                target_name: format!("{} / {}", download.display_name, download.file.path),
                current_state: Some(format!(
                    "HAL100 已从 {:?} 重新检查公开仓库与精确文件元数据",
                    download.source
                )),
                details: vec![
                    format!("仓库：{}", download.repository),
                    format!("修订：{}", download.file.revision),
                    format!("远端文件：{}", download.file.path),
                    format!("文件大小：{} 字节", download.file.size_bytes),
                    format!("SHA-256：{hash}"),
                    format!(
                        "所需空间（含安全余量）：{} 字节",
                        download.required_storage_bytes
                    ),
                ],
                expires_at_ms: download.expires_at_ms,
                action_summary: download.action_summary,
                requires_native_confirmation: true,
            },
        };
        let target_name = pending.plan.target_name.clone();
        self.register_pending_action(
            pending,
            request,
            PendingActionPresentation {
                tool_name: PLAN_MODEL_DOWNLOAD_TOOL,
                label: "生成模型下载计划",
                summary: format!(
                    "已为“{target_name}”生成一次性下载计划；尚未下载，必须通过 Rust 原生确认。"
                ),
            },
        )
    }

    fn register_pending_action(
        &mut self,
        pending: PendingAgentAction,
        request: &ToolCallRequestPayload,
        presentation: PendingActionPresentation,
    ) -> Result<ToolCallResultPayload, AgentToolExecutionError> {
        let plan = pending.plan.clone();
        self.executor.action_plans.register(pending)?;
        self.executor.database.insert_audit_event(
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
        self.tool_events.push(AgentToolEvent {
            tool_call_id: request.tool_call_id.clone(),
            tool_name: presentation.tool_name.to_owned(),
            label: presentation.label.to_owned(),
            status: "awaiting_confirmation".to_owned(),
            summary: presentation.summary,
        });
        self.action_plans.push(plan.clone());
        success(request, plan)
    }

    fn record_completed(
        &mut self,
        request: &ToolCallRequestPayload,
        tool_name: &'static str,
        label: &'static str,
        summary: String,
    ) {
        self.tool_events.push(AgentToolEvent {
            tool_call_id: request.tool_call_id.clone(),
            tool_name: tool_name.to_owned(),
            label: label.to_owned(),
            status: "completed".to_owned(),
            summary,
        });
    }

    fn completed(&self, tool_name: &str) -> bool {
        self.tool_events
            .iter()
            .any(|event| event.tool_name == tool_name)
    }

    fn has_action_plan(&self) -> bool {
        !self.action_plans.is_empty()
    }

    fn await_remote<F, T>(&self, future: F) -> Result<T, AgentToolExecutionError>
    where
        F: Future<Output = T>,
    {
        block_on_cancellable(&self.runtime_handle, &self.cancellation, future)
    }
}

fn block_on_cancellable<F, T>(
    runtime_handle: &tokio::runtime::Handle,
    cancellation: &AtomicBool,
    future: F,
) -> Result<T, AgentToolExecutionError>
where
    F: Future<Output = T>,
{
    if cancellation.load(Ordering::Acquire) {
        return Err(AgentToolExecutionError::Cancelled);
    }
    runtime_handle.block_on(async {
        tokio::pin!(future);
        let mut poll = tokio::time::interval(TOOL_CANCELLATION_POLL);
        poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;
                _ = poll.tick() => {
                    if cancellation.load(Ordering::Acquire) {
                        return Err(AgentToolExecutionError::Cancelled);
                    }
                }
                output = &mut future => return Ok(output),
            }
        }
    })
}

fn is_user_backend(backend_id: &str) -> bool {
    backend_id != BUILT_IN_AGENT_RUNTIME.runtime_id
        && !backend_id.starts_with(INTERNAL_CLOUD_BACKEND_PREFIX)
}

fn sample_state(
    sample: &AgentOperationalHealthSample,
) -> (hal100_protocol::EngineRuntimeState, bool, u32, u32) {
    (
        sample.engine_runtime_state,
        sample.active_route,
        sample.registered_backend_count,
        sample.open_circuit_count,
    )
}

fn build_runtime_catalog(
    executor: &AgentToolExecutor,
) -> Result<AgentRuntimeCatalog, AgentToolExecutionError> {
    executor.database.refresh_local_model_states()?;
    let engine = executor.engine.status()?;
    let routing = executor.gateway.routing_snapshot();
    let mut models = executor
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

fn build_model_start_plan(
    executor: &AgentToolExecutor,
    run_id: &str,
    model_id: &str,
) -> Result<Option<PendingAgentAction>, AgentToolExecutionError> {
    executor.database.refresh_local_model_states()?;
    let engine = executor.engine.status()?;
    if engine.install_state != EngineInstallState::Installed {
        return Ok(None);
    }
    let Some(model) = executor
        .database
        .local_model(model_id)?
        .filter(|model| model.state == LocalModelState::Ready)
    else {
        return Ok(None);
    };
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
            plan_id: next_plan_id(),
            run_id: run_id.to_owned(),
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
    executor: &AgentToolExecutor,
    run_id: &str,
    model_id: &str,
) -> Result<PendingAgentAction, AgentToolExecutionError> {
    executor.database.refresh_local_model_states()?;
    let engine = executor.engine.status()?;
    let removal_plan = executor
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
            plan_id: next_plan_id(),
            run_id: run_id.to_owned(),
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
    executor: &AgentToolExecutor,
    run_id: &str,
) -> Result<Option<PendingAgentAction>, AgentToolExecutionError> {
    let status = executor.engine.status()?;
    if status.install_state == EngineInstallState::Installed {
        return Ok(None);
    }
    let engine_plan = executor.engine.plan_install()?;
    Ok(Some(PendingAgentAction {
        executor: AgentActionExecutor::InstallLlamaCpp {
            engine_plan_id: engine_plan.plan_id.clone(),
        },
        plan: AgentActionPlan {
            plan_id: next_plan_id(),
            run_id: run_id.to_owned(),
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
    executor: &AgentToolExecutor,
    run_id: &str,
) -> Result<Option<PendingAgentAction>, AgentToolExecutionError> {
    let status = executor.engine.status()?;
    if status.install_state == EngineInstallState::NotInstalled {
        return Ok(None);
    }
    let engine_plan = executor.engine.plan_remove()?;
    Ok(Some(PendingAgentAction {
        executor: AgentActionExecutor::RemoveLlamaCpp {
            engine_plan_id: engine_plan.plan_id.clone(),
        },
        plan: AgentActionPlan {
            plan_id: next_plan_id(),
            run_id: run_id.to_owned(),
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

fn build_external_agent_status(
    executor: &AgentToolExecutor,
    integration_id: ExternalAgentIntegrationId,
) -> Result<AgentExternalIntegrationStatus, AgentToolExecutionError> {
    let descriptor = ExternalAgentIntegrationRegistry::descriptor(integration_id);
    let managed_installation = integration_id == ExternalAgentIntegrationId::PiCodingAgent
        && executor.managed_deployment.managed_pi_installed()?;
    let (installed, version, integration_state, configured_protocol, warning_count) =
        match integration_id {
            ExternalAgentIntegrationId::OpenCode => {
                let detection = executor.open_code.detect()?;
                let state = match detection.integration_state {
                    OpenCodeIntegrationState::NotConfigured if detection.installed => {
                        ExternalAgentIntegrationState::InstalledNotConfigured
                    }
                    OpenCodeIntegrationState::NotConfigured => {
                        ExternalAgentIntegrationState::NotInstalled
                    }
                    OpenCodeIntegrationState::Configured => {
                        ExternalAgentIntegrationState::Configured
                    }
                    OpenCodeIntegrationState::Conflict => ExternalAgentIntegrationState::Conflict,
                    OpenCodeIntegrationState::ModifiedOutsideHal100 => {
                        ExternalAgentIntegrationState::ModifiedOutsideHal100
                    }
                };
                let protocol = (state == ExternalAgentIntegrationState::Configured)
                    .then_some(ExternalAgentGatewayProtocol::OpenAiChatCompletions);
                (
                    detection.installed,
                    detection.version,
                    state,
                    protocol,
                    detection.warnings.len(),
                )
            }
            ExternalAgentIntegrationId::PiCodingAgent => {
                let detection = executor.pi_coding_agent.detect()?;
                (
                    detection.installed,
                    detection.version,
                    detection.integration_state,
                    detection.configured_protocol,
                    detection.warnings.len(),
                )
            }
            ExternalAgentIntegrationId::OpenClaw => {
                let detection = executor.openclaw.detect()?;
                (
                    detection.installed,
                    detection.version,
                    detection.integration_state,
                    detection.configured_protocol,
                    detection.warnings.len(),
                )
            }
            ExternalAgentIntegrationId::HermesAgent => {
                let detection = executor.hermes_agent.detect()?;
                (
                    detection.installed,
                    detection.version,
                    detection.integration_state,
                    detection.configured_protocol,
                    detection.warnings.len(),
                )
            }
        };
    Ok(AgentExternalIntegrationStatus {
        integration_id: descriptor.integration_id.to_owned(),
        display_name: descriptor.display_name.to_owned(),
        installed,
        managed_installation,
        version,
        integration_state,
        configured_protocol,
        warning_count: warning_count.try_into().unwrap_or(u32::MAX),
    })
}

fn build_external_agent_configuration_plan(
    executor: &AgentToolExecutor,
    run_id: &str,
    integration_id: ExternalAgentIntegrationId,
) -> Result<PendingAgentAction, AgentToolExecutionError> {
    let descriptor = ExternalAgentIntegrationRegistry::descriptor(integration_id);
    let (integration_plan_id, expires_at_ms, creates_backup, preserves_default_model, protocol) =
        match integration_id {
            ExternalAgentIntegrationId::OpenCode => {
                let plan = executor.open_code.plan_configuration()?;
                (
                    plan.plan_id,
                    plan.expires_at_ms,
                    plan.creates_backup,
                    plan.preserves_default_model,
                    ExternalAgentGatewayProtocol::OpenAiChatCompletions,
                )
            }
            ExternalAgentIntegrationId::PiCodingAgent => {
                let plan = executor.pi_coding_agent.plan_configuration()?;
                (
                    plan.plan_id,
                    plan.expires_at_ms,
                    plan.creates_backup,
                    plan.preserves_default_model,
                    plan.gateway_protocol,
                )
            }
            ExternalAgentIntegrationId::OpenClaw => {
                let plan = executor
                    .openclaw
                    .plan_configuration(ExternalAgentGatewayProtocol::OpenAiChatCompletions)?;
                (
                    plan.plan_id,
                    plan.expires_at_ms,
                    plan.creates_backup,
                    plan.preserves_default_model,
                    plan.gateway_protocol,
                )
            }
            ExternalAgentIntegrationId::HermesAgent => {
                let plan = executor.hermes_agent.plan_configuration()?;
                (
                    plan.plan_id,
                    plan.expires_at_ms,
                    plan.creates_backup,
                    plan.preserves_default_model,
                    plan.gateway_protocol,
                )
            }
        };
    Ok(PendingAgentAction {
        executor: AgentActionExecutor::ConfigureExternalAgent {
            integration_id,
            integration_plan_id,
        },
        plan: AgentActionPlan {
            plan_id: next_plan_id(),
            run_id: run_id.to_owned(),
            action_kind: AgentActionKind::ConfigureExternalAgent,
            target_id: descriptor.integration_id.to_owned(),
            target_name: descriptor.display_name.to_owned(),
            current_state: Some("Rust 已检查现有配置快照与 HAL100 受管片段所有权".to_owned()),
            details: vec![
                format!("使用受支持的 {:?} Gateway 协议", protocol),
                if preserves_default_model {
                    "保留用户默认模型，并拒绝覆盖冲突配置".to_owned()
                } else {
                    "只写入适配器声明的 HAL100 受管配置".to_owned()
                },
                "使用该外部 Agent 的独立 Gateway 凭据".to_owned(),
                if creates_backup {
                    "写入前创建原配置备份；写后验证失败时自动回滚".to_owned()
                } else {
                    "写后验证失败时移除新配置并自动回滚".to_owned()
                },
            ],
            expires_at_ms,
            action_summary: format!(
                "以事务方式为 {} 写入 HAL100 受管 Gateway 配置和独立凭据",
                descriptor.display_name
            ),
            requires_native_confirmation: true,
        },
    })
}

fn build_external_agent_disconnection_plan(
    executor: &AgentToolExecutor,
    run_id: &str,
    integration_id: ExternalAgentIntegrationId,
) -> Result<PendingAgentAction, AgentToolExecutionError> {
    let descriptor = ExternalAgentIntegrationRegistry::descriptor(integration_id);
    let (integration_plan_id, expires_at_ms, creates_backup, revokes_credential) =
        match integration_id {
            ExternalAgentIntegrationId::OpenCode => {
                let plan = executor.open_code.plan_disconnection()?;
                (
                    plan.plan_id,
                    plan.expires_at_ms,
                    plan.creates_backup,
                    plan.revokes_credential,
                )
            }
            ExternalAgentIntegrationId::PiCodingAgent => {
                let plan = executor.pi_coding_agent.plan_disconnection()?;
                (
                    plan.plan_id,
                    plan.expires_at_ms,
                    plan.creates_backup,
                    plan.revokes_credential,
                )
            }
            ExternalAgentIntegrationId::OpenClaw => {
                let plan = executor.openclaw.plan_disconnection()?;
                (
                    plan.plan_id,
                    plan.expires_at_ms,
                    plan.creates_backup,
                    plan.revokes_credential,
                )
            }
            ExternalAgentIntegrationId::HermesAgent => {
                let plan = executor.hermes_agent.plan_disconnection()?;
                (
                    plan.plan_id,
                    plan.expires_at_ms,
                    plan.creates_backup,
                    plan.revokes_credential,
                )
            }
        };
    Ok(PendingAgentAction {
        executor: AgentActionExecutor::DisconnectExternalAgent {
            integration_id,
            integration_plan_id,
        },
        plan: AgentActionPlan {
            plan_id: next_plan_id(),
            run_id: run_id.to_owned(),
            action_kind: AgentActionKind::DisconnectExternalAgent,
            target_id: descriptor.integration_id.to_owned(),
            target_name: descriptor.display_name.to_owned(),
            current_state: Some("Rust 已确认 HAL100 受管配置与独立凭据仍保持原快照".to_owned()),
            details: vec![
                "只移除 HAL100 受管配置片段，不删除用户配置文件".to_owned(),
                if revokes_credential {
                    "撤销并删除该外部 Agent 的专属 Gateway 凭据".to_owned()
                } else {
                    "不操作任何非 HAL100 凭据".to_owned()
                },
                if creates_backup {
                    "修改前创建原配置备份；任一步失败时自动回滚".to_owned()
                } else {
                    "任一步失败时恢复原有受管状态".to_owned()
                },
            ],
            expires_at_ms,
            action_summary: format!(
                "以事务方式断开 {}，仅移除 HAL100 受管资源",
                descriptor.display_name
            ),
            requires_native_confirmation: true,
        },
    })
}

fn build_diagnostic_repair_plan(
    executor: &AgentToolExecutor,
    run_id: &str,
    report: &EnvironmentDiagnosticReport,
    finding_id: &str,
) -> Result<PendingAgentAction, AgentToolExecutionError> {
    let finding = report
        .findings
        .iter()
        .find(|finding| finding.finding_id == finding_id)
        .ok_or(AgentToolExecutionError::InvalidProtocol)?;
    let repair_kind = finding
        .repair_kind
        .ok_or(AgentToolExecutionError::InvalidProtocol)?;
    let mut pending = match repair_kind {
        DiagnosticRepairKind::InstallLlamaCpp => build_engine_install_plan(executor, run_id)?
            .ok_or(AgentToolExecutionError::ActionPlan(
                AgentActionPlanError::Unavailable,
            ))?,
        DiagnosticRepairKind::ConfigureExternalAgent => {
            let target_id = finding
                .target_id
                .as_deref()
                .ok_or(AgentToolExecutionError::InvalidProtocol)?;
            let integration_id = ExternalAgentIntegrationRegistry::by_integration_id(target_id)
                .map(|descriptor| descriptor.id)
                .ok_or(AgentToolExecutionError::InvalidProtocol)?;
            build_external_agent_configuration_plan(executor, run_id, integration_id)?
        }
        DiagnosticRepairKind::RemoveModelIndex => {
            let target_id = finding
                .target_id
                .as_deref()
                .ok_or(AgentToolExecutionError::InvalidProtocol)?;
            executor.database.refresh_local_model_states()?;
            let model = executor.database.local_model(target_id)?.ok_or(
                AgentToolExecutionError::ActionPlan(AgentActionPlanError::Unavailable),
            )?;
            if model.state != LocalModelState::Missing {
                return Err(AgentToolExecutionError::ActionPlan(
                    AgentActionPlanError::Unavailable,
                ));
            }
            build_model_removal_plan(executor, run_id, target_id)?
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

fn success(
    request: &ToolCallRequestPayload,
    output: impl serde::Serialize,
) -> Result<ToolCallResultPayload, AgentToolExecutionError> {
    let result = ToolCallResultPayload::success(
        &request.tool_call_id,
        serde_json::to_value(output).map_err(|_| AgentToolExecutionError::InvalidProtocol)?,
    );
    let serialized =
        serde_json::to_vec(&result).map_err(|_| AgentToolExecutionError::InvalidProtocol)?;
    if serialized.len() > AGENT_RPC_MAX_TOOL_RESULT_BYTES {
        return Err(AgentToolExecutionError::ResultTooLarge);
    }
    Ok(result)
}

fn tool_error(
    request: &ToolCallRequestPayload,
    code: impl Into<String>,
    message: impl Into<String>,
) -> ToolCallResultPayload {
    ToolCallResultPayload::error(&request.tool_call_id, code, message)
}

fn action_already_planned(request: &ToolCallRequestPayload) -> ToolCallResultPayload {
    tool_error(
        request,
        "action_already_planned",
        "only one mutating action plan is allowed per Agent run",
    )
}

fn next_plan_id() -> String {
    format!("agent-plan-{}", Uuid::new_v4().simple())
}

fn safe_operational_identifier(value: &str) -> String {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_-.:".contains(character));
    if valid {
        value.to_owned()
    } else {
        "unknown".to_owned()
    }
}

fn safe_operational_detail(details: &[hal100_protocol::AuditDetail], key: &str) -> Option<String> {
    details
        .iter()
        .find(|detail| detail.key == key)
        .map(|detail| safe_operational_identifier(&detail.value))
        .filter(|value| value != "unknown")
}

fn saturating_u32(value: usize) -> u32 {
    value.try_into().unwrap_or(u32::MAX)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}

#[cfg(test)]
#[path = "agent_tools_tests.rs"]
mod tests;
