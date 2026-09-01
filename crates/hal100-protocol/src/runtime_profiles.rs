use serde::{Deserialize, Serialize};

use crate::{
    HostCapabilitySnapshot, InferenceAccelerator, InferenceArchitecture, InferenceDeployment,
    InferenceEngineOwnership, InferencePlatform, LlamaCppStatus,
};

pub const RUNTIME_PROFILE_SPEC_VERSION: u16 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeProfileReadiness {
    Active,
    Ready,
    NeedsVerification,
    NeedsRepair,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeProfileIssue {
    EngineNotInstalled,
    BackendUnavailable,
    BackendIdentityChanged,
    EngineIncompatible,
    EngineVersionChanged,
    ModelUnavailable,
    ModelIntegrityChanged,
    CapacityPolicyChanged,
    SupportCellMissing,
    SupportCellChanged,
}

/// Stable, engine-neutral failure codes for runtime-profile operations.
///
/// These codes are safe to project across the desktop IPC and Agent tool boundary. They do not
/// contain endpoints, model identifiers, response bodies, commands, or credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeProfileFailureCode {
    InvalidRequest,
    PersistenceUnavailable,
    ManagedEngineUnavailable,
    BackendUnavailable,
    EngineClientUnavailable,
    EngineEndpointInvalid,
    EngineUnreachable,
    EngineResponseInvalid,
    EngineAdapterRegistryInvalid,
    EngineAdapterUnavailable,
    QualificationUnavailable,
    QualificationFailed,
    AcceptanceEvidenceUnavailable,
    ActionPlanUnavailable,
    NoVerifiedRuntime,
    DuplicateProfile,
    ProfileNotFound,
    ProfileNeedsRepair,
    ProfileChanged,
    LiveVerificationRequired,
    SupportCellSelectionRequired,
    InvalidSupportCell,
    RuntimeDeviceUnproven,
    ExternalProfileRequired,
    ActivationFailed,
    ActivationRecoveryRequired,
    InteractionIncomplete,
}

