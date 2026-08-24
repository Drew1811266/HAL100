mod agent;
mod agent_ecosystem;
mod agent_rpc;
mod anthropic;
mod backends;
mod desktop;
mod diagnostics;
mod engine;
mod model_management;
mod openai;
mod opencode;
mod remote_models;
mod tool_broker;
mod usage;

pub use agent::{
    AgentActionKind, AgentActionPlan, AgentActionResult, AgentCloudRunPreview,
    AgentCloudSessionPreview, AgentCloudSessionStatus, AgentCloudTarget, AgentComponentState,
    AgentExternalIntegrationStatus, AgentOperationalEvent, AgentOperationalHealthObservation,
    AgentOperationalHealthSample, AgentOperationalHealthStatus, AgentOperationalHistory,
    AgentPromptRequest, AgentProviderProtocol, AgentRunResult, AgentRuntimeCatalog,
    AgentRuntimeModel, AgentStatus, AgentSystemSummary, AgentToolEvent,
};
pub use agent_ecosystem::{
    AgentEcosystemCatalog, BuiltInAgentRuntimeSummary, ExternalAgentConfigurationChange,
    ExternalAgentConfigurationPlan, ExternalAgentConfigurationResult, ExternalAgentDetection,
    ExternalAgentDisconnectPlan, ExternalAgentDisconnectResult, ExternalAgentGatewayProtocol,
    ExternalAgentInputModality, ExternalAgentIntegrationAvailability,
    ExternalAgentIntegrationState, ExternalAgentIntegrationSummary, ExternalAgentManagedChange,
    ExternalAgentManagedChangeAction, ExternalAgentModelProfile,
};
pub use agent_rpc::{
    AGENT_RPC_MAX_ACTION_PLANS, AGENT_RPC_MAX_FRAME_BYTES, AGENT_RPC_MAX_REQUIRED_TOOLS,
    AGENT_RPC_MAX_TOOL_RESULT_BYTES, AGENT_RPC_VERSION, AgentRpcEnvelope, AgentRpcFrameDecoder,
    AgentRpcFrameError, AgentRunCompletedPayload, AgentRunStartPayload, encode_agent_rpc_frame,
};
pub use anthropic::{
    AnthropicError, AnthropicErrorEnvelope, AnthropicMessagesRequestMetadata, AnthropicUsage,
};
pub use backends::{
    BackendAuthMethod, BackendCatalog, BackendDraft, BackendKind, BackendProbeResult,
    BackendProbeStatus, BackendRouteDraft, BackendRouteSummary, BackendSummary,
    LocalBackendCandidate, LocalBackendDiscovery,
};
pub use desktop::{
    AuditDetail, AuditEventSummary, AuditLog, DataCleanupPreview, DataCleanupResult,
    DesktopSettings, GenericClientCatalog, GenericClientCredential, GenericClientSummary,
    OnboardingCompletion, RetentionSettingsDraft,
};
pub use diagnostics::{
    DiagnosticComponent, DiagnosticRepairKind, DiagnosticSeverity, EnvironmentDiagnosticFinding,
    EnvironmentDiagnosticReport, EnvironmentHealthStatus,
};
pub use engine::{
    EngineInstallPlan, EngineInstallState, EngineRemovePlan, EngineRuntimeState, LlamaCppStatus,
};
pub use model_management::{
    DownloadSource, GgufImportPlan, GgufImportResult, HardwareProfile, HardwareRecommendation,
    LocalModelState, LocalModelSummary, ModelLibrary, ModelOwnership, ModelRemovalKind,
    ModelRemovalPlan, ModelRemovalResult, ModelSource,
};
pub use openai::{
    OpenAiChatRequestMetadata, OpenAiError, OpenAiErrorEnvelope, OpenAiPromptTokenDetails,
    OpenAiResponsesInputTokenDetails, OpenAiResponsesRequestMetadata, OpenAiResponsesUsage,
    OpenAiUsage,
};
pub use opencode::{
    OpenCodeApplyResult, OpenCodeConfigChange, OpenCodeConfigFormat, OpenCodeConfigPlan,
    OpenCodeDetection, OpenCodeIntegrationState, OpenCodeProjectDiagnosis,
};
pub use remote_models::{
    ModelDownloadPlan, ModelDownloadSnapshot, ModelDownloadState, RemoteGgufFile,
    RemoteModelRepository, RemoteModelSearchItem, RemoteModelSearchResults,
};
use serde::{Deserialize, Serialize};
pub use tool_broker::{
    ENVIRONMENT_DIAGNOSTICS_TOOL, EXTERNAL_AGENT_STATUS_TOOL, MODEL_CATALOG_SEARCH_TOOL,
    MODEL_REPOSITORY_INSPECTION_TOOL, OPERATIONAL_HEALTH_OBSERVATION_TOOL,
    OPERATIONAL_HISTORY_TOOL, PLAN_DIAGNOSTIC_REPAIR_TOOL, PLAN_ENGINE_INSTALL_TOOL,
    PLAN_ENGINE_REMOVE_TOOL, PLAN_EXTERNAL_AGENT_CONFIGURATION_TOOL,
    PLAN_EXTERNAL_AGENT_DISCONNECTION_TOOL, PLAN_EXTERNAL_AGENT_INSTALLATION_TOOL,
    PLAN_MANAGED_EXTERNAL_AGENT_REMOVAL_TOOL, PLAN_MODEL_DOWNLOAD_TOOL, PLAN_MODEL_REMOVAL_TOOL,
    PLAN_MODEL_START_TOOL, RUNTIME_CATALOG_TOOL, SIMULATED_SYSTEM_SUMMARY_TOOL,
    SYSTEM_SUMMARY_TOOL, ToolCallErrorPayload, ToolCallRequestPayload, ToolCallResultPayload,
    ToolCallResultStatus,
};
pub use usage::{ModelTestResult, UsageDashboard, UsageRequestSummary, UsageTotals};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformSummary {
    pub os: String,
    pub architecture: String,
    pub supported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppOverview {
    pub app_name: String,
    pub version: String,
    pub phase: String,
    pub gateway_state: ServiceState,
    pub database_state: DatabaseState,
    pub platform: PlatformSummary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceState {
    #[serde(rename = "未启动")]
    NotStarted,
    #[serde(rename = "运行中")]
    Running,
    #[serde(rename = "异常")]
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DatabaseState {
    #[serde(rename = "未连接")]
    Disconnected,
    #[serde(rename = "已就绪")]
    Ready,
    #[serde(rename = "异常")]
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayHealth {
    pub service: String,
    pub status: String,
    pub protocol_version: u16,
}
