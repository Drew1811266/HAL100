use serde::{Deserialize, Serialize};

use crate::{
    BackendKind, EngineInstallState, EngineRuntimeState, EnvironmentDiagnosticReport,
    ExternalAgentGatewayProtocol, ExternalAgentIntegrationState, InferenceAccelerator,
    InferenceArchitecture, InferenceDeployment, InferenceEngineKind, InferenceEngineOwnership,
    InferenceEngineRecommendation, InferenceEngineSupportEvidenceSummary,
    InferenceEngineSupportStatus, InferenceModelFormat, InferencePlatform, InferenceProtocol,
    RuntimeProfileIssue, RuntimeProfileReadiness,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentComponentState {
    Unavailable,
    Stopped,
    Starting,
    Running,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentIntentShadowProposalStatus {
    NotRequested,
    Proposed,
    Invalid,
    Failed,
    Rejected,
    ProtocolError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentIntentShadowAdjudicationOutcome {
    Agreement,
    DeterministicGuard,
    DeterministicOnly,
    ProposalCandidate,
    Conflict,
    Unresolved,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentIntentShadowMetrics {
    pub sample_count: u64,
    pub deterministic_resolved_count: u64,
    pub pi_requested_count: u64,
    pub pi_proposed_count: u64,
    pub pi_invalid_count: u64,
    pub pi_failed_count: u64,
    pub pi_rejected_count: u64,
    pub pi_protocol_error_count: u64,
    pub agreement_count: u64,
    pub deterministic_guard_count: u64,
    pub deterministic_only_count: u64,
    pub proposal_candidate_count: u64,
    pub conflict_count: u64,
    pub unresolved_count: u64,
    pub cumulative_pi_latency_ms: u64,
    pub max_pi_latency_ms: u64,
    pub last_pi_latency_ms: Option<u64>,
    pub last_proposal_status: Option<AgentIntentShadowProposalStatus>,
    pub last_adjudication_outcome: Option<AgentIntentShadowAdjudicationOutcome>,
    pub updated_at_ms: Option<i64>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentTaskRoutingMode {
    #[default]
    Controlled,
    SafeLegacy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentTaskRoutingDecision {
    StructuredDeterministic,
    StructuredPi,
    GuardedResponse,
    SafeLegacyDeterministic,
    LegacyNoToolFallback,
    FailClosed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskRoutingMetrics {
    pub sample_count: u64,
    pub structured_deterministic_count: u64,
    pub structured_pi_count: u64,
    pub guarded_response_count: u64,
    pub safe_legacy_deterministic_count: u64,
    pub legacy_no_tool_fallback_count: u64,
    pub fail_closed_count: u64,
    pub last_decision: Option<AgentTaskRoutingDecision>,
    pub updated_at_ms: Option<i64>,
}

pub const AGENT_TASK_CHECKPOINT_SCHEMA_VERSION: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentTaskCheckpointPhase {
    Draft,
    Clarifying,
    Inspecting,
    Planning,
    AwaitingConfirmation,
    Executing,
    Verifying,
    Completed,
    Blocked,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentTaskVerificationState {
    NotStarted,
    Pending,
    Satisfied,
    Unsatisfied,
    EvidenceUnavailable,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentTaskEvidenceSource {
    SystemProbe,
    RuntimeCatalog,
    EnvironmentDiagnostics,
    OperationalHistory,
    OperationalHealth,
    ModelCatalog,
    ModelRepository,
    ExternalIntegrationStatus,
    ActionPlan,
    RuntimeRecheck,
    RuntimeProfileRecheck,
    ModelLibraryRecheck,
    EngineRecheck,
    IntegrationRecheck,
    ManagedInstallationRecheck,
    RepairDiagnosticRecheck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentTaskRecoveryScope {
    None,
    InProcessClarification,
    InProcessConfirmation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentClarificationKind {
    ExternalAgentTarget,
    ManagedOwnership,
    SingleMutationTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentClarificationChoice {
    SelectExternalAgent,
    RemoveManagedRuntime,
    DisconnectOnly,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentExternalAgentChoice {
    OpenCode,
    PiCodingAgent,
    OpenClaw,
    HermesAgent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentClarificationOption {
    pub choice: AgentClarificationChoice,
    #[serde(default)]
    pub external_agent: Option<AgentExternalAgentChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentClarification {
    pub kind: AgentClarificationKind,
    pub options: Vec<AgentClarificationOption>,
    pub attempt_count: u8,
    pub max_attempts: u8,
    pub expires_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentClarificationAnswerRequest {
    pub kind: AgentClarificationKind,
    pub choice: AgentClarificationChoice,
    #[serde(default)]
    pub external_agent: Option<AgentExternalAgentChoice>,
    #[serde(default)]
    pub cloud_target: Option<AgentCloudTarget>,
}

/// A bounded semantic checkpoint for the current or most recent Agent task.
///
/// It deliberately carries no prompt, answer, resource identifier, plan identifier, path,
/// credential, tool arguments, or raw tool result. Checkpoints live only in the desktop process;
/// an application restart invalidates any pending confirmation instead of restoring authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskCheckpoint {
    pub schema_version: u8,
    pub phase: AgentTaskCheckpointPhase,
    pub checkpoint_sequence: u32,
    pub task_kind: String,
    pub target_kind: String,
    pub desired_state: String,
    pub provider_mode: String,
    pub data_scope: String,
    pub success_predicate: String,
    pub pending_action_plan: bool,
    pub native_confirmation_required: bool,
    pub verification_state: AgentTaskVerificationState,
    pub evidence_source: Option<AgentTaskEvidenceSource>,
    pub evidence_observation_count: u8,
    pub replan_attempt_count: u8,
    pub max_replan_attempts: u8,
    #[serde(default)]
    pub clarification_kind: Option<AgentClarificationKind>,
    #[serde(default)]
    pub clarification_attempt_count: u8,
    #[serde(default)]
    pub max_clarification_attempts: u8,
    #[serde(default)]
    pub clarification_expires_at_ms: Option<i64>,
    pub recovery_scope: AgentTaskRecoveryScope,
    pub updated_at_ms: i64,
}

pub const AGENT_TASK_GRAPH_CHECKPOINT_SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentTaskGraphCheckpointState {
    Active,
    Succeeded,
    Failed,
    Compensating,
    Compensated,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentTaskGraphNodeCheckpointState {
    Blocked,
    Ready,
    Running,
    AwaitingConfirmation,
    Succeeded,
    Failed,
    Compensating,
    Compensated,
    Cancelled,
}

/// A redacted, non-authorizing snapshot of a Rust-owned composite task graph.
///
/// Node indexes and dependency indexes describe only the bounded graph shape. This object never
/// carries resource identifiers, prompts, answers, action/run identifiers, arguments, paths,
/// credentials, or raw tool/evidence payloads. `requires_reauthorization` records that an
/// in-process state lost authority; it does not grant authority to resume it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentTaskGraphNodeCheckpoint {
    pub node_index: u8,
    pub state: AgentTaskGraphNodeCheckpointState,
    pub task_kind: String,
    pub target_kind: String,
    pub success_predicate: String,
    pub dependency_indexes: Vec<u8>,
    pub evidence_source: Option<AgentTaskEvidenceSource>,
    pub changed_owned_state: bool,
    pub requires_reauthorization: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentTaskGraphCheckpoint {
    pub schema_version: u8,
    pub checkpoint_sequence: u32,
    pub state: AgentTaskGraphCheckpointState,
    pub nodes: Vec<AgentTaskGraphNodeCheckpoint>,
    pub ready_node_count: u8,
    pub succeeded_node_count: u8,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentTaskGraphKind {
    PrepareExternalAgent,
    PrepareManagedPi,
}

/// Exact user-selected inputs for a Rust-owned graph factory. This request is not accepted from
/// Pi model output and contains no action or confirmation authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentTaskGraphStartRequest {
    pub kind: AgentTaskGraphKind,
    pub model_id: String,
    #[serde(default)]
    pub external_agent: Option<AgentExternalAgentChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub kernel_state: AgentComponentState,
    pub model_runtime_state: AgentComponentState,
    pub pi_version: String,
    pub model_name: String,
    pub model_prepared: bool,
    pub model_size_bytes: u64,
    pub capacity_tier: String,
    pub context_window_tokens: u32,
    pub available_input_tokens_before_reserve: u32,
    pub max_output_tokens: u32,
    pub idle_timeout_seconds: u32,
    pub active_run_id: Option<String>,
    pub cancellation_requested: bool,
    pub last_error_code: Option<String>,
    #[serde(default)]
    pub intent_shadow_metrics: AgentIntentShadowMetrics,
    #[serde(default)]
    pub task_routing_mode: AgentTaskRoutingMode,
    #[serde(default)]
    pub task_routing_metrics: AgentTaskRoutingMetrics,
    #[serde(default)]
    pub task_checkpoint: Option<AgentTaskCheckpoint>,
    #[serde(default)]
    pub task_graph_checkpoint: Option<AgentTaskGraphCheckpoint>,
    #[serde(default)]
    pub recoverable_task_graph_checkpoint: Option<AgentTaskGraphCheckpoint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPromptRequest {
    pub prompt: String,
    #[serde(default)]
    pub cloud_target: Option<AgentCloudTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentProviderProtocol {
    LocalOpenAi,
    CloudOpenAi,
    CloudAnthropic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCloudTarget {
    pub backend_id: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCloudRunPreview {
    pub backend_id: String,
    pub backend_name: String,
    pub backend_kind: BackendKind,
    pub api_root: String,
    pub model: String,
    pub prompt_bytes: u32,
    pub sends_system_instructions: bool,
    pub may_send_tool_results: bool,
    pub sends_credentials_to_sidecar: bool,
    pub sends_local_paths: bool,
    pub confirmation_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCloudSessionPreview {
    pub backend_id: String,
    pub backend_name: String,
    pub backend_kind: BackendKind,
    pub api_root: String,
    pub model: String,
    pub sends_future_prompts: bool,
    pub sends_system_instructions: bool,
    pub may_send_tool_results: bool,
    pub stores_conversation_history: bool,
    pub sends_credentials_to_sidecar: bool,
    pub sends_local_paths: bool,
    pub confirmation_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCloudSessionStatus {
    pub active: bool,
    pub available: bool,
    pub backend_id: Option<String>,
    pub backend_name: Option<String>,
    pub backend_kind: Option<BackendKind>,
    pub api_root: Option<String>,
    pub model: Option<String>,
    pub provider_protocol: Option<AgentProviderProtocol>,
    pub activated_at_ms: Option<i64>,
    pub last_error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolEvent {
    pub tool_call_id: String,
    pub tool_name: String,
    pub label: String,
    pub status: String,
    pub summary: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunEfficiency {
    pub context_window_tokens: u32,
    pub max_output_tokens: u32,
    pub intent_model_turn_count: u32,
    pub execution_model_turn_count: u32,
    pub total_model_turn_count: u32,
    pub continuation_prompt_count: u32,
    pub provider_usage_available: bool,
    pub reported_input_tokens: u64,
    pub reported_output_tokens: u64,
    pub peak_reported_input_tokens: u64,
    pub peak_estimated_input_tokens: u64,
    pub task_system_prompt_bytes: u64,
    pub compacted_turn_count: u32,
    pub sent_tool_result_bytes: u64,
    pub sent_tool_result_token_estimate: u64,
    pub repeated_tool_result_bytes: u64,
    pub repeated_tool_result_token_estimate: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunResult {
    pub run_id: String,
    pub answer: String,
    pub tool_events: Vec<AgentToolEvent>,
    pub action_plans: Vec<AgentActionPlan>,
    #[serde(default)]
    pub clarification: Option<AgentClarification>,
    #[serde(default)]
    pub efficiency: AgentRunEfficiency,
    pub model_name: String,
    pub started_at_ms: i64,
    pub completed_at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentActionKind {
    StartOrSwitchModel,
    ActivateRuntimeProfile,
    StopModel,
    DownloadModel,
    RemoveModel,
    InstallLlamaCpp,
    RemoveLlamaCpp,
    InstallExternalAgent,
    RemoveExternalAgent,
    ConfigureExternalAgent,
    DisconnectExternalAgent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExternalIntegrationStatus {
    pub integration_id: String,
    pub display_name: String,
    pub installed: bool,
    pub managed_installation: bool,
    pub version: Option<String>,
    pub integration_state: ExternalAgentIntegrationState,
    pub configured_protocol: Option<ExternalAgentGatewayProtocol>,
    pub warning_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOperationalEvent {
    pub event_type: String,
    pub target_type: String,
    pub occurred_at_ms: i64,
    pub error_code: Option<String>,
    pub action: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOperationalHistory {
    pub generated_at_ms: i64,
    pub total_event_count: u64,
    pub returned_event_count: u32,
    pub events: Vec<AgentOperationalEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentOperationalHealthStatus {
    Ready,
    NeedsAttention,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOperationalHealthSample {
    pub observed_at_ms: i64,
    pub engine_runtime_state: EngineRuntimeState,
    pub active_route: bool,
    pub registered_backend_count: u32,
    pub open_circuit_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOperationalHealthObservation {
    pub generated_at_ms: i64,
    pub window_ms: u32,
    pub sample_count: u32,
    pub stable: bool,
    pub status: AgentOperationalHealthStatus,
    pub engine_install_state: EngineInstallState,
    pub ready_model_count: u32,
    pub configured_backend_count: u32,
    pub installed_external_agent_count: u32,
    pub configured_external_agent_count: u32,
    pub attention_external_agent_count: u32,
    pub blocking_codes: Vec<String>,
    pub samples: Vec<AgentOperationalHealthSample>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentActionPlan {
    pub plan_id: String,
    pub run_id: String,
    pub action_kind: AgentActionKind,
    pub target_id: String,
    pub target_name: String,
    pub current_state: Option<String>,
    pub details: Vec<String>,
    pub expires_at_ms: i64,
    pub action_summary: String,
    pub requires_native_confirmation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentActionResult {
    pub plan_id: String,
    pub action_kind: AgentActionKind,
    pub target_id: String,
    pub target_name: String,
    pub outcome_summary: String,
    pub runtime_state: Option<EngineRuntimeState>,
    pub diagnostic_report: Option<EnvironmentDiagnosticReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeModel {
    pub id: String,
    pub display_name: String,
    pub quantization: Option<String>,
    pub size_bytes: u64,
    pub ready: bool,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeProfile {
    pub id: String,
    pub name: String,
    pub description: String,
    pub ownership: InferenceEngineOwnership,
    pub backend_id: Option<String>,
    pub model_id: String,
    pub model_display_name: String,
    pub engine: String,
    pub adapter_variant: String,
    pub adapter_contract_revision: String,
    pub engine_version: String,
    pub context_window_tokens: Option<u32>,
    /// Reviewed performance only when Rust matched this exact saved identity and current device.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_performance: Option<crate::RuntimeProfileReviewedPerformance>,
    pub readiness: RuntimeProfileReadiness,
    pub issues: Vec<RuntimeProfileIssue>,
    pub active: bool,
}

/// Bounded, non-secret projection of one registered inference-engine adapter for Pi.
///
/// This deliberately excludes endpoints, model digests, filesystem paths, commands, credentials,
/// and runtime process details. Pi can use the projection to explain host compatibility and choose
/// an exact saved `runtimeProfileId`; Rust still owns all live discovery, verification and
/// activation authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeEngineCapability {
    pub engine: InferenceEngineKind,
    /// Stable storage key used to correlate this capability with saved runtime profiles.
    pub engine_key: String,
    pub adapter_variant: String,
    pub adapter_contract_revision: String,
    pub display_name: String,
    pub ownership: InferenceEngineOwnership,
    pub deployment: InferenceDeployment,
    pub protocols: Vec<InferenceProtocol>,
    pub model_formats: Vec<InferenceModelFormat>,
    pub platforms: Vec<InferencePlatform>,
    pub architectures: Vec<InferenceArchitecture>,
    pub accelerators: Vec<InferenceAccelerator>,
    pub managed_lifecycle: bool,
    pub compatible: bool,
    pub support_status: Option<InferenceEngineSupportStatus>,
    pub matched_accelerators: Vec<InferenceAccelerator>,
    pub issues: Vec<crate::EngineHostCompatibilityIssue>,
    #[serde(default)]
    pub support_evidence: Option<InferenceEngineSupportEvidenceSummary>,
    #[serde(default)]
    pub support_cells: Vec<AgentRuntimeSupportCell>,
    #[serde(default)]
    pub saved_profile_count: u32,
    #[serde(default)]
    pub recommendation: Option<InferenceEngineRecommendation>,
}

/// Redacted support-cell coordinates exposed to Pi for explanation and deterministic selection.
/// It contains no evidence values or runtime identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeSupportCell {
    pub platform: InferencePlatform,
    pub architecture: InferenceArchitecture,
    pub accelerator: InferenceAccelerator,
    pub deployment: InferenceDeployment,
    pub status: InferenceEngineSupportStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeCatalog {
    pub engine_install_state: EngineInstallState,
    pub engine_runtime_state: EngineRuntimeState,
    pub active_model_id: Option<String>,
    pub active_model_name: Option<String>,
    pub active_backend_id: Option<String>,
    pub configured_backend_count: u32,
    pub models: Vec<AgentRuntimeModel>,
    #[serde(default)]
    pub runtime_profiles: Vec<AgentRuntimeProfile>,
    /// Static engine capability projection used by Pi for bounded, read-only planning context.
    #[serde(default)]
    pub engine_capabilities: Vec<AgentRuntimeEngineCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSystemSummary {
    pub source: String,
    pub platform: String,
    pub architecture: String,
    pub chip: String,
    pub model_identifier: String,
    pub total_unified_memory_bytes: u64,
    pub physical_cpu_cores: u32,
    pub logical_cpu_cores: u32,
    pub model_storage_available_bytes: u64,
    pub recommendation_summary: String,
    pub recommended_parameter_range: String,
    pub recommended_quantization: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_request_defaults_to_the_local_provider_for_older_clients() {
        let request: AgentPromptRequest =
            serde_json::from_str(r#"{"prompt":"检查 HAL100 本地模型"}"#)
                .expect("compatible Agent request");
        assert_eq!(request.cloud_target, None);
    }

    #[test]
    fn cloud_target_contains_a_reference_but_never_a_credential() {
        let request = AgentPromptRequest {
            prompt: "检查 HAL100 后端配置".to_owned(),
            cloud_target: Some(AgentCloudTarget {
                backend_id: "cloud-openai".to_owned(),
                model: "gpt-test".to_owned(),
            }),
        };
        let serialized = serde_json::to_string(&request).expect("serialize Agent request");
        assert!(serialized.contains("cloud-openai"));
        assert!(!serialized.to_lowercase().contains("api_key"));
        assert!(!serialized.to_lowercase().contains("apikey"));
    }

    #[test]
    fn inactive_cloud_session_status_contains_no_target_state() {
        let status = AgentCloudSessionStatus {
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
        };
        assert!(!status.active);
        assert!(status.backend_id.is_none());
        assert!(status.model.is_none());
    }

    #[test]
    fn external_runtime_profile_exposes_bounded_identity_without_execution_authority() {
        let profile = AgentRuntimeProfile {
            id: "runtime-profile-external".to_owned(),
            name: "外部代码助手".to_owned(),
            description: "已验证 Ollama 组合".to_owned(),
            ownership: InferenceEngineOwnership::External,
            backend_id: Some("saved-ollama".to_owned()),
            model_id: "qwen3:8b".to_owned(),
            model_display_name: "qwen3:8b".to_owned(),
            engine: "ollama".to_owned(),
            adapter_variant: "official-loopback-api".to_owned(),
            adapter_contract_revision: "engine-contract-v1".to_owned(),
            engine_version: "0.12.6".to_owned(),
            context_window_tokens: None,
            reviewed_performance: None,
            readiness: RuntimeProfileReadiness::Ready,
            issues: Vec::new(),
            active: false,
        };
        let value = serde_json::to_value(profile).expect("external profile JSON");
        assert_eq!(value["ownership"], "external");
        assert_eq!(value["backendId"], "saved-ollama");
        assert!(value["contextWindowTokens"].is_null());
        let rendered = value.to_string().to_ascii_lowercase();
        for forbidden in [
            "apiroot",
            "digest",
            "credential",
            "apikey",
            "command",
            "path",
        ] {
            assert!(!rendered.contains(forbidden), "leaked {forbidden}");
        }
    }

    #[test]
    fn runtime_catalog_engine_capability_projection_is_bounded_and_non_secret() {
        let capability = AgentRuntimeEngineCapability {
            engine: InferenceEngineKind::Vllm,
            engine_key: "vllm".to_owned(),
            adapter_variant: "official-openai-server".to_owned(),
            adapter_contract_revision: "engine-contract-v1".to_owned(),
            display_name: "vLLM".to_owned(),
            ownership: InferenceEngineOwnership::External,
            deployment: InferenceDeployment::Local,
            protocols: vec![InferenceProtocol::OpenAi],
            model_formats: vec![InferenceModelFormat::Safetensors],
            platforms: vec![InferencePlatform::Linux],
            architectures: vec![InferenceArchitecture::X86_64],
            accelerators: vec![InferenceAccelerator::Cuda],
            managed_lifecycle: false,
            compatible: false,
            support_status: Some(InferenceEngineSupportStatus::Connected),
            matched_accelerators: vec![InferenceAccelerator::Cuda],
            issues: vec![crate::EngineHostCompatibilityIssue::SupportNotFormal],
            support_evidence: Some(InferenceEngineSupportEvidenceSummary::for_status(
                InferenceEngineSupportStatus::Connected,
            )),
            support_cells: vec![AgentRuntimeSupportCell {
                platform: InferencePlatform::Linux,
                architecture: InferenceArchitecture::X86_64,
                accelerator: InferenceAccelerator::Cuda,
                deployment: InferenceDeployment::Local,
                status: InferenceEngineSupportStatus::Connected,
            }],
            saved_profile_count: 0,
            recommendation: None,
        };
        let catalog = AgentRuntimeCatalog {
            engine_install_state: EngineInstallState::NotInstalled,
            engine_runtime_state: EngineRuntimeState::Stopped,
            active_model_id: None,
            active_model_name: None,
            active_backend_id: None,
            configured_backend_count: 0,
            models: Vec::new(),
            runtime_profiles: Vec::new(),
            engine_capabilities: vec![capability],
        };
        let rendered = serde_json::to_string(&catalog).expect("bounded runtime catalog JSON");
        assert!(rendered.contains("engineCapabilities"));
        for forbidden in [
            "apiRoot",
            "digest",
            "credential",
            "apiKey",
            "command",
            "path",
        ] {
            assert!(
                !rendered
                    .to_ascii_lowercase()
                    .contains(&forbidden.to_ascii_lowercase())
            );
        }
    }

    #[test]
    fn model_download_action_uses_the_stable_desktop_wire_name() {
        assert_eq!(
            serde_json::to_string(&AgentActionKind::DownloadModel).expect("action kind"),
            r#""downloadModel""#
        );
    }

    #[test]
    fn intent_shadow_metrics_are_bounded_aggregates_without_prompt_fields() {
        let metrics = AgentIntentShadowMetrics {
            sample_count: 2,
            pi_requested_count: 1,
            last_proposal_status: Some(AgentIntentShadowProposalStatus::Proposed),
            last_adjudication_outcome: Some(
                AgentIntentShadowAdjudicationOutcome::ProposalCandidate,
            ),
            ..AgentIntentShadowMetrics::default()
        };
        let value = serde_json::to_value(metrics).expect("intent shadow metrics");
        assert_eq!(value["sampleCount"], 2);
        assert_eq!(value["lastProposalStatus"], "proposed");
        let rendered = value.to_string().to_ascii_lowercase();
        for forbidden in ["prompt", "answer", "targetid", "runid", "apikey"] {
            assert!(!rendered.contains(forbidden));
        }
    }

    #[test]
    fn task_routing_metrics_expose_only_bounded_rollout_decisions() {
        let metrics = AgentTaskRoutingMetrics {
            sample_count: 3,
            structured_pi_count: 1,
            fail_closed_count: 1,
            last_decision: Some(AgentTaskRoutingDecision::StructuredPi),
            updated_at_ms: Some(1_755_000_000_000),
            ..AgentTaskRoutingMetrics::default()
        };
        let value = serde_json::to_value(metrics).expect("task routing metrics");
        assert_eq!(value["sampleCount"], 3);
        assert_eq!(value["lastDecision"], "structuredPi");
        let rendered = value.to_string().to_ascii_lowercase();
        for forbidden in [
            "prompt", "answer", "targetid", "taskkind", "runid", "apikey",
        ] {
            assert!(!rendered.contains(forbidden));
        }
    }

    #[test]
    fn task_checkpoint_is_versioned_bounded_and_non_authorizing() {
        let checkpoint = AgentTaskCheckpoint {
            schema_version: AGENT_TASK_CHECKPOINT_SCHEMA_VERSION,
            phase: AgentTaskCheckpointPhase::AwaitingConfirmation,
            checkpoint_sequence: 3,
            task_kind: "configure_external_agent".to_owned(),
            target_kind: "external_agent".to_owned(),
            desired_state: "configured".to_owned(),
            provider_mode: "local".to_owned(),
            data_scope: "integration_metadata".to_owned(),
            success_predicate: "integration_configured".to_owned(),
            pending_action_plan: true,
            native_confirmation_required: true,
            verification_state: AgentTaskVerificationState::Pending,
            evidence_source: Some(AgentTaskEvidenceSource::ActionPlan),
            evidence_observation_count: 1,
            replan_attempt_count: 0,
            max_replan_attempts: 1,
            clarification_kind: None,
            clarification_attempt_count: 0,
            max_clarification_attempts: 0,
            clarification_expires_at_ms: None,
            recovery_scope: AgentTaskRecoveryScope::InProcessConfirmation,
            updated_at_ms: 1_755_000_000_000,
        };
        let value = serde_json::to_value(checkpoint).expect("task checkpoint");
        assert_eq!(value["schemaVersion"], 3);
        assert_eq!(value["phase"], "awaitingConfirmation");
        assert_eq!(value["recoveryScope"], "inProcessConfirmation");
        let rendered = value.to_string().to_ascii_lowercase();
        for forbidden in [
            "prompt",
            "answer",
            "resourceid",
            "targetid",
            "planid",
            "runid",
            "path",
            "credential",
            "apikey",
            "toolresult",
        ] {
            assert!(!rendered.contains(forbidden), "leaked {forbidden}");
        }
    }

    #[test]
    fn task_graph_checkpoint_is_redacted_bounded_and_non_authorizing() {
        let checkpoint = AgentTaskGraphCheckpoint {
            schema_version: AGENT_TASK_GRAPH_CHECKPOINT_SCHEMA_VERSION,
            checkpoint_sequence: 7,
            state: AgentTaskGraphCheckpointState::Active,
            nodes: vec![AgentTaskGraphNodeCheckpoint {
                node_index: 1,
                state: AgentTaskGraphNodeCheckpointState::AwaitingConfirmation,
                task_kind: "configure_external_agent".to_owned(),
                target_kind: "external_agent".to_owned(),
                success_predicate: "integration_configured".to_owned(),
                dependency_indexes: vec![0],
                evidence_source: Some(AgentTaskEvidenceSource::ActionPlan),
                changed_owned_state: false,
                requires_reauthorization: true,
            }],
            ready_node_count: 0,
            succeeded_node_count: 1,
            updated_at_ms: 1_755_000_000_000,
        };
        let mut value = serde_json::to_value(checkpoint).expect("task graph checkpoint");
        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["nodes"][0]["dependencyIndexes"][0], 0);
        assert_eq!(value["nodes"][0]["requiresReauthorization"], true);
        let rendered = value.to_string().to_ascii_lowercase();
        for forbidden in [
            "prompt",
            "answer",
            "resourceid",
            "targetid",
            "planid",
            "runid",
            "path",
            "credential",
            "apikey",
            "toolresult",
            "arguments",
        ] {
            assert!(!rendered.contains(forbidden), "leaked {forbidden}");
        }
        value
            .as_object_mut()
            .expect("checkpoint object")
            .insert("prompt".to_owned(), serde_json::json!("forbidden"));
        assert!(serde_json::from_value::<AgentTaskGraphCheckpoint>(value).is_err());
    }

    #[test]
    fn task_graph_start_request_is_typed_and_contains_no_execution_authority() {
        let request = AgentTaskGraphStartRequest {
            kind: AgentTaskGraphKind::PrepareExternalAgent,
            model_id: "model-1".to_owned(),
            external_agent: Some(AgentExternalAgentChoice::OpenCode),
        };
        let value = serde_json::to_value(request).expect("task graph request");
        assert_eq!(value["kind"], "prepareExternalAgent");
        assert_eq!(value["externalAgent"], "openCode");
        let rendered = value.to_string().to_ascii_lowercase();
        for forbidden in [
            "prompt",
            "planid",
            "runid",
            "tool",
            "arguments",
            "confirmation",
            "credential",
        ] {
            assert!(!rendered.contains(forbidden), "leaked {forbidden}");
        }
    }

    #[test]
    fn clarification_answer_is_a_typed_choice_without_free_form_text_or_authority() {
        let request = AgentClarificationAnswerRequest {
            kind: AgentClarificationKind::ExternalAgentTarget,
            choice: AgentClarificationChoice::SelectExternalAgent,
            external_agent: Some(AgentExternalAgentChoice::OpenCode),
            cloud_target: None,
        };
        let value = serde_json::to_value(request).expect("clarification answer");
        assert_eq!(value["kind"], "externalAgentTarget");
        assert_eq!(value["choice"], "selectExternalAgent");
        assert_eq!(value["externalAgent"], "openCode");
        let rendered = value.to_string().to_ascii_lowercase();
        for forbidden in ["prompt", "answer", "planid", "runid", "authorization"] {
            assert!(!rendered.contains(forbidden), "leaked {forbidden}");
        }
    }
}