impl RuntimeProfileFailureCode {
    /// Stable snake-case code used by Pi and other bounded Agent tool clients.
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::InvalidRequest => "runtime_profile_invalid_request",
            Self::PersistenceUnavailable => "runtime_profile_persistence_unavailable",
            Self::ManagedEngineUnavailable => "runtime_profile_managed_engine_unavailable",
            Self::BackendUnavailable => "runtime_profile_backend_unavailable",
            Self::EngineClientUnavailable => "runtime_profile_engine_client_unavailable",
            Self::EngineEndpointInvalid => "runtime_profile_engine_endpoint_invalid",
            Self::EngineUnreachable => "runtime_profile_engine_unreachable",
            Self::EngineResponseInvalid => "runtime_profile_engine_response_invalid",
            Self::EngineAdapterRegistryInvalid => "runtime_profile_engine_adapter_registry_invalid",
            Self::EngineAdapterUnavailable => "runtime_profile_engine_adapter_unavailable",
            Self::QualificationUnavailable => "runtime_profile_qualification_unavailable",
            Self::QualificationFailed => "runtime_profile_qualification_failed",
            Self::AcceptanceEvidenceUnavailable => {
                "runtime_profile_acceptance_evidence_unavailable"
            }
            Self::ActionPlanUnavailable => "runtime_profile_action_plan_unavailable",
            Self::NoVerifiedRuntime => "runtime_profile_no_verified_runtime",
            Self::DuplicateProfile => "runtime_profile_duplicate",
            Self::ProfileNotFound => "runtime_profile_not_found",
            Self::ProfileNeedsRepair => "runtime_profile_needs_repair",
            Self::ProfileChanged => "runtime_profile_changed",
            Self::LiveVerificationRequired => "runtime_profile_live_verification_required",
            Self::SupportCellSelectionRequired => "runtime_profile_support_cell_selection_required",
            Self::InvalidSupportCell => "runtime_profile_invalid_support_cell",
            Self::RuntimeDeviceUnproven => "runtime_profile_runtime_device_unproven",
            Self::ExternalProfileRequired => "runtime_profile_external_profile_required",
            Self::ActivationFailed => "runtime_profile_activation_failed",
            Self::ActivationRecoveryRequired => "runtime_profile_activation_recovery_required",
            Self::InteractionIncomplete => "runtime_profile_interaction_incomplete",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeProfileFailureStage {
    Input,
    Persistence,
    Discovery,
    Inspection,
    Qualification,
    Evidence,
    Verification,
    Planning,
    Activation,
    Recovery,
    Interaction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeProfileRecoveryAction {
    CorrectInput,
    StartRuntime,
    CheckService,
    ReviewProfile,
    ReverifyProfile,
    SelectSupportCell,
    Retry,
    RecoverActivation,
    UpdateApplication,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeProfileFailure {
    pub code: RuntimeProfileFailureCode,
    pub stage: RuntimeProfileFailureStage,
    pub retryable: bool,
    pub recovery_action: RuntimeProfileRecoveryAction,
}

impl RuntimeProfileFailure {
    pub const fn new(
        code: RuntimeProfileFailureCode,
        stage: RuntimeProfileFailureStage,
        retryable: bool,
        recovery_action: RuntimeProfileRecoveryAction,
    ) -> Self {
        Self {
            code,
            stage,
            retryable,
            recovery_action,
        }
    }
}

/// Persisted identity of the exact engine support cell used by a runtime profile.
///
/// This is deliberately separate from the manifest's support status/evidence: a profile binds
/// the hardware/deployment coordinates, while the Rust-owned manifest remains the authority for
/// whether those coordinates are currently formal enough to execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeProfileSupportCell {
    pub platform: InferencePlatform,
    pub architecture: InferenceArchitecture,
    pub accelerator: InferenceAccelerator,
    pub deployment: InferenceDeployment,
}

impl RuntimeProfileSupportCell {
    pub fn matches_host(self, host: &HostCapabilitySnapshot) -> bool {
        self.platform == host.platform
            && self.architecture == host.architecture
            && host.accelerators.contains(&self.accelerator)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeProfileModelDigestKind {
    Sha256,
    OllamaDigest,
    EvidenceFingerprint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeProfileEvidenceKind {
    ContentDigest,
    RepositoryRevision,
    DeploymentFingerprint,
    CatalogIdentity,
}

/// Typed verification evidence for spec v3 profiles.
///
/// `algorithm` names the engine-specific verification procedure; `value` is interpreted only by
/// the registered Rust adapter. Neither field is an executable command or execution authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeProfileEvidence {
    pub kind: RuntimeProfileEvidenceKind,
    pub algorithm: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeProfileAdapterBinding {
    pub variant: String,
    pub contract_revision: String,
    pub backend_config_revision: Option<u64>,
    pub origin_fingerprint: Option<String>,
    pub protocol_capability_hash: Option<String>,
    #[serde(default)]
    pub support_cell: Option<RuntimeProfileSupportCell>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProfileDraft {
    pub name: String,
    pub description: String,
}

/// Bounded, non-secret performance facts for this exact saved engine/model/host identity.
///
/// Rust emits this only after matching a reviewed acceptance record by adapter, support cell,
/// origin/config revision, engine identity, typed model-evidence fingerprint and native device
/// class. It is advisory context and never grants activation authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeProfileReviewedPerformance {
    pub workload_revision: String,
    pub attempts: u16,
    pub concurrency: u8,
    pub p95_latency_ms: u64,
    pub max_latency_ms: u64,
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub wall_time_ms: u64,
    /// Aggregate completion throughput for the fixed sample, expressed as milli-token/s to avoid
    /// floating-point drift across Rust, JSON and TypeScript.
    pub sample_completion_tokens_per_second_milli: u64,
    pub reviewed_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalRuntimeProfileDraft {
    pub name: String,
    pub description: String,
    pub backend_id: String,
    pub model_id: String,
    pub expected_evidence: RuntimeProfileEvidence,
    /// Optional explicit support-cell selection. Rust validates every coordinate against the
    /// current host and manifest before accepting it; omission is allowed only when one formal
    /// support cell can be selected unambiguously.
    #[serde(default)]
    pub support_cell: Option<RuntimeProfileSupportCell>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProfileSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub spec_version: u16,
    pub ownership: InferenceEngineOwnership,
    pub backend_id: Option<String>,
    pub backend_api_root: Option<String>,
    pub model_id: String,
    pub model_display_name: String,
    pub model_digest_kind: RuntimeProfileModelDigestKind,
    pub engine: String,
    pub engine_version: String,
    pub capacity_tier: Option<String>,
    pub context_window_tokens: Option<u32>,
    pub capacity_revision: Option<String>,
    pub adapter_binding: RuntimeProfileAdapterBinding,
    pub evidence: RuntimeProfileEvidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_performance: Option<RuntimeProfileReviewedPerformance>,
    pub readiness: RuntimeProfileReadiness,
    pub issues: Vec<RuntimeProfileIssue>,
    pub verified_at_ms: i64,
    pub last_activated_at_ms: Option<i64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProfileCatalog {
    pub profiles: Vec<RuntimeProfileSummary>,
    pub active_profile_id: Option<String>,
    pub can_save_current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProfileActivationPlan {
    pub plan_id: String,
    pub expires_at_ms: i64,
    pub profile_id: String,
    pub profile_name: String,
    pub model_id: String,
    pub model_display_name: String,
    pub ownership: InferenceEngineOwnership,
    pub backend_id: Option<String>,
    pub engine: String,
    pub engine_version: String,
    #[serde(default)]
    pub support_cell: Option<RuntimeProfileSupportCell>,
    pub context_window_tokens: Option<u32>,
    pub current_backend_id: Option<String>,
    pub current_model_id: Option<String>,
    pub current_model_name: Option<String>,
    pub issues: Vec<RuntimeProfileIssue>,
    pub action_summary: String,
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProfileActivationResult {
    pub profile_id: String,
    pub ownership: InferenceEngineOwnership,
    pub active_backend_id: Option<String>,
    pub active_model_id: String,
    pub managed_runtime: Option<LlamaCppStatus>,
    pub catalog: RuntimeProfileCatalog,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_contract_contains_only_bounded_runtime_identity() {
        let draft = RuntimeProfileDraft {
            name: "代码助手".to_owned(),
            description: "本机验证通过".to_owned(),
        };
        let value = serde_json::to_value(draft).expect("profile draft JSON");

        assert_eq!(RUNTIME_PROFILE_SPEC_VERSION, 3);
        assert_eq!(value["name"], "代码助手");
        assert!(value.get("apiKey").is_none());
        assert!(value.get("command").is_none());
        assert!(value.get("path").is_none());
    }

    #[test]
    fn spec_v3_evidence_types_do_not_conflate_repository_or_deployment_identity_with_digest() {
        let repository = RuntimeProfileEvidence {
            kind: RuntimeProfileEvidenceKind::RepositoryRevision,
            algorithm: "git-commit".to_owned(),
            value: "0123456789abcdef".to_owned(),
        };
        let deployment = RuntimeProfileEvidence {
            kind: RuntimeProfileEvidenceKind::DeploymentFingerprint,
            algorithm: "container-image-digest".to_owned(),
            value: "sha256:abcdef".to_owned(),
        };
        let binding = RuntimeProfileAdapterBinding {
            variant: "official-openai-server".to_owned(),
            contract_revision: "engine-contract-v1".to_owned(),
            backend_config_revision: Some(7),
            origin_fingerprint: Some("a".repeat(64)),
            protocol_capability_hash: Some("b".repeat(64)),
            support_cell: Some(RuntimeProfileSupportCell {
                platform: InferencePlatform::Linux,
                architecture: InferenceArchitecture::X86_64,
                accelerator: InferenceAccelerator::Cuda,
                deployment: InferenceDeployment::Local,
            }),
        };

        assert_ne!(repository.kind, deployment.kind);
        let value =
            serde_json::to_value((repository, deployment, binding)).expect("spec v3 evidence JSON");
        assert_eq!(value[0]["kind"], "repositoryRevision");
        assert_eq!(value[1]["kind"], "deploymentFingerprint");
        assert!(value.to_string().find("command").is_none());
        assert!(value.to_string().find("credential").is_none());
    }

    #[test]
    fn runtime_profile_failure_contract_is_typed_bounded_and_stable() {
        let failure = RuntimeProfileFailure {
            code: RuntimeProfileFailureCode::RuntimeDeviceUnproven,
            stage: RuntimeProfileFailureStage::Qualification,
            retryable: false,
            recovery_action: RuntimeProfileRecoveryAction::SelectSupportCell,
        };
        let value = serde_json::to_value(failure).expect("failure JSON");

        assert_eq!(value["code"], "runtimeDeviceUnproven");
        assert_eq!(value["stage"], "qualification");
        assert_eq!(value["retryable"], false);
        assert_eq!(value["recoveryAction"], "selectSupportCell");
        assert_eq!(
            failure.code.as_code(),
            "runtime_profile_runtime_device_unproven"
        );
        assert!(value.get("message").is_none());
        assert!(value.to_string().find("endpoint").is_none());
    }
}
