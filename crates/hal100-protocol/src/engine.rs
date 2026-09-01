use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InferenceEngineKind {
    LlamaCpp,
    Ollama,
    MlxLm,
    Vllm,
    Sglang,
    TensorRtLlm,
    OpenVino,
    MlcLlm,
    LmDeploy,
}

impl InferenceEngineKind {
    pub const ALL: [Self; 9] = [
        Self::LlamaCpp,
        Self::Ollama,
        Self::MlxLm,
        Self::Vllm,
        Self::Sglang,
        Self::TensorRtLlm,
        Self::OpenVino,
        Self::MlcLlm,
        Self::LmDeploy,
    ];

    pub const fn storage_key(self) -> &'static str {
        match self {
            Self::LlamaCpp => "llama.cpp",
            Self::Ollama => "ollama",
            Self::MlxLm => "mlx-lm",
            Self::Vllm => "vllm",
            Self::Sglang => "sglang",
            Self::TensorRtLlm => "tensorrt-llm",
            Self::OpenVino => "openvino",
            Self::MlcLlm => "mlc-llm",
            Self::LmDeploy => "lmdeploy",
        }
    }

    pub fn from_storage_key(value: &str) -> Option<Self> {
        match value {
            "llama.cpp" => Some(Self::LlamaCpp),
            "ollama" => Some(Self::Ollama),
            "mlx-lm" => Some(Self::MlxLm),
            "vllm" => Some(Self::Vllm),
            "sglang" => Some(Self::Sglang),
            "tensorrt-llm" => Some(Self::TensorRtLlm),
            "openvino" => Some(Self::OpenVino),
            "mlc-llm" => Some(Self::MlcLlm),
            "lmdeploy" => Some(Self::LmDeploy),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InferenceEngineOwnership {
    Managed,
    External,
}

/// Stable identity of one HAL100 adapter implementation.
///
/// `engine` identifies the upstream runtime while `variant` distinguishes independently
/// qualified integrations such as an official external service and a HAL100-managed runtime.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EngineAdapterId {
    pub engine: InferenceEngineKind,
    pub variant: String,
    pub contract_revision: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InferenceEngineSupportStatus {
    Reserved,
    Connected,
    VerifiedExternal,
    Managed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InferenceDeployment {
    Local,
    Remote,
}

impl InferenceDeployment {
    pub const fn storage_key(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Remote => "remote",
        }
    }

    pub fn from_storage_key(value: &str) -> Option<Self> {
        match value {
            "local" => Some(Self::Local),
            "remote" => Some(Self::Remote),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InferenceProtocol {
    OpenAi,
    Anthropic,
    Ollama,
}

pub const ENGINE_PROTOCOL_CAPABILITY_REVISION: &str = "engine-protocol-capabilities-v1";

/// Stable revision of the typed engine-adapter identity contract.
///
/// This is deliberately owned by the protocol crate so manifests, saved runtime profiles,
/// acceptance artifacts and desktop DTOs cannot silently drift to different contract strings.
pub const ENGINE_ADAPTER_CONTRACT_REVISION: &str = "engine-contract-v1";

/// Individually qualified protocol behavior. "OpenAI compatible" is deliberately not a single
/// boolean because engines and models often implement different subsets of the API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EngineProtocolCapability {
    ModelsList,
    ChatCompletionsUnary,
    ChatCompletionsStream,
    Completions,
    ResponsesUnary,
    ResponsesStream,
    Embeddings,
    UsagePromptCompletion,
    UsageCachedTokens,
    ToolCallsSingle,
    ToolCallsParallel,
    StructuredOutput,
    VisionInput,
    AudioInput,
    RequestCancellation,
}

impl EngineProtocolCapability {
    pub const fn storage_key(self) -> &'static str {
        match self {
            Self::ModelsList => "models_list",
            Self::ChatCompletionsUnary => "chat_completions_unary",
            Self::ChatCompletionsStream => "chat_completions_stream",
            Self::Completions => "completions",
            Self::ResponsesUnary => "responses_unary",
            Self::ResponsesStream => "responses_stream",
            Self::Embeddings => "embeddings",
            Self::UsagePromptCompletion => "usage_prompt_completion",
            Self::UsageCachedTokens => "usage_cached_tokens",
            Self::ToolCallsSingle => "tool_calls_single",
            Self::ToolCallsParallel => "tool_calls_parallel",
            Self::StructuredOutput => "structured_output",
            Self::VisionInput => "vision_input",
            Self::AudioInput => "audio_input",
            Self::RequestCancellation => "request_cancellation",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EngineProtocolCapabilitySet {
    pub revision: String,
    pub capabilities: Vec<EngineProtocolCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EngineQualificationReport {
    pub adapter_id: EngineAdapterId,
    pub model_id: String,
    pub protocol_capabilities: EngineProtocolCapabilitySet,
    pub protocol_capability_hash: String,
    /// Exact upstream engine version observed by an active qualification request when the
    /// read-only discovery API does not expose one.
    pub observed_engine_version: Option<String>,
    /// Typed basis for binding this qualification to a runtime device. This deliberately keeps
    /// a service observation distinct from an adapter variant's fixed device contract. An
    /// unresolved report must never be upgraded to a device claim by the caller.
    pub runtime_device_evidence: EngineRuntimeDeviceEvidence,
    /// Optional deployment identity observed during the model-specific qualification request.
    /// This is intentionally distinct from a content digest: it may bind an engine fingerprint,
    /// model id and deployment configuration without claiming to hash model weights.
    #[serde(default)]
    pub deployment_fingerprint: Option<String>,
}

/// Device identity carried by a model-specific qualification request.
///
/// `ModelResidencyObservation` is the strongest form currently available: the service reports
/// where the model that served the protocol probe is resident. `AdapterVariantContract` is weaker
/// and may only be used by an adapter variant whose manifest declares exactly one accelerator.
/// `Unresolved` is explicit fail-closed evidence for services that expose neither property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "basis", rename_all = "camelCase", deny_unknown_fields)]
pub enum EngineRuntimeDeviceEvidence {
    ModelResidencyObservation { accelerator: InferenceAccelerator },
    AdapterVariantContract { accelerator: InferenceAccelerator },
    Unresolved,
}

impl EngineRuntimeDeviceEvidence {
    pub const fn accelerator(self) -> Option<InferenceAccelerator> {
        match self {
            Self::ModelResidencyObservation { accelerator }
            | Self::AdapterVariantContract { accelerator } => Some(accelerator),
            Self::Unresolved => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InferenceAccelerator {
    Cpu,
    Metal,
    Cuda,
    Rocm,
    Vulkan,
    Sycl,
    IntelGpu,
    IntelNpu,
}

impl InferenceAccelerator {
    pub const fn storage_key(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Metal => "metal",
            Self::Cuda => "cuda",
            Self::Rocm => "rocm",
            Self::Vulkan => "vulkan",
            Self::Sycl => "sycl",
            Self::IntelGpu => "intel_gpu",
            Self::IntelNpu => "intel_npu",
        }
    }

    pub fn from_storage_key(value: &str) -> Option<Self> {
        match value {
            "cpu" => Some(Self::Cpu),
            "metal" => Some(Self::Metal),
            "cuda" => Some(Self::Cuda),
            "rocm" => Some(Self::Rocm),
            "vulkan" => Some(Self::Vulkan),
            "sycl" => Some(Self::Sycl),
            "intel_gpu" => Some(Self::IntelGpu),
            "intel_npu" => Some(Self::IntelNpu),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InferencePlatform {
    MacOs,
    Windows,
    Linux,
}

impl InferencePlatform {
    pub const fn storage_key(self) -> &'static str {
        match self {
            Self::MacOs => "macos",
            Self::Windows => "windows",
            Self::Linux => "linux",
        }
    }

    pub fn from_storage_key(value: &str) -> Option<Self> {
        match value {
            "macos" => Some(Self::MacOs),
            "windows" => Some(Self::Windows),
            "linux" => Some(Self::Linux),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InferenceArchitecture {
    #[serde(rename = "aarch64")]
    Aarch64,
    #[serde(rename = "x86_64")]
    X86_64,
}

impl InferenceArchitecture {
    pub const fn storage_key(self) -> &'static str {
        match self {
            Self::Aarch64 => "aarch64",
            Self::X86_64 => "x86_64",
        }
    }

    pub fn from_storage_key(value: &str) -> Option<Self> {
        match value {
            "aarch64" => Some(Self::Aarch64),
            "x86_64" => Some(Self::X86_64),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InferenceModelFormat {
    Gguf,
    Safetensors,
    Mlx,
    Mlc,
    OpenVino,
}

/// Support is qualified per platform cell instead of being claimed for an engine globally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InferenceEngineSupportUnit {
    pub platform: InferencePlatform,
    pub architecture: InferenceArchitecture,
    pub accelerator: InferenceAccelerator,
    pub deployment: InferenceDeployment,
    pub status: InferenceEngineSupportStatus,
    /// Evidence progress declared for this exact platform support cell. Formal cells must carry
    /// the complete seven-part summary; weaker cells may omit it and are projected conservatively.
    #[serde(default)]
    pub evidence: Option<InferenceEngineSupportEvidenceSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceEngineDescriptor {
    pub kind: InferenceEngineKind,
    pub display_name: String,
    pub ownership: InferenceEngineOwnership,
    pub deployment: InferenceDeployment,
    pub protocols: Vec<InferenceProtocol>,
    pub platforms: Vec<InferencePlatform>,
    pub architectures: Vec<InferenceArchitecture>,
    pub accelerators: Vec<InferenceAccelerator>,
    pub model_formats: Vec<InferenceModelFormat>,
    pub managed_lifecycle: bool,
}

/// Static, Rust-owned declaration used to register an adapter and project the legacy descriptor.
/// Runtime observations and execution authority are deliberately not part of this manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InferenceEngineManifest {
    pub adapter_id: EngineAdapterId,
    pub descriptor: InferenceEngineDescriptor,
    pub support_units: Vec<InferenceEngineSupportUnit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostCapabilitySnapshot {
    pub platform: InferencePlatform,
    pub architecture: InferenceArchitecture,
    pub cpu_brand: String,
    pub device_model: String,
    pub total_memory_bytes: u64,
    pub physical_cpu_cores: u32,
    pub logical_cpu_cores: u32,
    pub accelerators: Vec<InferenceAccelerator>,
    pub model_storage_path: String,
    pub model_storage_available_bytes: u64,
    pub probe_revision: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EngineHostCompatibilityIssue {
    PlatformUnsupported,
    ArchitectureUnsupported,
    AcceleratorUnavailable,
    /// More than one host accelerator matched, but their support levels differ. Without an
    /// explicit accelerator selection, choosing the highest status would overclaim the weaker
    /// support cell.
    SupportCellAmbiguous,
    SupportNotFormal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineHostCompatibility {
    pub engine: InferenceEngineKind,
    pub compatible: bool,
    pub matched_accelerators: Vec<InferenceAccelerator>,
    pub support_status: Option<InferenceEngineSupportStatus>,
    /// Evidence for the exact matched support cell, when the manifest declares it.
    #[serde(default)]
    pub support_evidence: Option<InferenceEngineSupportEvidenceSummary>,
    pub issues: Vec<EngineHostCompatibilityIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceEngineCapability {
    pub descriptor: InferenceEngineDescriptor,
    pub compatibility: EngineHostCompatibility,
    #[serde(default)]
    pub external_runtimes: Vec<ExternalEngineSnapshot>,
    /// Bounded evidence progress for the current support cell. This is display/audit data only;
    /// activation still requires the exact runtime profile and target rechecks.
    #[serde(default)]
    pub support_evidence: Option<InferenceEngineSupportEvidenceSummary>,
    /// Deterministic Rust-side ranking for the current host and observed runtimes.
    ///
    /// This is advisory display data only; activation still requires the existing exact
    /// profile/evidence gates.
    #[serde(default)]
    pub recommendation: Option<InferenceEngineRecommendation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceEngineSupportEvidenceSummary {
    pub verified: Vec<InferenceEngineSupportEvidenceKind>,
    pub missing: Vec<InferenceEngineSupportEvidenceKind>,
}

impl InferenceEngineSupportEvidenceSummary {
    /// Return the conservative evidence contract associated with a support status. This is a
    /// schema-level baseline; a real-service acceptance record is still required before a cell
    /// may be promoted to a formal status.
    pub fn for_status(status: InferenceEngineSupportStatus) -> Self {
        const ALL: [InferenceEngineSupportEvidenceKind; 7] = [
            InferenceEngineSupportEvidenceKind::OfficialContract,
            InferenceEngineSupportEvidenceKind::ProtocolQualification,
            InferenceEngineSupportEvidenceKind::PlatformRuntime,
            InferenceEngineSupportEvidenceKind::EngineIdentity,
            InferenceEngineSupportEvidenceKind::ModelDeploymentIdentity,
            InferenceEngineSupportEvidenceKind::RuntimeProfileLifecycle,
            InferenceEngineSupportEvidenceKind::Stability,
        ];
        let verified_len = match status {
            InferenceEngineSupportStatus::Managed
            | InferenceEngineSupportStatus::VerifiedExternal => ALL.len(),
            InferenceEngineSupportStatus::Connected => 2,
            InferenceEngineSupportStatus::Reserved => 1,
        };
        Self {
            verified: ALL[..verified_len].to_vec(),
            missing: ALL[verified_len..].to_vec(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InferenceEngineSupportEvidenceKind {
    OfficialContract,
    ProtocolQualification,
    PlatformRuntime,
    EngineIdentity,
    ModelDeploymentIdentity,
    RuntimeProfileLifecycle,
    Stability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceEngineRecommendation {
    pub eligible: bool,
    pub score: u16,
    pub reasons: Vec<InferenceEngineRecommendationReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InferenceEngineRecommendationReason {
    HostCompatible,
    FormalSupport,
    ManagedLifecycle,
    VerifiedRuntimeObserved,
    ConnectedOnly,
    HostMismatch,
    SupportCellAmbiguous,
    ProtocolRequiresExplicitQualification,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceCapabilityCatalog {
    pub host: HostCapabilitySnapshot,
    pub engines: Vec<InferenceEngineCapability>,
    #[serde(default)]
    pub runtime_profile_candidates: Vec<ExternalRuntimeProfileCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalEngineModelSummary {
    pub name: String,
    pub digest: String,
    pub size_bytes: u64,
    pub format: String,
    pub family: Option<String>,
    pub parameter_size: Option<String>,
    pub quantization: Option<String>,
    pub evidence: crate::RuntimeProfileEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalEngineSnapshot {
    pub engine: InferenceEngineKind,
    pub display_name: String,
    pub api_root: String,
    pub version: String,
    /// False means the engine requires an active, model-specific qualification request before
    /// its exact version can be bound into an executable runtime profile.
    pub engine_version_exact: bool,
    pub models: Vec<ExternalEngineModelSummary>,
    pub model_catalog_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalRuntimeProfileCandidate {
    pub backend_id: String,
    pub backend_display_name: String,
    pub backend_api_root: String,
    pub engine: InferenceEngineKind,
    pub engine_version: String,
    pub model_id: String,
    pub model_digest: String,
    pub evidence: crate::RuntimeProfileEvidence,
    pub model_format: String,
    pub parameter_size: Option<String>,
    pub quantization: Option<String>,
    /// Formal support cells that can be explicitly selected before saving this candidate.
    #[serde(default)]
    pub support_cells: Vec<crate::RuntimeProfileSupportCell>,
}

impl InferenceEngineDescriptor {
    pub fn compatibility_with(&self, host: &HostCapabilitySnapshot) -> EngineHostCompatibility {
        let mut issues = Vec::new();
        if !self.platforms.contains(&host.platform) {
            issues.push(EngineHostCompatibilityIssue::PlatformUnsupported);
        }
        if !self.architectures.contains(&host.architecture) {
            issues.push(EngineHostCompatibilityIssue::ArchitectureUnsupported);
        }
        let matched_accelerators = self
            .accelerators
            .iter()
            .copied()
            .filter(|accelerator| host.accelerators.contains(accelerator))
            .collect::<Vec<_>>();
        if matched_accelerators.is_empty() {
            issues.push(EngineHostCompatibilityIssue::AcceleratorUnavailable);
        }
        EngineHostCompatibility {
            engine: self.kind,
            compatible: issues.is_empty(),
            matched_accelerators,
            support_status: None,
            support_evidence: None,
            issues,
        }
    }
}

impl InferenceEngineManifest {
    /// Resolves compatibility from an exact platform support cell.
    ///
    /// Descriptor-level overlap remains useful for planning, but only a `VerifiedExternal` or
    /// `Managed` support unit is formal enough to enable runtime use. `Reserved` and `Connected`
    /// cells stay visible while failing closed.
    pub fn compatibility_with(&self, host: &HostCapabilitySnapshot) -> EngineHostCompatibility {
        let platform_units = self
            .support_units
            .iter()
            .filter(|unit| {
                unit.platform == host.platform && unit.deployment == self.descriptor.deployment
            })
            .collect::<Vec<_>>();
        let architecture_units = platform_units
            .iter()
            .copied()
            .filter(|unit| unit.architecture == host.architecture)
            .collect::<Vec<_>>();
        let matched_units = architecture_units
            .iter()
            .copied()
            .filter(|unit| host.accelerators.contains(&unit.accelerator))
            .collect::<Vec<_>>();
        let mut matched_accelerators = matched_units
            .iter()
            .map(|unit| unit.accelerator)
            .collect::<Vec<_>>();
        matched_accelerators.sort_by_key(|accelerator| accelerator_rank(*accelerator));
        matched_accelerators.dedup();
        let support_status = matched_units
            .iter()
            .map(|unit| unit.status)
            .max_by_key(|status| support_status_rank(*status));
        let support_evidence = matched_units
            .iter()
            .max_by_key(|unit| {
                (
                    support_status_rank(unit.status),
                    accelerator_rank(unit.accelerator),
                )
            })
            .and_then(|unit| {
                unit.evidence.clone().or_else(|| {
                    Some(InferenceEngineSupportEvidenceSummary::for_status(
                        unit.status,
                    ))
                })
            });
        let mut issues = Vec::new();
        if platform_units.is_empty() {
            issues.push(EngineHostCompatibilityIssue::PlatformUnsupported);
        } else if architecture_units.is_empty() {
            issues.push(EngineHostCompatibilityIssue::ArchitectureUnsupported);
        } else if matched_units.is_empty() {
            issues.push(EngineHostCompatibilityIssue::AcceleratorUnavailable);
        }
        if support_status.is_some_and(|status| {
            !matches!(
                status,
                InferenceEngineSupportStatus::VerifiedExternal
                    | InferenceEngineSupportStatus::Managed
            )
        }) {
            issues.push(EngineHostCompatibilityIssue::SupportNotFormal);
        }
        let has_formal_match = matched_units.iter().any(|unit| {
            matches!(
                unit.status,
                InferenceEngineSupportStatus::VerifiedExternal
                    | InferenceEngineSupportStatus::Managed
            )
        });
        let has_non_formal_match = matched_units.iter().any(|unit| {
            matches!(
                unit.status,
                InferenceEngineSupportStatus::Reserved | InferenceEngineSupportStatus::Connected
            )
        });
        if has_formal_match && has_non_formal_match {
            issues.push(EngineHostCompatibilityIssue::SupportCellAmbiguous);
        }
        EngineHostCompatibility {
            engine: self.adapter_id.engine,
            compatible: issues.is_empty() && support_status.is_some(),
            matched_accelerators,
            support_status,
            support_evidence,
            issues,
        }
    }
}

const fn support_status_rank(status: InferenceEngineSupportStatus) -> u8 {
    match status {
        InferenceEngineSupportStatus::Reserved => 0,
        InferenceEngineSupportStatus::Connected => 1,
        InferenceEngineSupportStatus::VerifiedExternal => 2,
        InferenceEngineSupportStatus::Managed => 3,
    }
}

const fn accelerator_rank(accelerator: InferenceAccelerator) -> u8 {
    match accelerator {
        InferenceAccelerator::Cpu => 0,
        InferenceAccelerator::Metal => 1,
        InferenceAccelerator::Cuda => 2,
        InferenceAccelerator::Rocm => 3,
        InferenceAccelerator::Vulkan => 4,
        InferenceAccelerator::Sycl => 5,
        InferenceAccelerator::IntelGpu => 6,
        InferenceAccelerator::IntelNpu => 7,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EngineInstallState {
    NotInstalled,
    Installed,
    VerificationFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EngineRuntimeState {
    Stopped,
    Starting,
    Running,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedEngineStatus {
    pub version: String,
    pub install_state: EngineInstallState,
    pub runtime_state: EngineRuntimeState,
    pub active_model_id: Option<String>,
    pub active_model_name: Option<String>,
    pub port: Option<u16>,
    pub last_error_code: Option<String>,
}

/// Backwards-compatible name retained for the existing desktop command surface.
/// New engine-neutral infrastructure should use `ManagedEngineStatus`.
pub type LlamaCppStatus = ManagedEngineStatus;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInstallPlan {
    pub plan_id: String,
    pub expires_at_ms: i64,
    pub engine: String,
    pub version: String,
    pub archive_size_bytes: u64,
    pub publisher: String,
    pub action_summary: String,
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineRemovePlan {
    pub plan_id: String,
    pub expires_at_ms: i64,
    pub engine: String,
    pub version: String,
    pub install_path: String,
    pub action_summary: String,
    pub requires_confirmation: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_identity_storage_keys_are_stable_and_allowlisted() {
        for kind in InferenceEngineKind::ALL {
            assert_eq!(
                InferenceEngineKind::from_storage_key(kind.storage_key()),
                Some(kind)
            );
        }
        assert_eq!(
            InferenceEngineKind::from_storage_key("arbitrary-shell-engine"),
            None
        );
    }

    #[test]
    fn descriptor_serializes_engine_protocol_ownership_and_hardware_separately() {
        let descriptor = InferenceEngineDescriptor {
            kind: InferenceEngineKind::LlamaCpp,
            display_name: "HAL100 托管 llama.cpp".to_owned(),
            ownership: InferenceEngineOwnership::Managed,
            deployment: InferenceDeployment::Local,
            protocols: vec![InferenceProtocol::OpenAi],
            platforms: vec![InferencePlatform::MacOs],
            architectures: vec![InferenceArchitecture::Aarch64],
            accelerators: vec![InferenceAccelerator::Metal],
            model_formats: vec![InferenceModelFormat::Gguf],
            managed_lifecycle: true,
        };
        let value = serde_json::to_value(descriptor).expect("engine descriptor JSON");

        assert_eq!(value["kind"], "llamaCpp");
        assert_eq!(value["ownership"], "managed");
        assert_eq!(value["deployment"], "local");
        assert_eq!(value["protocols"][0], "openAi");
        assert_eq!(value["platforms"][0], "macOs");
        assert_eq!(value["architectures"][0], "aarch64");
        assert_eq!(value["accelerators"][0], "metal");
        assert_eq!(value["modelFormats"][0], "gguf");
    }

    #[test]
    fn manifest_separates_adapter_identity_from_platform_support_status() {
        let descriptor = InferenceEngineDescriptor {
            kind: InferenceEngineKind::Ollama,
            display_name: "用户所有的本机 Ollama".to_owned(),
            ownership: InferenceEngineOwnership::External,
            deployment: InferenceDeployment::Local,
            protocols: vec![InferenceProtocol::OpenAi, InferenceProtocol::Ollama],
            platforms: vec![InferencePlatform::MacOs, InferencePlatform::Windows],
            architectures: vec![
                InferenceArchitecture::Aarch64,
                InferenceArchitecture::X86_64,
            ],
            accelerators: vec![InferenceAccelerator::Cpu, InferenceAccelerator::Metal],
            model_formats: vec![InferenceModelFormat::Gguf],
            managed_lifecycle: false,
        };
        let manifest = InferenceEngineManifest {
            adapter_id: EngineAdapterId {
                engine: InferenceEngineKind::Ollama,
                variant: "official-loopback-api".to_owned(),
                contract_revision: "engine-contract-v1".to_owned(),
            },
            descriptor,
            support_units: vec![InferenceEngineSupportUnit {
                platform: InferencePlatform::MacOs,
                architecture: InferenceArchitecture::Aarch64,
                accelerator: InferenceAccelerator::Metal,
                deployment: InferenceDeployment::Local,
                status: InferenceEngineSupportStatus::VerifiedExternal,
                evidence: Some(InferenceEngineSupportEvidenceSummary::for_status(
                    InferenceEngineSupportStatus::VerifiedExternal,
                )),
            }],
        };
        let value = serde_json::to_value(manifest).expect("engine manifest JSON");

        assert_eq!(value["adapterId"]["engine"], "ollama");
        assert_eq!(value["adapterId"]["variant"], "official-loopback-api");
        assert_eq!(value["supportUnits"][0]["status"], "verifiedExternal");
        assert!(value.get("apiRoot").is_none());
        assert!(value.get("credential").is_none());
    }

    #[test]
    fn compatibility_requires_platform_architecture_and_accelerator_intersection() {
        let descriptor = InferenceEngineDescriptor {
            kind: InferenceEngineKind::LlamaCpp,
            display_name: "HAL100 托管 llama.cpp".to_owned(),
            ownership: InferenceEngineOwnership::Managed,
            deployment: InferenceDeployment::Local,
            protocols: vec![InferenceProtocol::OpenAi],
            platforms: vec![InferencePlatform::MacOs],
            architectures: vec![InferenceArchitecture::Aarch64],
            accelerators: vec![InferenceAccelerator::Metal],
            model_formats: vec![InferenceModelFormat::Gguf],
            managed_lifecycle: true,
        };
        let supported = HostCapabilitySnapshot {
            platform: InferencePlatform::MacOs,
            architecture: InferenceArchitecture::Aarch64,
            cpu_brand: "Apple M1".to_owned(),
            device_model: "MacBookPro17,1".to_owned(),
            total_memory_bytes: 16 * 1024 * 1024 * 1024,
            physical_cpu_cores: 8,
            logical_cpu_cores: 8,
            accelerators: vec![InferenceAccelerator::Cpu, InferenceAccelerator::Metal],
            model_storage_path: "/models".to_owned(),
            model_storage_available_bytes: 100,
            probe_revision: "host-capabilities-v1".to_owned(),
        };
        let compatible = descriptor.compatibility_with(&supported);
        assert!(compatible.compatible);
        assert_eq!(
            compatible.matched_accelerators,
            vec![InferenceAccelerator::Metal]
        );

        let unsupported = HostCapabilitySnapshot {
            platform: InferencePlatform::Windows,
            architecture: InferenceArchitecture::X86_64,
            accelerators: vec![InferenceAccelerator::Cpu],
            ..supported
        };
        let incompatible = descriptor.compatibility_with(&unsupported);
        assert!(!incompatible.compatible);
        assert_eq!(
            incompatible.issues,
            vec![
                EngineHostCompatibilityIssue::PlatformUnsupported,
                EngineHostCompatibilityIssue::ArchitectureUnsupported,
                EngineHostCompatibilityIssue::AcceleratorUnavailable,
            ]
        );
    }

    #[test]
    fn manifest_compatibility_refuses_reserved_cells_even_when_descriptor_shapes_overlap() {
        let descriptor = InferenceEngineDescriptor {
            kind: InferenceEngineKind::Ollama,
            display_name: "Ollama".to_owned(),
            ownership: InferenceEngineOwnership::External,
            deployment: InferenceDeployment::Local,
            protocols: vec![InferenceProtocol::OpenAi],
            platforms: vec![InferencePlatform::MacOs, InferencePlatform::Windows],
            architectures: vec![
                InferenceArchitecture::Aarch64,
                InferenceArchitecture::X86_64,
            ],
            accelerators: vec![InferenceAccelerator::Cpu],
            model_formats: vec![InferenceModelFormat::Gguf],
            managed_lifecycle: false,
        };
        let manifest = InferenceEngineManifest {
            adapter_id: EngineAdapterId {
                engine: InferenceEngineKind::Ollama,
                variant: "official-loopback-api".to_owned(),
                contract_revision: "engine-contract-v1".to_owned(),
            },
            descriptor,
            support_units: vec![
                InferenceEngineSupportUnit {
                    platform: InferencePlatform::MacOs,
                    architecture: InferenceArchitecture::Aarch64,
                    accelerator: InferenceAccelerator::Cpu,
                    deployment: InferenceDeployment::Local,
                    status: InferenceEngineSupportStatus::VerifiedExternal,
                    evidence: Some(InferenceEngineSupportEvidenceSummary::for_status(
                        InferenceEngineSupportStatus::VerifiedExternal,
                    )),
                },
                InferenceEngineSupportUnit {
                    platform: InferencePlatform::Windows,
                    architecture: InferenceArchitecture::X86_64,
                    accelerator: InferenceAccelerator::Cpu,
                    deployment: InferenceDeployment::Local,
                    status: InferenceEngineSupportStatus::Reserved,
                    evidence: None,
                },
            ],
        };
        let base = HostCapabilitySnapshot {
            platform: InferencePlatform::MacOs,
            architecture: InferenceArchitecture::Aarch64,
            cpu_brand: "fixture".to_owned(),
            device_model: "fixture".to_owned(),
            total_memory_bytes: 16 * 1024 * 1024 * 1024,
            physical_cpu_cores: 8,
            logical_cpu_cores: 8,
            accelerators: vec![InferenceAccelerator::Cpu],
            model_storage_path: "/models".to_owned(),
            model_storage_available_bytes: 100,
            probe_revision: "host-capabilities-v2".to_owned(),
        };
        let verified = manifest.compatibility_with(&base);
        assert!(verified.compatible);
        assert_eq!(
            verified.support_status,
            Some(InferenceEngineSupportStatus::VerifiedExternal)
        );
        assert_eq!(
            verified
                .support_evidence
                .as_ref()
                .map(|evidence| evidence.verified.len()),
            Some(7)
        );

        let reserved = manifest.compatibility_with(&HostCapabilitySnapshot {
            platform: InferencePlatform::Windows,
            architecture: InferenceArchitecture::X86_64,
            ..base
        });
        assert!(!reserved.compatible);
        assert_eq!(
            reserved.support_status,
            Some(InferenceEngineSupportStatus::Reserved)
        );
        assert_eq!(
            reserved
                .support_evidence
                .as_ref()
                .map(|evidence| evidence.verified.len()),
            Some(1)
        );
        assert_eq!(
            reserved.issues,
            vec![EngineHostCompatibilityIssue::SupportNotFormal]
        );
    }

    #[test]
    fn manifest_compatibility_refuses_mixed_support_cells_without_explicit_accelerator() {
        let manifest = InferenceEngineManifest {
            adapter_id: EngineAdapterId {
                engine: InferenceEngineKind::OpenVino,
                variant: "ovms-openai-server".to_owned(),
                contract_revision: "engine-contract-v1".to_owned(),
            },
            descriptor: InferenceEngineDescriptor {
                kind: InferenceEngineKind::OpenVino,
                display_name: "OpenVINO Model Server".to_owned(),
                ownership: InferenceEngineOwnership::External,
                deployment: InferenceDeployment::Local,
                protocols: vec![InferenceProtocol::OpenAi],
                platforms: vec![InferencePlatform::Linux],
                architectures: vec![InferenceArchitecture::X86_64],
                accelerators: vec![InferenceAccelerator::Cpu, InferenceAccelerator::IntelGpu],
                model_formats: vec![InferenceModelFormat::OpenVino],
                managed_lifecycle: false,
            },
            support_units: vec![
                InferenceEngineSupportUnit {
                    platform: InferencePlatform::Linux,
                    architecture: InferenceArchitecture::X86_64,
                    accelerator: InferenceAccelerator::Cpu,
                    deployment: InferenceDeployment::Local,
                    status: InferenceEngineSupportStatus::VerifiedExternal,
                    evidence: Some(InferenceEngineSupportEvidenceSummary::for_status(
                        InferenceEngineSupportStatus::VerifiedExternal,
                    )),
                },
                InferenceEngineSupportUnit {
                    platform: InferencePlatform::Linux,
                    architecture: InferenceArchitecture::X86_64,
                    accelerator: InferenceAccelerator::IntelGpu,
                    deployment: InferenceDeployment::Local,
                    status: InferenceEngineSupportStatus::Connected,
                    evidence: None,
                },
            ],
        };
        let host = HostCapabilitySnapshot {
            platform: InferencePlatform::Linux,
            architecture: InferenceArchitecture::X86_64,
            cpu_brand: "Intel".to_owned(),
            device_model: "fixture".to_owned(),
            total_memory_bytes: 32 * 1024 * 1024 * 1024,
            physical_cpu_cores: 8,
            logical_cpu_cores: 16,
            accelerators: vec![InferenceAccelerator::Cpu, InferenceAccelerator::IntelGpu],
            model_storage_path: "/models".to_owned(),
            model_storage_available_bytes: 100,
            probe_revision: "host-capabilities-v2".to_owned(),
        };

        let compatibility = manifest.compatibility_with(&host);
        assert!(!compatibility.compatible);
        assert_eq!(
            compatibility.support_status,
            Some(InferenceEngineSupportStatus::VerifiedExternal)
        );
        assert_eq!(
            compatibility.issues,
            vec![EngineHostCompatibilityIssue::SupportCellAmbiguous]
        );
    }

    #[test]
    fn mixed_support_issue_has_a_stable_wire_name() {
        let value = serde_json::to_value(EngineHostCompatibilityIssue::SupportCellAmbiguous)
            .expect("compatibility issue JSON");
        assert_eq!(value, serde_json::json!("supportCellAmbiguous"));
        let reason =
            serde_json::to_value(InferenceEngineRecommendationReason::SupportCellAmbiguous)
                .expect("recommendation reason JSON");
        assert_eq!(reason, serde_json::json!("supportCellAmbiguous"));
    }

    #[test]
    fn runtime_device_evidence_keeps_observation_contract_and_unknown_distinct() {
        let observed =
            serde_json::to_value(EngineRuntimeDeviceEvidence::ModelResidencyObservation {
                accelerator: InferenceAccelerator::Metal,
            })
            .expect("runtime device observation JSON");
        assert_eq!(
            observed,
            serde_json::json!({
                "basis": "modelResidencyObservation",
                "accelerator": "metal"
            })
        );
        let contract = serde_json::to_value(EngineRuntimeDeviceEvidence::AdapterVariantContract {
            accelerator: InferenceAccelerator::Cuda,
        })
        .expect("runtime device contract JSON");
        assert_eq!(
            contract,
            serde_json::json!({
                "basis": "adapterVariantContract",
                "accelerator": "cuda"
            })
        );
        assert_eq!(EngineRuntimeDeviceEvidence::Unresolved.accelerator(), None);
        assert!(
            serde_json::from_value::<EngineRuntimeDeviceEvidence>(
                serde_json::json!({"basis": "hostGuess", "accelerator": "cpu"})
            )
            .is_err()
        );
    }

    #[test]
    fn external_snapshot_contains_stable_identity_without_lifecycle_authority() {
        let snapshot = ExternalEngineSnapshot {
            engine: InferenceEngineKind::Ollama,
            display_name: "本机 Ollama".to_owned(),
            api_root: "http://127.0.0.1:11434/v1/".to_owned(),
            version: "0.12.6".to_owned(),
            engine_version_exact: true,
            models: vec![ExternalEngineModelSummary {
                name: "qwen3:8b".to_owned(),
                digest: "a".repeat(64),
                size_bytes: 4_000_000_000,
                format: "gguf".to_owned(),
                family: Some("qwen3".to_owned()),
                parameter_size: Some("8.2B".to_owned()),
                quantization: Some("Q4_K_M".to_owned()),
                evidence: crate::RuntimeProfileEvidence {
                    kind: crate::RuntimeProfileEvidenceKind::ContentDigest,
                    algorithm: "ollama-digest".to_owned(),
                    value: "a".repeat(64),
                },
            }],
            model_catalog_complete: true,
        };
        let value = serde_json::to_value(snapshot).expect("external engine snapshot JSON");
        assert_eq!(value["engine"], "ollama");
        assert_eq!(
            value["models"][0]["digest"].as_str().map(str::len),
            Some(64)
        );
        assert!(value.get("command").is_none());
        assert!(value.get("credential").is_none());
    }

    #[test]
    fn external_runtime_profile_candidate_contains_identity_but_not_execution_authority() {
        let candidate = ExternalRuntimeProfileCandidate {
            backend_id: "backend-ollama".to_owned(),
            backend_display_name: "本机 Ollama".to_owned(),
            backend_api_root: "http://127.0.0.1:11434/v1/".to_owned(),
            engine: InferenceEngineKind::Ollama,
            engine_version: "0.12.6".to_owned(),
            model_id: "qwen3:8b".to_owned(),
            model_digest: "a".repeat(64),
            evidence: crate::RuntimeProfileEvidence {
                kind: crate::RuntimeProfileEvidenceKind::ContentDigest,
                algorithm: "ollama-digest".to_owned(),
                value: "a".repeat(64),
            },
            model_format: "gguf".to_owned(),
            parameter_size: Some("8.2B".to_owned()),
            quantization: Some("Q4_K_M".to_owned()),
            support_cells: vec![crate::RuntimeProfileSupportCell {
                platform: InferencePlatform::MacOs,
                architecture: InferenceArchitecture::Aarch64,
                accelerator: InferenceAccelerator::Metal,
                deployment: InferenceDeployment::Local,
            }],
        };
        let value = serde_json::to_value(candidate).expect("external profile candidate JSON");
        assert_eq!(value["backendId"], "backend-ollama");
        assert_eq!(value["modelDigest"].as_str().map(str::len), Some(64));
        assert_eq!(value["backendApiRoot"], "http://127.0.0.1:11434/v1/");
        assert!(value.get("command").is_none());
        assert!(value.get("credential").is_none());
    }
}
