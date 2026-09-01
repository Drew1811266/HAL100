use std::{
    collections::HashMap,
    fmt::Write as _,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use hal100_protocol::{
    ENGINE_ADAPTER_CONTRACT_REVISION, EngineAdapterId, EngineInstallState,
    EngineRuntimeDeviceEvidence, EngineRuntimeState, ExternalEngineModelSummary,
    ExternalEngineSnapshot, ExternalRuntimeProfileDraft, HostCapabilitySnapshot,
    InferenceEngineCapability, InferenceEngineKind, InferenceEngineManifest,
    InferenceEngineOwnership, InferenceEngineSupportStatus, LocalModelState,
    RUNTIME_PROFILE_SPEC_VERSION, RuntimeProfileActivationPlan, RuntimeProfileActivationResult,
    RuntimeProfileAdapterBinding, RuntimeProfileCatalog, RuntimeProfileDraft,
    RuntimeProfileEvidence, RuntimeProfileEvidenceKind, RuntimeProfileFailure,
    RuntimeProfileFailureCode, RuntimeProfileFailureStage, RuntimeProfileIssue,
    RuntimeProfileModelDigestKind, RuntimeProfileReadiness, RuntimeProfileRecoveryAction,
    RuntimeProfileSummary, RuntimeProfileSupportCell,
};
use thiserror::Error;
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use crate::{
    BackendManager, BackendManagerError, Database, DatabaseError, EngineManagerError,
    EngineObservationService, EngineTargetKey, ExternalEngineAdapterError,
    ExternalInferenceEngineRegistry, GatewayState, InferenceEngineAdapter,
    InferenceEngineManifestRegistry, PendingPlanError, PendingPlanStore,
    RuntimeActivationJournalRepository, RuntimeActivationPhase, RuntimeProfileRepository,
    StoredActiveGatewayRoute, StoredRuntimeActivationJournal, StoredRuntimeProfileRecord,
    StoredRuntimeProfileVerification, VerifiedEngineTarget,
};

const ACTIVATION_PLAN_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_NAME_CHARS: usize = 80;
const MAX_DESCRIPTION_CHARS: usize = 500;
const MAX_EXTERNAL_ENGINE_TARGETS_PER_ADAPTER: usize = 64;
/// Explicit storage marker for adapters whose service does not expose a package version.
///
/// This is not a version and may only be persisted when a separate deployment identity (for
/// example a model-bound fingerprint) has been qualified.
pub const ENGINE_VERSION_NOT_EXPOSED: &str = "qualification-required";
const OPENAI_CORE_CAPABILITY_HASH: &str =
    "1b3e385cbb7f30878cba8eaccf7d5f5e6e1f18b2861a44bc79b18d963cbdd258";

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingActivation {
    profile_id: String,
    expected_updated_at_ms: i64,
    model_id: String,
    ownership: InferenceEngineOwnership,
    backend_id: Option<String>,
    observed_engine_version: String,
    observed_model_digest: String,
    observed_evidence: RuntimeProfileEvidence,
    authority: ActivationAuthorityBinding,
    expected_route: Option<StoredActiveGatewayRoute>,
    requires_confirmation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExternalProtocolQualification {
    capability_hash: String,
    observed_engine_version: Option<String>,
    deployment_fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActivationAuthorityBinding {
    adapter_id: EngineAdapterId,
    backend_config_revision: Option<u64>,
    origin_fingerprint: Option<String>,
    evidence_kind: String,
    evidence_algorithm: String,
    evidence_value: String,
    protocol_capability_hash: String,
    support_cell: Option<RuntimeProfileSupportCell>,
}

impl ActivationAuthorityBinding {
    fn from_profile(
        profile: &StoredRuntimeProfileRecord,
    ) -> Result<Self, RuntimeProfileManagerError> {
        let engine = InferenceEngineKind::from_storage_key(&profile.engine)
            .ok_or(RuntimeProfileManagerError::ProfileNeedsRepair)?;
        Ok(Self {
            adapter_id: EngineAdapterId {
                engine,
                variant: profile.adapter_variant.clone(),
                contract_revision: profile.adapter_contract_revision.clone(),
            },
            backend_config_revision: profile.backend_config_revision,
            origin_fingerprint: profile.origin_fingerprint.clone(),
            evidence_kind: profile.evidence_kind.clone(),
            evidence_algorithm: profile.evidence_algorithm.clone(),
            evidence_value: profile.evidence_value.clone(),
            protocol_capability_hash: profile.protocol_capability_hash.clone(),
            support_cell: profile.support_cell,
        })
    }
}

#[derive(Debug, Error)]
pub enum RuntimeProfileManagerError {
    #[error(transparent)]
    Database(#[from] DatabaseError),
    #[error(transparent)]
    Engine(#[from] EngineManagerError),
    #[error(transparent)]
    Backend(#[from] BackendManagerError),
    #[error(transparent)]
    ExternalEngine(#[from] ExternalEngineAdapterError),
    #[error(transparent)]
    PendingPlan(#[from] PendingPlanError),
    #[error("方案名称无效")]
    InvalidName,
    #[error("方案说明超出长度限制")]
    InvalidDescription,
    #[error("外部运行方案候选身份无效")]
    InvalidExternalDraft,
    #[error("当前没有正在运行且可验证的本地模型")]
    NoVerifiedRuntime,
    #[error("当前模型与引擎组合已经保存为运行方案")]
    DuplicateProfile,
    #[error("运行方案不存在")]
    ProfileNotFound,
    #[error("运行方案引用的模型或引擎当前不可用")]
    ProfileNeedsRepair,
    #[error("运行方案保存的引擎或模型身份已发生变化，请重新验证并保存方案")]
    ProfileChanged,
    #[error("外部运行方案必须经过实时引擎复检")]
    ExternalVerificationRequired,
    #[error("当前宿主匹配多个正式支持格，请明确选择平台、架构、加速器与部署")]
    SupportCellSelectionRequired,
    #[error("运行方案选择的支持格与当前宿主或引擎清单不匹配")]
    InvalidSupportCell,
    #[error("实时资格检查无法证明引擎正在使用运行方案选择的加速器")]
    SupportCellNotProven,
    #[error("只有外部运行方案可以重新验证外部身份快照")]
    ExternalProfileRequired,
    #[error("运行方案切换失败；原运行模型恢复状态：{rollback_restored}")]
    ActivationFailed { rollback_restored: bool },
    #[error("存在未完成的运行方案切换，必须先恢复到已知状态")]
    ActivationRecoveryRequired,
}

impl RuntimeProfileManagerError {
    /// Projects internal errors into a bounded cross-engine contract suitable for desktop and Pi.
    /// The result intentionally excludes backend response bodies and runtime identity details.
    pub const fn failure(&self) -> RuntimeProfileFailure {
        use RuntimeProfileFailureCode as Code;
        use RuntimeProfileFailureStage as Stage;
        use RuntimeProfileRecoveryAction as Recovery;

        match self {
            Self::Database(_) => RuntimeProfileFailure::new(
                Code::PersistenceUnavailable,
                Stage::Persistence,
                true,
                Recovery::Retry,
            ),
            Self::Engine(_) => RuntimeProfileFailure::new(
                Code::ManagedEngineUnavailable,
                Stage::Verification,
                true,
                Recovery::StartRuntime,
            ),
            Self::Backend(_) => RuntimeProfileFailure::new(
                Code::BackendUnavailable,
                Stage::Inspection,
                true,
                Recovery::CheckService,
            ),
            Self::ExternalEngine(error) => external_engine_failure(error),
            Self::PendingPlan(_) => RuntimeProfileFailure::new(
                Code::ActionPlanUnavailable,
                Stage::Planning,
                true,
                Recovery::Retry,
            ),
            Self::InvalidName | Self::InvalidDescription | Self::InvalidExternalDraft => {
                RuntimeProfileFailure::new(
                    Code::InvalidRequest,
                    Stage::Input,
                    false,
                    Recovery::CorrectInput,
                )
            }
            Self::NoVerifiedRuntime => RuntimeProfileFailure::new(
                Code::NoVerifiedRuntime,
                Stage::Verification,
                true,
                Recovery::StartRuntime,
            ),
            Self::DuplicateProfile => RuntimeProfileFailure::new(
                Code::DuplicateProfile,
                Stage::Input,
                false,
                Recovery::ReviewProfile,
            ),
            Self::ProfileNotFound => RuntimeProfileFailure::new(
                Code::ProfileNotFound,
                Stage::Verification,
                false,
                Recovery::ReviewProfile,
            ),
            Self::ProfileNeedsRepair => RuntimeProfileFailure::new(
                Code::ProfileNeedsRepair,
                Stage::Verification,
                false,
                Recovery::ReviewProfile,
            ),
            Self::ProfileChanged => RuntimeProfileFailure::new(
                Code::ProfileChanged,
                Stage::Verification,
                false,
                Recovery::ReverifyProfile,
            ),
            Self::ExternalVerificationRequired => RuntimeProfileFailure::new(
                Code::LiveVerificationRequired,
                Stage::Verification,
                true,
                Recovery::ReverifyProfile,
            ),
            Self::SupportCellSelectionRequired => RuntimeProfileFailure::new(
                Code::SupportCellSelectionRequired,
                Stage::Input,
                false,
                Recovery::SelectSupportCell,
            ),
            Self::InvalidSupportCell => RuntimeProfileFailure::new(
                Code::InvalidSupportCell,
                Stage::Verification,
                false,
                Recovery::SelectSupportCell,
            ),
            Self::SupportCellNotProven => RuntimeProfileFailure::new(
                Code::RuntimeDeviceUnproven,
                Stage::Qualification,
                false,
                Recovery::SelectSupportCell,
            ),
            Self::ExternalProfileRequired => RuntimeProfileFailure::new(
                Code::ExternalProfileRequired,
                Stage::Input,
                false,
                Recovery::ReviewProfile,
            ),
            Self::ActivationFailed { rollback_restored } => RuntimeProfileFailure::new(
                Code::ActivationFailed,
                Stage::Activation,
                *rollback_restored,
                if *rollback_restored {
                    Recovery::Retry
                } else {
                    Recovery::RecoverActivation
                },
            ),
            Self::ActivationRecoveryRequired => RuntimeProfileFailure::new(
                Code::ActivationRecoveryRequired,
                Stage::Recovery,
                false,
                Recovery::RecoverActivation,
            ),
        }
    }
}

const fn external_engine_failure(error: &ExternalEngineAdapterError) -> RuntimeProfileFailure {
    use RuntimeProfileFailureCode as Code;
    use RuntimeProfileFailureStage as Stage;
    use RuntimeProfileRecoveryAction as Recovery;

    match error {
        ExternalEngineAdapterError::Client => RuntimeProfileFailure::new(
            Code::EngineClientUnavailable,
            Stage::Inspection,
            true,
            Recovery::Retry,
        ),
        ExternalEngineAdapterError::InvalidEndpoint => RuntimeProfileFailure::new(
            Code::EngineEndpointInvalid,
            Stage::Input,
            false,
            Recovery::CorrectInput,
        ),
        ExternalEngineAdapterError::Unreachable => RuntimeProfileFailure::new(
            Code::EngineUnreachable,
            Stage::Inspection,
            true,
            Recovery::CheckService,
        ),
        ExternalEngineAdapterError::InvalidResponse => RuntimeProfileFailure::new(
            Code::EngineResponseInvalid,
            Stage::Inspection,
            false,
            Recovery::CheckService,
        ),
        ExternalEngineAdapterError::InvalidAdapterRegistry => RuntimeProfileFailure::new(
            Code::EngineAdapterRegistryInvalid,
            Stage::Discovery,
            false,
            Recovery::UpdateApplication,
        ),
        ExternalEngineAdapterError::AdapterUnavailable => RuntimeProfileFailure::new(
            Code::EngineAdapterUnavailable,
            Stage::Discovery,
            false,
            Recovery::ReviewProfile,
        ),
        ExternalEngineAdapterError::QualificationUnavailable => RuntimeProfileFailure::new(
            Code::QualificationUnavailable,
            Stage::Qualification,
            false,
            Recovery::UpdateApplication,
        ),
        ExternalEngineAdapterError::QualificationFailed => RuntimeProfileFailure::new(
            Code::QualificationFailed,
            Stage::Qualification,
            false,
            Recovery::CheckService,
        ),
        ExternalEngineAdapterError::AcceptanceEvidenceUnavailable => RuntimeProfileFailure::new(
            Code::AcceptanceEvidenceUnavailable,
            Stage::Evidence,
            false,
            Recovery::UpdateApplication,
        ),
    }
}

pub struct RuntimeProfileManager {
    database: Arc<Database>,
    profiles: Arc<RuntimeProfileRepository>,
    activation_journal: RuntimeActivationJournalRepository,
    manifests: InferenceEngineManifestRegistry,
    engine: Arc<dyn InferenceEngineAdapter>,
    host_capabilities: Option<HostCapabilitySnapshot>,
    backend_manager: Option<Arc<BackendManager>>,
    gateway: Option<GatewayState>,
    external_engines: Option<Arc<ExternalInferenceEngineRegistry>>,
    external_observations: Option<EngineObservationService>,
    pending_activations: PendingPlanStore<PendingActivation>,
    mutations: AsyncMutex<()>,
}

impl RuntimeProfileManager {
    pub fn new(database: Arc<Database>, engine: Arc<dyn InferenceEngineAdapter>) -> Self {
        let profiles = Arc::new(RuntimeProfileRepository::new(database.clone()));
        let activation_journal = RuntimeActivationJournalRepository::new(database.clone());
        let manifests = managed_manifest_registry(engine.as_ref());
        Self {
            database,
            profiles,
            activation_journal,
            manifests,
            engine,
            host_capabilities: None,
            backend_manager: None,
            gateway: None,
            external_engines: None,
            external_observations: None,
            pending_activations: PendingPlanStore::new(ACTIVATION_PLAN_TTL),
            mutations: AsyncMutex::new(()),
        }
    }

    pub fn with_host_capabilities(
        database: Arc<Database>,
        engine: Arc<dyn InferenceEngineAdapter>,
        host_capabilities: HostCapabilitySnapshot,
    ) -> Self {
        let profiles = Arc::new(RuntimeProfileRepository::new(database.clone()));
        let activation_journal = RuntimeActivationJournalRepository::new(database.clone());
        let manifests = managed_manifest_registry(engine.as_ref());
        Self {
            database,
            profiles,
            activation_journal,
            manifests,
            engine,
            host_capabilities: Some(host_capabilities),
            backend_manager: None,
            gateway: None,
            external_engines: None,
            external_observations: None,
            pending_activations: PendingPlanStore::new(ACTIVATION_PLAN_TTL),
            mutations: AsyncMutex::new(()),
        }
    }

    pub fn with_external_context(
        database: Arc<Database>,
        engine: Arc<dyn InferenceEngineAdapter>,
        host_capabilities: HostCapabilitySnapshot,
        backend_manager: Arc<BackendManager>,
        gateway: GatewayState,
        external_engines: Arc<ExternalInferenceEngineRegistry>,
    ) -> Self {
        let profiles = Arc::new(RuntimeProfileRepository::new(database.clone()));
        let activation_journal = RuntimeActivationJournalRepository::new(database.clone());
        let mut manifests = external_engines.manifest_registry().manifests();
        manifests.push(engine.manifest());
        let manifests = InferenceEngineManifestRegistry::new(manifests)
            .expect("compile-time inference engine manifests must be valid and unique");
        let external_observations = EngineObservationService::new(external_engines.clone());
        Self {
            database,
            profiles,
            activation_journal,
            manifests,
            engine,
            host_capabilities: Some(host_capabilities),
            backend_manager: Some(backend_manager),
            gateway: Some(gateway),
            external_engines: Some(external_engines),
            external_observations: Some(external_observations),
            pending_activations: PendingPlanStore::new(ACTIVATION_PLAN_TTL),
            mutations: AsyncMutex::new(()),
        }
    }

    pub fn manifest_registry(&self) -> InferenceEngineManifestRegistry {
        self.manifests.clone()
    }

    /// Builds a multi-instance external capability catalog from Rust-owned saved backends.
    ///
    /// Fixed discovery targets remain useful before configuration, while saved instances are
    /// inspected with their Keychain-backed authentication. Invalid or unavailable instances are
    /// omitted from observations without promoting the adapter's static support status.
    pub async fn external_engine_capabilities(
        &self,
        host: &HostCapabilitySnapshot,
    ) -> Result<Vec<InferenceEngineCapability>, RuntimeProfileManagerError> {
        let registry = self
            .external_engines
            .as_ref()
            .ok_or(RuntimeProfileManagerError::ExternalVerificationRequired)?;
        let observations = self
            .external_observations
            .as_ref()
            .ok_or(RuntimeProfileManagerError::ExternalVerificationRequired)?;
        let backend_manager = self
            .backend_manager
            .as_ref()
            .ok_or(RuntimeProfileManagerError::ExternalVerificationRequired)?;
        let backends = self.database.backends()?;
        let mut capabilities = Vec::new();

        for adapter in registry.adapters() {
            let manifest = adapter.manifest();
            let mut targets = Vec::new();
            for backend in backends.iter().filter(|backend| backend.enabled) {
                if targets.len() >= MAX_EXTERNAL_ENGINE_TARGETS_PER_ADAPTER {
                    break;
                }
                let Some(binding) = self.database.backend_engine_binding(&backend.id)? else {
                    continue;
                };
                if binding.engine_kind != manifest.adapter_id.engine.storage_key()
                    || binding.adapter_variant != manifest.adapter_id.variant
                    || binding.deployment != "local"
                {
                    continue;
                }
                let Ok(request_auth) = backend_manager.engine_request_auth(&backend.id) else {
                    continue;
                };
                let Ok(target) = registry.verified_local_target_by_id_with_auth(
                    &manifest.adapter_id,
                    &backend.id,
                    &backend.api_root,
                    binding.config_revision,
                    request_auth,
                ) else {
                    continue;
                };
                targets.push(target);
            }
            if let Some(default_target) = adapter.default_target()
                && !targets.iter().any(|target| {
                    target.origin().fingerprint_hex() == default_target.origin().fingerprint_hex()
                })
            {
                targets.push(default_target);
            }

            let mut external_runtimes = Vec::new();
            for target in targets {
                if let Ok(observation) = observations.observe_for_display(&target).await {
                    external_runtimes.push(observation.snapshot);
                }
            }
            external_runtimes.sort_by(|left, right| left.api_root.cmp(&right.api_root));
            let compatibility = manifest.compatibility_with(host);
            let recommendation = crate::recommendation_for(
                &compatibility,
                manifest.descriptor.ownership,
                external_runtimes.len(),
            );
            let support_evidence = compatibility.support_evidence.clone().unwrap_or_else(|| {
                crate::support_evidence_for(manifest.descriptor.kind, compatibility.support_status)
            });
            capabilities.push(InferenceEngineCapability {
                descriptor: manifest.descriptor,
                compatibility,
                external_runtimes,
                support_evidence: Some(support_evidence),
                recommendation: Some(recommendation),
            });
        }
        capabilities.sort_by(|left, right| {
            left.descriptor
                .kind
                .storage_key()
                .cmp(right.descriptor.kind.storage_key())
        });
        Ok(capabilities)
    }

    pub fn catalog(&self) -> Result<RuntimeProfileCatalog, RuntimeProfileManagerError> {
        self.database.refresh_local_model_states()?;
        let runtime = self.engine.status()?;
        let manifest = self.engine.manifest();
        let descriptor = &manifest.descriptor;
        let engine_key = descriptor.kind.storage_key();
        let capacity = self.engine.capacity_profile();
        let records = self.profiles.list()?;
        let backends = self.database.backends()?;
        let routing = self.gateway.as_ref().map(GatewayState::routing_snapshot);
        let mut active_profile_id = None;
        let mut profiles = Vec::with_capacity(records.len());

        for record in records {
            let adapter_binding = profile_adapter_binding(&record);
            let evidence = profile_evidence(&record)?;
            let ownership = parse_profile_ownership(&record.ownership)?;
            let digest_kind = parse_digest_kind(&record.model_digest_kind)?;
            if ownership == InferenceEngineOwnership::External {
                let mut issues = Vec::new();
                let engine_kind = InferenceEngineKind::from_storage_key(&record.engine)
                    .ok_or_else(|| {
                        DatabaseError::InvalidData(
                            "runtime profile has an unknown engine identity".to_owned(),
                        )
                    })?;
                let backend_matches = record.backend_id.as_deref().is_some_and(|backend_id| {
                    let endpoint_matches = backends.iter().any(|backend| {
                        backend.id == backend_id
                            && backend.enabled
                            && Some(backend.api_root.as_str()) == record.backend_api_root.as_deref()
                    });
                    let binding_matches = self
                        .database
                        .backend_engine_binding(backend_id)
                        .ok()
                        .flatten()
                        .is_some_and(|binding| {
                            binding.engine_kind == record.engine
                                && binding.adapter_variant == record.adapter_variant
                                && binding.deployment == "local"
                                && Some(binding.config_revision) == record.backend_config_revision
                        });
                    endpoint_matches && binding_matches
                });
                if !backend_matches {
                    issues.push(RuntimeProfileIssue::BackendIdentityChanged);
                }
                let adapter_id = EngineAdapterId {
                    engine: engine_kind,
                    variant: record.adapter_variant.clone(),
                    contract_revision: record.adapter_contract_revision.clone(),
                };
                let adapter = self
                    .external_engines
                    .as_ref()
                    .and_then(|registry| registry.adapter_by_id(&adapter_id));
                if let Some(adapter) = adapter.as_ref() {
                    if !profile_support_cell_matches_manifest(
                        &record,
                        &adapter.manifest(),
                        self.host_capabilities.as_ref(),
                    ) {
                        if record.support_cell.is_some() {
                            issues.push(RuntimeProfileIssue::SupportCellChanged);
                        } else {
                            issues.push(RuntimeProfileIssue::SupportCellMissing);
                        }
                    }
                } else {
                    issues.push(RuntimeProfileIssue::BackendUnavailable);
                }
                if backend_matches
                    && adapter.is_some()
                    && record.support_cell.is_some()
                    && !issues.contains(&RuntimeProfileIssue::SupportCellChanged)
                    && !issues.contains(&RuntimeProfileIssue::SupportCellMissing)
                    && self.external_engines.as_ref().is_some_and(|registry| {
                        self.verified_target_for_profile(&record, registry).is_err()
                    })
                {
                    issues.push(RuntimeProfileIssue::BackendIdentityChanged);
                }
                let active = routing.as_ref().is_some_and(|routing| {
                    routing.active_backend_id.as_deref() == record.backend_id.as_deref()
                        && routing.active_resolved_model.as_deref()
                            == Some(record.model_id.as_str())
                });
                let readiness = profile_readiness(active, &issues);
                if readiness == RuntimeProfileReadiness::Active {
                    active_profile_id = Some(record.id.clone());
                }
                let reviewed_performance = if issues.is_empty() {
                    self.external_engines.as_ref().and_then(|registry| {
                        registry.reviewed_performance_for_runtime_profile(
                            &adapter_id,
                            record.support_cell?,
                            self.host_capabilities.as_ref()?,
                            record.origin_fingerprint.as_deref()?,
                            record.backend_config_revision?,
                            record.engine_version.as_str(),
                            &evidence,
                        )
                    })
                } else {
                    None
                };
                profiles.push(RuntimeProfileSummary {
                    id: record.id,
                    name: record.name,
                    description: record.description,
                    spec_version: record.spec_version,
                    ownership,
                    backend_id: record.backend_id,
                    backend_api_root: record.backend_api_root,
                    model_id: record.model_id,
                    model_display_name: record.model_display_name,
                    model_digest_kind: digest_kind,
                    engine: record.engine,
                    engine_version: record.engine_version,
                    capacity_tier: None,
                    context_window_tokens: None,
                    capacity_revision: None,
                    adapter_binding,
                    evidence,
                    reviewed_performance,
                    readiness,
                    issues,
                    verified_at_ms: record.verified_at_ms,
                    last_activated_at_ms: record.last_activated_at_ms,
                    created_at_ms: record.created_at_ms,
                    updated_at_ms: record.updated_at_ms,
                });
                continue;
            }

            let model = self.database.local_model(&record.model_id)?;
            let integrity = self.database.model_integrity(&record.model_id)?;
            let mut issues = Vec::new();
            let engine_kind =
                InferenceEngineKind::from_storage_key(&record.engine).ok_or_else(|| {
                    DatabaseError::InvalidData(
                        "runtime profile has an unknown engine identity".to_owned(),
                    )
                })?;
            let adapter_id = EngineAdapterId {
                engine: engine_kind,
                variant: record.adapter_variant.clone(),
                contract_revision: record.adapter_contract_revision.clone(),
            };
            let binding_matches = self.manifests.manifest(&adapter_id).is_some()
                && adapter_id.contract_revision == ENGINE_ADAPTER_CONTRACT_REVISION
                && expected_protocol_capability_hash(&adapter_id)
                    .is_some_and(|expected| record.protocol_capability_hash == expected)
                && profile_evidence_is_consistent(&record);
            let engine_matches = record.engine == engine_key && binding_matches;
            if !engine_matches || runtime.install_state != EngineInstallState::Installed {
                issues.push(RuntimeProfileIssue::EngineNotInstalled);
            } else if record.support_cell.is_none() {
                issues.push(RuntimeProfileIssue::SupportCellMissing);
            } else if !profile_support_cell_matches_manifest(
                &record,
                &manifest,
                self.host_capabilities.as_ref(),
            ) {
                issues.push(RuntimeProfileIssue::SupportCellChanged);
            } else if runtime.version != record.engine_version {
                issues.push(RuntimeProfileIssue::EngineVersionChanged);
            }
            if model
                .as_ref()
                .is_none_or(|model| model.state != LocalModelState::Ready)
            {
                issues.push(RuntimeProfileIssue::ModelUnavailable);
            } else if integrity
                .as_ref()
                .and_then(|integrity| integrity.sha256)
                .is_none_or(|sha256| encode_sha256(&sha256) != record.model_digest)
            {
                issues.push(RuntimeProfileIssue::ModelIntegrityChanged);
            }
            if Some(capacity.tier) != record.capacity_tier.as_deref()
                || Some(capacity.context_window_tokens) != record.context_window_tokens
                || Some(capacity.revision) != record.capacity_revision.as_deref()
            {
                issues.push(RuntimeProfileIssue::CapacityPolicyChanged);
            }

            let active = engine_matches
                && runtime.runtime_state == EngineRuntimeState::Running
                && runtime.active_model_id.as_deref() == Some(record.model_id.as_str());
            let readiness = profile_readiness(active, &issues);
            if readiness == RuntimeProfileReadiness::Active {
                active_profile_id = Some(record.id.clone());
            }

            profiles.push(RuntimeProfileSummary {
                id: record.id,
                name: record.name,
                description: record.description,
                spec_version: record.spec_version,
                ownership,
                backend_id: None,
                backend_api_root: None,
                model_id: record.model_id,
                model_display_name: model.as_ref().map_or(record.model_display_name, |model| {
                    model.display_name.clone()
                }),
                model_digest_kind: digest_kind,
                engine: record.engine,
                engine_version: record.engine_version,
                capacity_tier: record.capacity_tier,
                context_window_tokens: record.context_window_tokens,
                capacity_revision: record.capacity_revision,
                adapter_binding,
                evidence,
                reviewed_performance: None,
                readiness,
                issues,
                verified_at_ms: record.verified_at_ms,
                last_activated_at_ms: record.last_activated_at_ms,
                created_at_ms: record.created_at_ms,
                updated_at_ms: record.updated_at_ms,
            });
        }

        let can_save_current = runtime.runtime_state == EngineRuntimeState::Running
            && resolve_support_cell(&manifest, self.host_capabilities.as_ref(), None).is_ok()
            && runtime.active_model_id.as_deref().is_some_and(|model_id| {
                self.database
                    .local_model(model_id)
                    .ok()
                    .flatten()
                    .is_some_and(|model| model.state == LocalModelState::Ready)
                    && self
                        .database
                        .model_integrity(model_id)
                        .ok()
                        .flatten()
                        .and_then(|integrity| integrity.sha256)
                        .is_some()
            });

        Ok(RuntimeProfileCatalog {
            profiles,
            active_profile_id,
            can_save_current,
        })
    }

    pub async fn catalog_verified(
        &self,
    ) -> Result<RuntimeProfileCatalog, RuntimeProfileManagerError> {
        let mut catalog = self.catalog()?;
        let Some(registry) = self.external_engines.as_ref() else {
            return Ok(catalog);
        };
        let Some(observations) = self.external_observations.as_ref() else {
            return Ok(catalog);
        };
        let records = self.profiles.list()?;
        let model_evidence = records
            .iter()
            .map(|profile| Ok((profile.id.clone(), profile_evidence(profile)?)))
            .collect::<Result<HashMap<_, _>, RuntimeProfileManagerError>>()?;
        let records_by_id = records
            .iter()
            .map(|profile| (profile.id.as_str(), profile))
            .collect::<HashMap<_, _>>();
        let mut targets_by_profile = HashMap::<String, EngineTargetKey>::new();
        let mut snapshots = HashMap::<EngineTargetKey, Option<ExternalEngineSnapshot>>::new();

        for profile in catalog
            .profiles
            .iter()
            .filter(|profile| profile.ownership == InferenceEngineOwnership::External)
        {
            let Some(record) = records_by_id.get(profile.id.as_str()) else {
                continue;
            };
            let Ok(target) = self.verified_target_for_profile(record, registry) else {
                continue;
            };
            let key = target.key().clone();
            targets_by_profile.insert(profile.id.clone(), key.clone());
            if let std::collections::hash_map::Entry::Vacant(entry) = snapshots.entry(key) {
                entry.insert(
                    observations
                        .observe_for_display(&target)
                        .await
                        .ok()
                        .map(|observation| observation.snapshot),
                );
            }
        }

        // A model-specific deployment fingerprint is produced by the active qualification
        // request (currently MLX-LM). Re-run that bounded request for profiles that store this
        // evidence kind; comparing the discovery catalog alone would incorrectly mark a valid
        // deployment fingerprint as drifted.
        let mut deployment_evidence = HashMap::<String, Option<RuntimeProfileEvidence>>::new();
        for profile in catalog
            .profiles
            .iter()
            .filter(|profile| profile.ownership == InferenceEngineOwnership::External)
        {
            let Some(expected) = model_evidence.get(&profile.id) else {
                continue;
            };
            if expected.kind != RuntimeProfileEvidenceKind::DeploymentFingerprint {
                continue;
            }
            let Some(record) = records_by_id.get(profile.id.as_str()) else {
                continue;
            };
            let Some(target) = targets_by_profile.get(&profile.id).and_then(|key| {
                // Reconstructing the target from the persisted profile keeps this pass
                // bound to the same backend/config revision and origin checks.
                self.verified_target_for_profile(record, registry)
                    .ok()
                    .filter(|target| target.key() == key)
            }) else {
                deployment_evidence.insert(profile.id.clone(), None);
                continue;
            };
            let observed = registry
                .qualify_target(&target, &record.model_id)
                .await
                .ok()
                .and_then(|report| report.deployment_fingerprint)
                .and_then(|fingerprint| {
                    (fingerprint.len() == 64
                        && fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()))
                    .then(|| RuntimeProfileEvidence {
                        kind: RuntimeProfileEvidenceKind::DeploymentFingerprint,
                        algorithm: "engine-deployment-fingerprint-v1".to_owned(),
                        value: fingerprint.to_ascii_lowercase(),
                    })
                });
            deployment_evidence.insert(profile.id.clone(), observed);
        }

        for profile in catalog
            .profiles
            .iter_mut()
            .filter(|profile| profile.ownership == InferenceEngineOwnership::External)
        {
            let routed_active = profile.readiness == RuntimeProfileReadiness::Active;
            let Some(target_key) = targets_by_profile.get(&profile.id) else {
                push_profile_issue(&mut profile.issues, RuntimeProfileIssue::BackendUnavailable);
                profile.readiness = profile_readiness(routed_active, &profile.issues);
                continue;
            };
            let Some(snapshot) = snapshots.get(target_key).and_then(Option::as_ref) else {
                push_profile_issue(&mut profile.issues, RuntimeProfileIssue::BackendUnavailable);
                profile.readiness = profile_readiness(routed_active, &profile.issues);
                continue;
            };
            if profile.backend_api_root.as_deref() != Some(snapshot.api_root.as_str()) {
                push_profile_issue(
                    &mut profile.issues,
                    RuntimeProfileIssue::BackendIdentityChanged,
                );
            }
            if snapshot.engine_version_exact && profile.engine_version != snapshot.version {
                push_profile_issue(
                    &mut profile.issues,
                    RuntimeProfileIssue::EngineVersionChanged,
                );
            }
            match snapshot
                .models
                .iter()
                .find(|model| model.name == profile.model_id)
            {
                Some(model)
                    if model_evidence.get(&profile.id).is_some_and(|evidence| {
                        if evidence.kind == RuntimeProfileEvidenceKind::DeploymentFingerprint {
                            deployment_evidence
                                .get(&profile.id)
                                .and_then(Option::as_ref)
                                .is_some_and(|observed| observed == evidence)
                        } else {
                            model.evidence == *evidence
                        }
                    }) => {}
                Some(_) => push_profile_issue(
                    &mut profile.issues,
                    RuntimeProfileIssue::ModelIntegrityChanged,
                ),
                None => {
                    push_profile_issue(&mut profile.issues, RuntimeProfileIssue::ModelUnavailable)
                }
            }
            profile.readiness = profile_readiness(routed_active, &profile.issues);
        }
        catalog.active_profile_id = catalog
            .profiles
            .iter()
            .find(|profile| profile.readiness == RuntimeProfileReadiness::Active)
            .map(|profile| profile.id.clone());
        Ok(catalog)
    }

    pub fn save_current(
        &self,
        draft: RuntimeProfileDraft,
    ) -> Result<RuntimeProfileCatalog, RuntimeProfileManagerError> {
        let (name, description) = validate_draft(draft)?;
        self.database.refresh_local_model_states()?;
        let runtime = self.engine.status()?;
        let manifest = self.engine.manifest();
        let descriptor = &manifest.descriptor;
        let engine_key = descriptor.kind.storage_key();
        let support_cell = resolve_support_cell(&manifest, self.host_capabilities.as_ref(), None)?;
        if runtime.install_state != EngineInstallState::Installed
            || runtime.runtime_state != EngineRuntimeState::Running
        {
            return Err(RuntimeProfileManagerError::NoVerifiedRuntime);
        }
        let model_id = runtime
            .active_model_id
            .as_deref()
            .ok_or(RuntimeProfileManagerError::NoVerifiedRuntime)?;
        let model = self
            .database
            .local_model(model_id)?
            .filter(|model| model.state == LocalModelState::Ready)
            .ok_or(RuntimeProfileManagerError::NoVerifiedRuntime)?;
        let model_sha256 = self
            .database
            .model_integrity(model_id)?
            .and_then(|integrity| integrity.sha256)
            .ok_or(RuntimeProfileManagerError::NoVerifiedRuntime)?;
        if self
            .profiles
            .list()?
            .iter()
            .any(|profile| profile.model_id == model_id && profile.engine == engine_key)
        {
            return Err(RuntimeProfileManagerError::DuplicateProfile);
        }
        let capacity = self.engine.capacity_profile();
        let adapter_id = self.engine.manifest().adapter_id;
        let model_digest = encode_sha256(&model_sha256);
        let now_ms = now_ms();
        self.profiles.insert(&StoredRuntimeProfileRecord {
            id: format!("runtime-profile-{}", Uuid::new_v4().simple()),
            name,
            description,
            spec_version: RUNTIME_PROFILE_SPEC_VERSION,
            ownership: "managed".to_owned(),
            backend_id: None,
            backend_api_root: None,
            model_id: model.id,
            model_display_name: model.display_name,
            model_digest: model_digest.clone(),
            model_digest_kind: "sha256".to_owned(),
            engine: engine_key.to_owned(),
            engine_version: runtime.version,
            capacity_tier: Some(capacity.tier.to_owned()),
            context_window_tokens: Some(capacity.context_window_tokens),
            capacity_revision: Some(capacity.revision.to_owned()),
            adapter_variant: adapter_id.variant,
            adapter_contract_revision: adapter_id.contract_revision,
            backend_config_revision: None,
            origin_fingerprint: None,
            evidence_kind: "content_digest".to_owned(),
            evidence_algorithm: "sha256".to_owned(),
            evidence_value: model_digest,
            protocol_capability_hash: OPENAI_CORE_CAPABILITY_HASH.to_owned(),
            support_cell: Some(support_cell),
            verified_at_ms: now_ms,
            last_activated_at_ms: Some(now_ms),
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        })?;
        self.catalog()
    }

    pub async fn save_external(
        &self,
        draft: ExternalRuntimeProfileDraft,
    ) -> Result<RuntimeProfileCatalog, RuntimeProfileManagerError> {
        let (name, description) = validate_draft(RuntimeProfileDraft {
            name: draft.name,
            description: draft.description,
        })?;
        validate_external_candidate(&draft.backend_id, &draft.model_id, &draft.expected_evidence)?;
        let registry = self
            .external_engines
            .as_ref()
            .ok_or(RuntimeProfileManagerError::ExternalVerificationRequired)?;
        let observations = self
            .external_observations
            .as_ref()
            .ok_or(RuntimeProfileManagerError::ExternalVerificationRequired)?;
        let backend = self
            .database
            .backends()?
            .into_iter()
            .find(|backend| backend.id == draft.backend_id && backend.enabled)
            .ok_or(RuntimeProfileManagerError::ProfileNeedsRepair)?;
        let binding = self
            .database
            .backend_engine_binding(&backend.id)?
            .ok_or(RuntimeProfileManagerError::ProfileNeedsRepair)?;
        let engine = InferenceEngineKind::from_storage_key(&binding.engine_kind)
            .ok_or(RuntimeProfileManagerError::ProfileNeedsRepair)?;
        let adapter_id = EngineAdapterId {
            engine,
            variant: binding.adapter_variant.clone(),
            contract_revision: ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
        };
        let manifest = registry
            .manifest_registry()
            .manifest(&adapter_id)
            .ok_or(RuntimeProfileManagerError::ProfileNeedsRepair)?;
        let support_cell = resolve_support_cell(
            &manifest,
            self.host_capabilities.as_ref(),
            draft.support_cell.as_ref(),
        )?;
        let request_auth = self
            .backend_manager
            .as_ref()
            .ok_or(RuntimeProfileManagerError::ExternalVerificationRequired)?
            .engine_request_auth(&backend.id)?;
        let target = registry.verified_local_target_by_id_with_auth(
            &adapter_id,
            &backend.id,
            &backend.api_root,
            binding.config_revision,
            request_auth,
        )?;
        let snapshot = observations
            .observe_for_authorization(&target)
            .await?
            .snapshot;
        if snapshot.engine != adapter_id.engine
            || snapshot.api_root != backend.api_root
            || !snapshot.model_catalog_complete
        {
            return Err(RuntimeProfileManagerError::ProfileNeedsRepair);
        }
        let model = snapshot
            .models
            .iter()
            .find(|model| model.name == draft.model_id && model.evidence == draft.expected_evidence)
            .cloned()
            .ok_or(RuntimeProfileManagerError::ProfileChanged)?;
        let qualification = self
            .qualify_external_protocol(registry, &target, &model.name, support_cell)
            .await?;
        let engine_version = qualified_engine_version(&snapshot, &qualification)?;
        let model = qualified_external_model(model, &qualification)?;
        let _guard = self.mutations.lock().await;
        let backend_unchanged = self.database.backends()?.into_iter().any(|current| {
            current.id == backend.id
                && current.enabled
                && current.api_root == backend.api_root
                && current.updated_at_ms == backend.updated_at_ms
                && self
                    .database
                    .backend_engine_binding(&current.id)
                    .ok()
                    .flatten()
                    == Some(binding.clone())
        });
        if !backend_unchanged {
            return Err(RuntimeProfileManagerError::ProfileChanged);
        }
        if self.profiles.list()?.iter().any(|profile| {
            profile.ownership == "external"
                && profile.backend_id.as_deref() == Some(draft.backend_id.as_str())
                && profile.model_id == draft.model_id
                && profile.engine == binding.engine_kind
        }) {
            return Err(RuntimeProfileManagerError::DuplicateProfile);
        }
        let now_ms = now_ms();
        let adapter_id = target.adapter_id();
        self.profiles.insert(&StoredRuntimeProfileRecord {
            id: format!("runtime-profile-{}", Uuid::new_v4().simple()),
            name,
            description,
            spec_version: RUNTIME_PROFILE_SPEC_VERSION,
            ownership: "external".to_owned(),
            backend_id: Some(backend.id),
            backend_api_root: Some(backend.api_root),
            model_id: model.name.clone(),
            model_display_name: model.name.clone(),
            model_digest: model.digest.clone(),
            model_digest_kind: model_digest_kind_key(
                model.evidence.kind,
                &model.evidence.algorithm,
            )
            .to_owned(),
            engine: binding.engine_kind,
            engine_version,
            capacity_tier: None,
            context_window_tokens: None,
            capacity_revision: None,
            adapter_variant: adapter_id.variant.clone(),
            adapter_contract_revision: adapter_id.contract_revision.clone(),
            backend_config_revision: Some(binding.config_revision),
            origin_fingerprint: Some(target.origin().fingerprint_hex()),
            evidence_kind: evidence_kind_key(model.evidence.kind).to_owned(),
            evidence_algorithm: model.evidence.algorithm.clone(),
            evidence_value: model.evidence.value.clone(),
            protocol_capability_hash: qualification.capability_hash,
            support_cell: Some(support_cell),
            verified_at_ms: now_ms,
            last_activated_at_ms: None,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        })?;
        self.catalog()
    }

    pub async fn reverify_external(
        &self,
        profile_id: &str,
    ) -> Result<RuntimeProfileCatalog, RuntimeProfileManagerError> {
        let profile = self
            .profiles
            .get(profile_id)?
            .ok_or(RuntimeProfileManagerError::ProfileNotFound)?;
        if parse_profile_ownership(&profile.ownership)? != InferenceEngineOwnership::External {
            return Err(RuntimeProfileManagerError::ExternalProfileRequired);
        }
        let (snapshot, model) = self.inspect_external_profile(&profile).await?;
        let registry = self
            .external_engines
            .as_ref()
            .ok_or(RuntimeProfileManagerError::ExternalVerificationRequired)?;
        let target = self.verified_target_for_profile(&profile, registry)?;
        let support_cell = profile
            .support_cell
            .ok_or(RuntimeProfileManagerError::ProfileNeedsRepair)?;
        let qualification = self
            .qualify_external_protocol(registry, &target, &model.name, support_cell)
            .await?;
        let engine_version = qualified_engine_version(&snapshot, &qualification)?;
        let model = qualified_external_model(model, &qualification)?;
        let guard = self.mutations.lock().await;
        if self.profiles.get(profile_id)?.as_ref() != Some(&profile) {
            return Err(RuntimeProfileManagerError::ProfileChanged);
        }
        if !self.profiles.reverify(
            &profile.id,
            &StoredRuntimeProfileVerification {
                model_digest: model.digest.clone(),
                evidence_kind: evidence_kind_key(model.evidence.kind).to_owned(),
                evidence_algorithm: model.evidence.algorithm,
                evidence_value: model.evidence.value,
                engine_version,
                capacity_tier: None,
                context_window_tokens: None,
                capacity_revision: None,
                support_cell: profile.support_cell,
            },
            now_ms(),
        )? {
            return Err(RuntimeProfileManagerError::ProfileNotFound);
        }
        drop(guard);
        self.catalog_verified().await
    }

    pub fn update(
        &self,
        profile_id: &str,
        draft: RuntimeProfileDraft,
    ) -> Result<RuntimeProfileCatalog, RuntimeProfileManagerError> {
        let (name, description) = validate_draft(draft)?;
        if !self
            .profiles
            .update_metadata(profile_id, &name, &description, now_ms())?
        {
            return Err(RuntimeProfileManagerError::ProfileNotFound);
        }
        self.catalog()
    }

    pub fn delete(
        &self,
        profile_id: &str,
    ) -> Result<RuntimeProfileCatalog, RuntimeProfileManagerError> {
        let profile = self
            .profiles
            .get(profile_id)?
            .ok_or(RuntimeProfileManagerError::ProfileNotFound)?;
        self.profiles.delete(profile_id, &profile.name, now_ms())?;
        self.catalog()
    }

    pub fn plan_activation(
        &self,
        profile_id: &str,
    ) -> Result<RuntimeProfileActivationPlan, RuntimeProfileManagerError> {
        if !self.activation_journal.pending()?.is_empty() {
            return Err(RuntimeProfileManagerError::ActivationRecoveryRequired);
        }
        let profile = self
            .profiles
            .get(profile_id)?
            .ok_or(RuntimeProfileManagerError::ProfileNotFound)?;
        if parse_profile_ownership(&profile.ownership)? != InferenceEngineOwnership::Managed {
            return Err(RuntimeProfileManagerError::ExternalVerificationRequired);
        }
        let catalog = self.catalog()?;
        let summary = catalog
            .profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .ok_or(RuntimeProfileManagerError::ProfileNotFound)?;
        if summary.readiness == RuntimeProfileReadiness::NeedsRepair {
            return Err(RuntimeProfileManagerError::ProfileNeedsRepair);
        }
        let runtime = self.engine.status()?;
        let requires_confirmation = runtime_change_requires_confirmation(
            runtime.runtime_state,
            runtime.active_model_id.as_deref(),
            &profile.model_id,
        );
        let pending = PendingActivation {
            profile_id: profile.id.clone(),
            expected_updated_at_ms: profile.updated_at_ms,
            model_id: profile.model_id.clone(),
            ownership: InferenceEngineOwnership::Managed,
            backend_id: None,
            observed_engine_version: runtime.version.clone(),
            observed_model_digest: profile.model_digest.clone(),
            observed_evidence: profile_evidence(&profile)?,
            authority: ActivationAuthorityBinding::from_profile(&profile)?,
            expected_route: self.database.active_gateway_route()?,
            requires_confirmation,
        };
        let ticket = self.pending_activations.replace(pending)?;
        let action_summary = match runtime.active_model_name.as_deref() {
            Some(current) if requires_confirmation => format!(
                "等待当前请求结束后，将 {current} 切换为 {}；失败时尝试恢复原模型",
                profile.model_display_name
            ),
            Some(_) => format!("复验并继续运行 {}", profile.model_display_name),
            None => format!("启动并验证 {}", profile.model_display_name),
        };

        Ok(RuntimeProfileActivationPlan {
            plan_id: ticket.plan_id,
            expires_at_ms: ticket.expires_at_ms,
            profile_id: profile.id,
            profile_name: profile.name,
            model_id: profile.model_id,
            model_display_name: profile.model_display_name,
            ownership: InferenceEngineOwnership::Managed,
            backend_id: None,
            engine: profile.engine,
            engine_version: runtime.version.clone(),
            support_cell: profile.support_cell,
            context_window_tokens: Some(self.engine.capacity_profile().context_window_tokens),
            current_backend_id: self
                .gateway
                .as_ref()
                .and_then(|gateway| gateway.routing_snapshot().active_backend_id),
            current_model_id: runtime.active_model_id,
            current_model_name: runtime.active_model_name,
            issues: summary.issues.clone(),
            action_summary,
            requires_confirmation,
        })
    }

    pub async fn plan_activation_verified(
        &self,
        profile_id: &str,
    ) -> Result<RuntimeProfileActivationPlan, RuntimeProfileManagerError> {
        if !self.activation_journal.pending()?.is_empty() {
            return Err(RuntimeProfileManagerError::ActivationRecoveryRequired);
        }
        let profile = self
            .profiles
            .get(profile_id)?
            .ok_or(RuntimeProfileManagerError::ProfileNotFound)?;
        if parse_profile_ownership(&profile.ownership)? == InferenceEngineOwnership::Managed {
            return self.plan_activation(profile_id);
        }
        let (snapshot, model, engine_version) =
            self.inspect_and_qualify_external_profile(&profile).await?;
        if engine_version != profile.engine_version || model.evidence != profile_evidence(&profile)?
        {
            return Err(RuntimeProfileManagerError::ProfileChanged);
        }
        let issues = Vec::new();
        let routing = self
            .gateway
            .as_ref()
            .ok_or(RuntimeProfileManagerError::ExternalVerificationRequired)?
            .routing_snapshot();
        let requires_confirmation = routing.active_backend_id.as_deref()
            != profile.backend_id.as_deref()
            || routing.active_resolved_model.as_deref() != Some(profile.model_id.as_str());
        let runtime = self.engine.status()?;
        let current_model_id = runtime
            .active_model_id
            .clone()
            .or_else(|| routing.active_resolved_model.clone());
        let current_model_name = runtime
            .active_model_name
            .clone()
            .or_else(|| routing.active_resolved_model.clone());
        let pending = PendingActivation {
            profile_id: profile.id.clone(),
            expected_updated_at_ms: profile.updated_at_ms,
            model_id: profile.model_id.clone(),
            ownership: InferenceEngineOwnership::External,
            backend_id: profile.backend_id.clone(),
            observed_engine_version: engine_version.clone(),
            observed_model_digest: model.digest.clone(),
            observed_evidence: model.evidence.clone(),
            authority: ActivationAuthorityBinding::from_profile(&profile)?,
            expected_route: self.database.active_gateway_route()?,
            requires_confirmation,
        };
        let ticket = self.pending_activations.replace(pending)?;
        let action_summary = if requires_confirmation {
            format!(
                "等待当前请求结束后，将活动路由切换到 {} 的 {}；失败时恢复原路由",
                snapshot.display_name, profile.model_display_name
            )
        } else {
            format!("重新复验并继续使用 {}", profile.model_display_name)
        };
        Ok(RuntimeProfileActivationPlan {
            plan_id: ticket.plan_id,
            expires_at_ms: ticket.expires_at_ms,
            profile_id: profile.id,
            profile_name: profile.name,
            model_id: profile.model_id,
            model_display_name: profile.model_display_name,
            ownership: InferenceEngineOwnership::External,
            backend_id: profile.backend_id,
            engine: profile.engine,
            engine_version,
            support_cell: profile.support_cell,
            context_window_tokens: None,
            current_backend_id: routing.active_backend_id,
            current_model_id,
            current_model_name,
            issues,
            action_summary,
            requires_confirmation,
        })
    }

    pub fn activation_requires_confirmation(
        &self,
        plan_id: &str,
    ) -> Result<bool, RuntimeProfileManagerError> {
        Ok(self
            .pending_activations
            .peek(plan_id)?
            .requires_confirmation)
    }

    pub fn discard_activation_plan(
        &self,
        plan_id: &str,
    ) -> Result<bool, RuntimeProfileManagerError> {
        self.pending_activations
            .discard(plan_id)
            .map_err(Into::into)
    }

    pub async fn verify_active_profile(
        &self,
        profile_id: &str,
    ) -> Result<bool, RuntimeProfileManagerError> {
        let profile = self
            .profiles
            .get(profile_id)?
            .ok_or(RuntimeProfileManagerError::ProfileNotFound)?;
        if profile.last_activated_at_ms.is_none() {
            return Ok(false);
        }
        if parse_profile_ownership(&profile.ownership)? == InferenceEngineOwnership::Managed {
            return Ok(self.catalog()?.active_profile_id.as_deref() == Some(profile_id));
        }

        let (_snapshot, model, engine_version) =
            self.inspect_and_qualify_external_profile(&profile).await?;
        if engine_version != profile.engine_version || model.evidence != profile_evidence(&profile)?
        {
            return Ok(false);
        }
        let routing = self
            .gateway
            .as_ref()
            .ok_or(RuntimeProfileManagerError::ExternalVerificationRequired)?
            .routing_snapshot();
        Ok(
            routing.active_backend_id.as_deref() == profile.backend_id.as_deref()
                && routing.active_resolved_model.as_deref() == Some(profile.model_id.as_str()),
        )
    }

    pub async fn apply_activation(
        &self,
        plan_id: &str,
    ) -> Result<RuntimeProfileActivationResult, RuntimeProfileManagerError> {
        let _guard = self.mutations.lock().await;
        let pending = self.pending_activations.take(plan_id)?;
        let profile = self
            .profiles
            .get(&pending.profile_id)?
            .ok_or(RuntimeProfileManagerError::ProfileNotFound)?;
        if profile.updated_at_ms != pending.expected_updated_at_ms
            || profile.model_id != pending.model_id
            || parse_profile_ownership(&profile.ownership)? != pending.ownership
            || profile.backend_id != pending.backend_id
            || ActivationAuthorityBinding::from_profile(&profile)? != pending.authority
            || self.database.active_gateway_route()? != pending.expected_route
        {
            return Err(RuntimeProfileManagerError::ProfileChanged);
        }
        if pending.ownership == InferenceEngineOwnership::External {
            return self.apply_external_activation(profile, pending).await;
        }
        let current_catalog = self.catalog()?;
        let current_summary = current_catalog
            .profiles
            .iter()
            .find(|summary| summary.id == profile.id)
            .ok_or(RuntimeProfileManagerError::ProfileNotFound)?;
        if current_summary.readiness == RuntimeProfileReadiness::NeedsRepair {
            return Err(RuntimeProfileManagerError::ProfileNeedsRepair);
        }

        let model_sha256 = self
            .database
            .model_integrity(&profile.model_id)?
            .and_then(|integrity| integrity.sha256)
            .ok_or(RuntimeProfileManagerError::NoVerifiedRuntime)?;
        let capacity = self.engine.capacity_profile();
        let previous = self.engine.status()?;
        let mut activation_journal = None;
        let runtime = if previous.runtime_state == EngineRuntimeState::Running
            && previous.active_model_id.as_deref() == Some(profile.model_id.as_str())
        {
            previous.clone()
        } else {
            let previous_route = self.database.active_gateway_route()?;
            let mut journal =
                self.begin_activation_journal(&profile.id, previous_route, &previous)?;
            match self.engine.start_model(&profile.model_id).await {
                Ok(runtime) => {
                    if self
                        .advance_activation_journal(
                            &mut journal,
                            RuntimeActivationPhase::RouteSwitched,
                        )
                        .is_err()
                    {
                        let rollback_restored = self.compensate_activation(&mut journal).await;
                        return Err(RuntimeProfileManagerError::ActivationFailed {
                            rollback_restored,
                        });
                    }
                    activation_journal = Some(journal);
                    runtime
                }
                Err(_error) => {
                    let rollback_restored = self.compensate_activation(&mut journal).await;
                    return Err(RuntimeProfileManagerError::ActivationFailed { rollback_restored });
                }
            }
        };
        let marked = self.profiles.mark_activated(
            &profile.id,
            &StoredRuntimeProfileVerification {
                model_digest: encode_sha256(&model_sha256),
                evidence_kind: "content_digest".to_owned(),
                evidence_algorithm: "sha256".to_owned(),
                evidence_value: encode_sha256(&model_sha256),
                engine_version: runtime.version.clone(),
                capacity_tier: Some(capacity.tier.to_owned()),
                context_window_tokens: Some(capacity.context_window_tokens),
                capacity_revision: Some(capacity.revision.to_owned()),
                support_cell: profile.support_cell,
            },
            now_ms(),
        );
        match marked {
            Ok(true) => {}
            Ok(false) => {
                if let Some(journal) = activation_journal.as_mut() {
                    let _ = self.compensate_activation(journal).await;
                }
                return Err(RuntimeProfileManagerError::ProfileNotFound);
            }
            Err(error) => {
                if let Some(journal) = activation_journal.as_mut() {
                    let _ = self.compensate_activation(journal).await;
                }
                return Err(error.into());
            }
        }
        if let Some(journal) = activation_journal.as_ref()
            && !self
                .activation_journal
                .finish(&journal.id, RuntimeActivationPhase::RouteSwitched)?
        {
            return Err(RuntimeProfileManagerError::ActivationRecoveryRequired);
        }
        let catalog = self.catalog()?;
        let active_backend_id = self
            .gateway
            .as_ref()
            .and_then(|gateway| gateway.routing_snapshot().active_backend_id);
        let active_model_id = profile.model_id.clone();
        Ok(RuntimeProfileActivationResult {
            profile_id: profile.id,
            ownership: InferenceEngineOwnership::Managed,
            active_backend_id,
            active_model_id,
            managed_runtime: Some(runtime),
            catalog,
        })
    }

    async fn apply_external_activation(
        &self,
        profile: StoredRuntimeProfileRecord,
        pending: PendingActivation,
    ) -> Result<RuntimeProfileActivationResult, RuntimeProfileManagerError> {
        let backend_manager = self
            .backend_manager
            .as_ref()
            .ok_or(RuntimeProfileManagerError::ExternalVerificationRequired)?;
        let (_snapshot, model, engine_version) =
            self.inspect_and_qualify_external_profile(&profile).await?;
        if engine_version != pending.observed_engine_version
            || model.digest != pending.observed_model_digest
            || model.evidence != pending.observed_evidence
        {
            return Err(RuntimeProfileManagerError::ProfileChanged);
        }
        let previous_runtime = self.engine.status()?;
        let previous_route = self.database.active_gateway_route()?;
        let mut journal =
            self.begin_activation_journal(&profile.id, previous_route.clone(), &previous_runtime)?;
        if previous_runtime.runtime_state == EngineRuntimeState::Running
            && self.engine.stop().await.is_err()
        {
            let rollback_restored = self.compensate_activation(&mut journal).await;
            return Err(RuntimeProfileManagerError::ActivationFailed { rollback_restored });
        }
        if self
            .advance_activation_journal(&mut journal, RuntimeActivationPhase::Quiesced)
            .is_err()
        {
            let rollback_restored = self.compensate_activation(&mut journal).await;
            return Err(RuntimeProfileManagerError::ActivationFailed { rollback_restored });
        }
        let backend_id = profile
            .backend_id
            .as_deref()
            .ok_or(RuntimeProfileManagerError::ProfileNeedsRepair)?;
        if backend_manager
            .activate_resolved_backend(backend_id, &profile.model_id)
            .await
            .is_err()
        {
            let rollback_restored = self.compensate_activation(&mut journal).await;
            return Err(RuntimeProfileManagerError::ActivationFailed { rollback_restored });
        }
        if self
            .advance_activation_journal(&mut journal, RuntimeActivationPhase::RouteSwitched)
            .is_err()
        {
            let rollback_restored = self.compensate_activation(&mut journal).await;
            return Err(RuntimeProfileManagerError::ActivationFailed { rollback_restored });
        }

        let verified = self.inspect_and_qualify_external_profile(&profile).await;
        let routing_matches = self.gateway.as_ref().is_some_and(|gateway| {
            let routing = gateway.routing_snapshot();
            routing.active_backend_id.as_deref() == Some(backend_id)
                && routing.active_resolved_model.as_deref() == Some(profile.model_id.as_str())
        });
        let (_verified_snapshot, verified_model, verified_engine_version) = match verified {
            Ok((verified_snapshot, verified_model, verified_engine_version))
                if verified_engine_version == engine_version
                    && verified_model.digest == model.digest
                    && verified_model.evidence == model.evidence
                    && routing_matches =>
            {
                (verified_snapshot, verified_model, verified_engine_version)
            }
            _ => {
                let rollback_restored = self.compensate_activation(&mut journal).await;
                return Err(RuntimeProfileManagerError::ActivationFailed { rollback_restored });
            }
        };
        let activated_at_ms = now_ms();
        let marked = self.profiles.mark_activated(
            &profile.id,
            &StoredRuntimeProfileVerification {
                model_digest: verified_model.digest.clone(),
                evidence_kind: evidence_kind_key(verified_model.evidence.kind).to_owned(),
                evidence_algorithm: verified_model.evidence.algorithm,
                evidence_value: verified_model.evidence.value,
                engine_version: verified_engine_version,
                capacity_tier: None,
                context_window_tokens: None,
                capacity_revision: None,
                support_cell: profile.support_cell,
            },
            activated_at_ms,
        );
        match marked {
            Ok(true) => {}
            Ok(false) => {
                let _ = self.compensate_activation(&mut journal).await;
                return Err(RuntimeProfileManagerError::ProfileNotFound);
            }
            Err(error) => {
                let _ = self.compensate_activation(&mut journal).await;
                return Err(error.into());
            }
        }
        if !self
            .activation_journal
            .finish(&journal.id, RuntimeActivationPhase::RouteSwitched)?
        {
            return Err(RuntimeProfileManagerError::ActivationRecoveryRequired);
        }
        let active_backend_id = backend_id.to_owned();
        let active_model_id = profile.model_id.clone();
        let catalog = self.catalog()?;
        Ok(RuntimeProfileActivationResult {
            profile_id: profile.id,
            ownership: InferenceEngineOwnership::External,
            active_backend_id: Some(active_backend_id),
            active_model_id,
            managed_runtime: None,
            catalog,
        })
    }

    async fn inspect_external_profile(
        &self,
        profile: &StoredRuntimeProfileRecord,
    ) -> Result<(ExternalEngineSnapshot, ExternalEngineModelSummary), RuntimeProfileManagerError>
    {
        let registry = self
            .external_engines
            .as_ref()
            .ok_or(RuntimeProfileManagerError::ExternalVerificationRequired)?;
        let observations = self
            .external_observations
            .as_ref()
            .ok_or(RuntimeProfileManagerError::ExternalVerificationRequired)?;
        let target = self.verified_target_for_profile(profile, registry)?;
        let snapshot = observations
            .observe_for_authorization(&target)
            .await?
            .snapshot;
        if snapshot.api_root != target.origin().api_root().as_str()
            || !snapshot.model_catalog_complete
        {
            return Err(RuntimeProfileManagerError::ProfileNeedsRepair);
        }
        let model = snapshot
            .models
            .iter()
            .find(|model| model.name == profile.model_id)
            .cloned()
            .ok_or(RuntimeProfileManagerError::ProfileNeedsRepair)?;
        Ok((snapshot, model))
    }

    async fn inspect_and_qualify_external_profile(
        &self,
        profile: &StoredRuntimeProfileRecord,
    ) -> Result<
        (ExternalEngineSnapshot, ExternalEngineModelSummary, String),
        RuntimeProfileManagerError,
    > {
        let (snapshot, model) = self.inspect_external_profile(profile).await?;
        let registry = self
            .external_engines
            .as_ref()
            .ok_or(RuntimeProfileManagerError::ExternalVerificationRequired)?;
        let target = self.verified_target_for_profile(profile, registry)?;
        let support_cell = profile
            .support_cell
            .ok_or(RuntimeProfileManagerError::ProfileNeedsRepair)?;
        let qualification = self
            .qualify_external_protocol(registry, &target, &model.name, support_cell)
            .await?;
        let engine_version = qualified_engine_version(&snapshot, &qualification)?;
        let model = qualified_external_model(model, &qualification)?;
        Ok((snapshot, model, engine_version))
    }

    fn verified_target_for_profile(
        &self,
        profile: &StoredRuntimeProfileRecord,
        registry: &ExternalInferenceEngineRegistry,
    ) -> Result<VerifiedEngineTarget, RuntimeProfileManagerError> {
        let engine = InferenceEngineKind::from_storage_key(&profile.engine)
            .ok_or(RuntimeProfileManagerError::ProfileNeedsRepair)?;
        let adapter_id = EngineAdapterId {
            engine,
            variant: profile.adapter_variant.clone(),
            contract_revision: profile.adapter_contract_revision.clone(),
        };
        if adapter_id.contract_revision != ENGINE_ADAPTER_CONTRACT_REVISION
            || !expected_protocol_capability_hash(&adapter_id)
                .is_some_and(|expected| profile.protocol_capability_hash == expected)
            || !profile_evidence_is_consistent(profile)
        {
            return Err(RuntimeProfileManagerError::ProfileNeedsRepair);
        }
        let manifest = registry
            .manifest_registry()
            .manifest(&adapter_id)
            .ok_or(RuntimeProfileManagerError::ProfileNeedsRepair)?;
        if !profile_support_cell_matches_manifest(
            profile,
            &manifest,
            self.host_capabilities.as_ref(),
        ) {
            return Err(RuntimeProfileManagerError::ProfileNeedsRepair);
        }
        let backend_id = profile
            .backend_id
            .as_deref()
            .ok_or(RuntimeProfileManagerError::ProfileNeedsRepair)?;
        let backend = self
            .database
            .backends()?
            .into_iter()
            .find(|backend| backend.id == backend_id && backend.enabled)
            .ok_or(RuntimeProfileManagerError::ProfileNeedsRepair)?;
        if Some(backend.api_root.as_str()) != profile.backend_api_root.as_deref() {
            return Err(RuntimeProfileManagerError::ProfileNeedsRepair);
        }
        let binding = self
            .database
            .backend_engine_binding(&backend.id)?
            .ok_or(RuntimeProfileManagerError::ProfileNeedsRepair)?;
        if binding.engine_kind != profile.engine
            || binding.adapter_variant != profile.adapter_variant
            || binding.deployment != "local"
            || profile.backend_config_revision != Some(binding.config_revision)
        {
            return Err(RuntimeProfileManagerError::ProfileNeedsRepair);
        }
        let request_auth = self
            .backend_manager
            .as_ref()
            .ok_or(RuntimeProfileManagerError::ExternalVerificationRequired)?
            .engine_request_auth(&backend.id)?;
        let target = registry.verified_local_target_by_id_with_auth(
            &adapter_id,
            &backend.id,
            &backend.api_root,
            binding.config_revision,
            request_auth,
        )?;
        if profile.origin_fingerprint.as_deref() != Some(target.origin().fingerprint_hex().as_str())
        {
            return Err(RuntimeProfileManagerError::ProfileNeedsRepair);
        }
        Ok(target)
    }

    async fn qualify_external_protocol(
        &self,
        registry: &ExternalInferenceEngineRegistry,
        target: &VerifiedEngineTarget,
        model_id: &str,
        support_cell: RuntimeProfileSupportCell,
    ) -> Result<ExternalProtocolQualification, RuntimeProfileManagerError> {
        let Some(expected_hash) = expected_protocol_capability_hash(target.adapter_id()) else {
            return Err(RuntimeProfileManagerError::ProfileNeedsRepair);
        };
        let report = registry.qualify_target(target, model_id).await?;
        if report.adapter_id != *target.adapter_id()
            || report.model_id != model_id
            || report.protocol_capability_hash != expected_hash
        {
            return Err(RuntimeProfileManagerError::ProfileNeedsRepair);
        }
        if report.observed_engine_version.is_none()
            && !matches!(
                target.adapter_id().engine,
                InferenceEngineKind::MlcLlm | InferenceEngineKind::LmDeploy
            )
        {
            return Err(RuntimeProfileManagerError::ProfileNeedsRepair);
        }
        let manifest = registry
            .manifest_registry()
            .manifest(target.adapter_id())
            .ok_or(RuntimeProfileManagerError::ProfileNeedsRepair)?;
        if !qualification_proves_support_cell(
            &manifest,
            support_cell,
            report.runtime_device_evidence,
        ) {
            return Err(RuntimeProfileManagerError::SupportCellNotProven);
        }
        Ok(ExternalProtocolQualification {
            capability_hash: report.protocol_capability_hash,
            observed_engine_version: report.observed_engine_version,
            deployment_fingerprint: report.deployment_fingerprint,
        })
    }

    fn begin_activation_journal(
        &self,
        profile_id: &str,
        previous_route: Option<StoredActiveGatewayRoute>,
        previous_runtime: &hal100_protocol::ManagedEngineStatus,
    ) -> Result<StoredRuntimeActivationJournal, RuntimeProfileManagerError> {
        if !self.activation_journal.pending()?.is_empty() {
            return Err(RuntimeProfileManagerError::ActivationRecoveryRequired);
        }
        let timestamp = now_ms();
        let journal = StoredRuntimeActivationJournal {
            id: format!("runtime-activation-{}", Uuid::new_v4().simple()),
            profile_id: profile_id.to_owned(),
            phase: RuntimeActivationPhase::Journaled,
            previous_route,
            previous_managed_model_id: (previous_runtime.runtime_state
                == EngineRuntimeState::Running)
                .then(|| previous_runtime.active_model_id.clone())
                .flatten(),
            created_at_ms: timestamp,
            updated_at_ms: timestamp,
        };
        self.activation_journal.begin(&journal)?;
        Ok(journal)
    }

    fn advance_activation_journal(
        &self,
        journal: &mut StoredRuntimeActivationJournal,
        next: RuntimeActivationPhase,
    ) -> Result<(), RuntimeProfileManagerError> {
        if !self
            .activation_journal
            .transition(&journal.id, journal.phase, next, now_ms())?
        {
            return Err(RuntimeProfileManagerError::ActivationRecoveryRequired);
        }
        journal.phase = next;
        journal.updated_at_ms = now_ms();
        Ok(())
    }

    async fn restore_journal_state(&self, journal: &StoredRuntimeActivationJournal) -> bool {
        if let Some(previous_model_id) = journal.previous_managed_model_id.as_deref() {
            let route_restored = if let Some(backend_manager) = self.backend_manager.as_ref() {
                backend_manager
                    .restore_active_route(journal.previous_route.as_ref())
                    .await
                    .is_ok()
            } else {
                journal.previous_route.is_none()
            };
            route_restored && self.engine.start_model(previous_model_id).await.is_ok()
        } else {
            let runtime_stopped = match self.engine.status() {
                Ok(status) if status.runtime_state == EngineRuntimeState::Running => {
                    self.engine.stop().await.is_ok()
                }
                Ok(_) => true,
                Err(_) => false,
            };
            let route_restored = if let Some(backend_manager) = self.backend_manager.as_ref() {
                backend_manager
                    .restore_active_route(journal.previous_route.as_ref())
                    .await
                    .is_ok()
            } else {
                journal.previous_route.is_none()
            };
            runtime_stopped && route_restored
        }
    }

    async fn compensate_activation(&self, journal: &mut StoredRuntimeActivationJournal) -> bool {
        if journal.phase != RuntimeActivationPhase::Compensating
            && self
                .advance_activation_journal(journal, RuntimeActivationPhase::Compensating)
                .is_err()
        {
            return false;
        }
        if self.restore_journal_state(journal).await {
            self.activation_journal
                .finish(&journal.id, RuntimeActivationPhase::Compensating)
                .unwrap_or(false)
        } else {
            self.advance_activation_journal(journal, RuntimeActivationPhase::RecoveryRequired)
                .is_ok()
                && false
        }
    }

    pub async fn recover_incomplete_activation(&self) -> Result<bool, RuntimeProfileManagerError> {
        let _guard = self.mutations.lock().await;
        let pending = self.activation_journal.pending()?;
        if pending.is_empty() {
            return Ok(true);
        }
        if pending.len() != 1 {
            return Err(RuntimeProfileManagerError::ActivationRecoveryRequired);
        }
        let mut journal = pending.into_iter().next().expect("one pending journal");
        if journal.phase != RuntimeActivationPhase::Compensating {
            self.advance_activation_journal(&mut journal, RuntimeActivationPhase::Compensating)?;
        }
        if self.restore_journal_state(&journal).await
            && self
                .activation_journal
                .finish(&journal.id, RuntimeActivationPhase::Compensating)?
        {
            return Ok(true);
        }
        self.advance_activation_journal(&mut journal, RuntimeActivationPhase::RecoveryRequired)?;
        Ok(false)
    }
}

fn managed_manifest_registry(
    engine: &dyn InferenceEngineAdapter,
) -> InferenceEngineManifestRegistry {
    InferenceEngineManifestRegistry::new(vec![engine.manifest()])
        .expect("compile-time managed inference engine manifest must be valid")
}

fn is_formal_support_status(status: InferenceEngineSupportStatus) -> bool {
    matches!(
        status,
        InferenceEngineSupportStatus::Managed | InferenceEngineSupportStatus::VerifiedExternal
    )
}

fn support_unit_matches_cell(
    unit: &hal100_protocol::InferenceEngineSupportUnit,
    cell: RuntimeProfileSupportCell,
) -> bool {
    unit.platform == cell.platform
        && unit.architecture == cell.architecture
        && unit.accelerator == cell.accelerator
        && unit.deployment == cell.deployment
}

/// Bind a qualification to the exact formal support cell without manufacturing device evidence.
/// A model-residency observation proves its own accelerator. A fixed adapter-variant contract is
/// accepted only when every cell and descriptor coordinate in that variant uses the same device.
/// An unresolved report always fails closed, even if only one formal cell currently happens to be
/// promoted; promotion state must not silently strengthen a weak qualification report.
fn qualification_proves_support_cell(
    manifest: &InferenceEngineManifest,
    cell: RuntimeProfileSupportCell,
    runtime_device_evidence: EngineRuntimeDeviceEvidence,
) -> bool {
    if !manifest
        .support_units
        .iter()
        .any(|unit| support_unit_matches_cell(unit, cell) && is_formal_support_status(unit.status))
    {
        return false;
    }
    match runtime_device_evidence {
        EngineRuntimeDeviceEvidence::ModelResidencyObservation { accelerator } => {
            accelerator == cell.accelerator
        }
        EngineRuntimeDeviceEvidence::AdapterVariantContract { accelerator } => {
            accelerator == cell.accelerator
                && manifest.descriptor.accelerators.as_slice() == [accelerator]
                && manifest
                    .support_units
                    .iter()
                    .all(|unit| unit.accelerator == accelerator)
        }
        EngineRuntimeDeviceEvidence::Unresolved => false,
    }
}

/// Resolve the exact support cell that a profile is allowed to bind to.
///
/// An omitted selection is accepted only when exactly one formal cell matches the current host
/// (or the manifest when no host snapshot is available). A host with mixed formal/non-formal
/// accelerators therefore requires an explicit cell, while an explicit choice is still accepted
/// when that chosen unit is formal and host-compatible.
fn resolve_support_cell(
    manifest: &InferenceEngineManifest,
    host: Option<&HostCapabilitySnapshot>,
    requested: Option<&RuntimeProfileSupportCell>,
) -> Result<RuntimeProfileSupportCell, RuntimeProfileManagerError> {
    let matching_units = manifest
        .support_units
        .iter()
        .filter(|unit| unit.deployment == manifest.descriptor.deployment)
        .filter(|unit| {
            host.is_none_or(|host| {
                unit.platform == host.platform
                    && unit.architecture == host.architecture
                    && host.accelerators.contains(&unit.accelerator)
            })
        })
        .collect::<Vec<_>>();
    if let Some(requested) = requested {
        let valid = matching_units.iter().any(|unit| {
            support_unit_matches_cell(unit, *requested) && is_formal_support_status(unit.status)
        });
        return if valid {
            Ok(*requested)
        } else {
            Err(RuntimeProfileManagerError::InvalidSupportCell)
        };
    }
    let formal = matching_units
        .iter()
        .filter(|unit| is_formal_support_status(unit.status))
        .collect::<Vec<_>>();
    if matching_units.len() == 1 && formal.len() == 1 {
        let unit = formal[0];
        return Ok(RuntimeProfileSupportCell {
            platform: unit.platform,
            architecture: unit.architecture,
            accelerator: unit.accelerator,
            deployment: unit.deployment,
        });
    }
    Err(RuntimeProfileManagerError::SupportCellSelectionRequired)
}

fn profile_support_cell_matches_manifest(
    profile: &StoredRuntimeProfileRecord,
    manifest: &InferenceEngineManifest,
    host: Option<&HostCapabilitySnapshot>,
) -> bool {
    let Some(cell) = profile.support_cell else {
        return false;
    };
    if host.is_some_and(|host| !cell.matches_host(host)) {
        return false;
    }
    manifest
        .support_units
        .iter()
        .any(|unit| support_unit_matches_cell(unit, cell) && is_formal_support_status(unit.status))
}

fn parse_profile_ownership(
    value: &str,
) -> Result<InferenceEngineOwnership, RuntimeProfileManagerError> {
    match value {
        "managed" => Ok(InferenceEngineOwnership::Managed),
        "external" => Ok(InferenceEngineOwnership::External),
        _ => Err(
            DatabaseError::InvalidData("runtime profile has an invalid ownership".to_owned())
                .into(),
        ),
    }
}

fn parse_digest_kind(
    value: &str,
) -> Result<RuntimeProfileModelDigestKind, RuntimeProfileManagerError> {
    match value {
        "sha256" => Ok(RuntimeProfileModelDigestKind::Sha256),
        "ollama_digest" => Ok(RuntimeProfileModelDigestKind::OllamaDigest),
        "evidence_fingerprint" => Ok(RuntimeProfileModelDigestKind::EvidenceFingerprint),
        _ => Err(DatabaseError::InvalidData(
            "runtime profile has an invalid model digest kind".to_owned(),
        )
        .into()),
    }
}

fn profile_adapter_binding(profile: &StoredRuntimeProfileRecord) -> RuntimeProfileAdapterBinding {
    RuntimeProfileAdapterBinding {
        variant: profile.adapter_variant.clone(),
        contract_revision: profile.adapter_contract_revision.clone(),
        backend_config_revision: profile.backend_config_revision,
        origin_fingerprint: profile.origin_fingerprint.clone(),
        protocol_capability_hash: Some(profile.protocol_capability_hash.clone()),
        support_cell: profile.support_cell,
    }
}

fn profile_evidence(
    profile: &StoredRuntimeProfileRecord,
) -> Result<RuntimeProfileEvidence, RuntimeProfileManagerError> {
    let kind = match profile.evidence_kind.as_str() {
        "content_digest" => RuntimeProfileEvidenceKind::ContentDigest,
        "repository_revision" => RuntimeProfileEvidenceKind::RepositoryRevision,
        "deployment_fingerprint" => RuntimeProfileEvidenceKind::DeploymentFingerprint,
        "catalog_identity" => RuntimeProfileEvidenceKind::CatalogIdentity,
        _ => {
            return Err(DatabaseError::InvalidData(
                "runtime profile has an invalid evidence kind".to_owned(),
            )
            .into());
        }
    };
    Ok(RuntimeProfileEvidence {
        kind,
        algorithm: profile.evidence_algorithm.clone(),
        value: profile.evidence_value.clone(),
    })
}

const fn evidence_kind_key(kind: RuntimeProfileEvidenceKind) -> &'static str {
    match kind {
        RuntimeProfileEvidenceKind::ContentDigest => "content_digest",
        RuntimeProfileEvidenceKind::RepositoryRevision => "repository_revision",
        RuntimeProfileEvidenceKind::DeploymentFingerprint => "deployment_fingerprint",
        RuntimeProfileEvidenceKind::CatalogIdentity => "catalog_identity",
    }
}

fn expected_protocol_capability_hash(adapter_id: &EngineAdapterId) -> Option<String> {
    if adapter_id.contract_revision != ENGINE_ADAPTER_CONTRACT_REVISION {
        return None;
    }
    match (adapter_id.engine, adapter_id.variant.as_str()) {
        (InferenceEngineKind::Ollama, "official-loopback-api") => {
            Some(crate::ollama_agent_protocol_capability_hash())
        }
        (InferenceEngineKind::Vllm, "official-openai-server") => {
            Some(crate::vllm_agent_protocol_capability_hash())
        }
        (InferenceEngineKind::MlxLm, "official-http-server") => {
            Some(crate::mlx_lm_agent_protocol_capability_hash())
        }
        (
            InferenceEngineKind::MlcLlm,
            "official-openai-metal"
            | "official-openai-vulkan"
            | "official-openai-cuda"
            | "official-openai-rocm",
        ) => Some(crate::mlc_llm_agent_protocol_capability_hash()),
        (
            InferenceEngineKind::OpenVino,
            "ovms-openai-cpu" | "ovms-openai-intel-gpu" | "ovms-openai-intel-npu",
        ) => Some(crate::openvino_agent_protocol_capability_hash()),
        (InferenceEngineKind::Sglang, "official-openai-server") => {
            Some(crate::sglang_agent_protocol_capability_hash())
        }
        (InferenceEngineKind::LmDeploy, "official-openai-server") => {
            Some(crate::lmdeploy_agent_protocol_capability_hash())
        }
        (InferenceEngineKind::TensorRtLlm, "trtllm-serve-openai-server") => {
            Some(crate::tensorrt_llm_agent_protocol_capability_hash())
        }
        (InferenceEngineKind::LlamaCpp, "hal100-managed-metal") => {
            Some(OPENAI_CORE_CAPABILITY_HASH.to_owned())
        }
        _ => None,
    }
}

fn qualified_engine_version(
    snapshot: &ExternalEngineSnapshot,
    qualification: &ExternalProtocolQualification,
) -> Result<String, RuntimeProfileManagerError> {
    match (
        snapshot.engine_version_exact,
        qualification.observed_engine_version.as_deref(),
    ) {
        (true, Some(observed)) if observed != snapshot.version => {
            Err(RuntimeProfileManagerError::ProfileChanged)
        }
        (true, _) => Ok(snapshot.version.clone()),
        (false, Some(observed)) => Ok(observed.to_owned()),
        (false, None) if qualification.deployment_fingerprint.is_some() => {
            Ok(ENGINE_VERSION_NOT_EXPOSED.to_owned())
        }
        (false, None) => Err(RuntimeProfileManagerError::ProfileNeedsRepair),
    }
}

fn qualified_external_model(
    mut model: ExternalEngineModelSummary,
    qualification: &ExternalProtocolQualification,
) -> Result<ExternalEngineModelSummary, RuntimeProfileManagerError> {
    let Some(fingerprint) = qualification.deployment_fingerprint.as_deref() else {
        return Ok(model);
    };
    if fingerprint.len() != 64 || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(RuntimeProfileManagerError::ProfileNeedsRepair);
    }
    model.digest = fingerprint.to_ascii_lowercase();
    model.evidence = RuntimeProfileEvidence {
        kind: RuntimeProfileEvidenceKind::DeploymentFingerprint,
        algorithm: "engine-deployment-fingerprint-v1".to_owned(),
        value: model.digest.clone(),
    };
    Ok(model)
}

fn profile_evidence_is_consistent(profile: &StoredRuntimeProfileRecord) -> bool {
    match (
        profile.evidence_kind.as_str(),
        profile.evidence_algorithm.as_str(),
    ) {
        ("content_digest", "sha256") | ("content_digest", "ollama-digest") => {
            profile.evidence_value == profile.model_digest
        }
        ("repository_revision", _) | ("deployment_fingerprint", _) | ("catalog_identity", _) => {
            !profile.evidence_value.is_empty()
        }
        _ => false,
    }
}

fn encode_sha256(digest: &[u8; 32]) -> String {
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn validate_external_candidate(
    backend_id: &str,
    model_id: &str,
    evidence: &RuntimeProfileEvidence,
) -> Result<(), RuntimeProfileManagerError> {
    let backend_valid = !backend_id.is_empty()
        && backend_id.len() <= 128
        && backend_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    let model_valid = !model_id.trim().is_empty()
        && model_id.len() <= 256
        && !model_id.chars().any(char::is_control);
    let evidence_valid = !evidence.algorithm.is_empty()
        && evidence.algorithm.len() <= 64
        && evidence
            .algorithm
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        && !evidence.value.is_empty()
        && evidence.value.len() <= 512
        && !evidence.value.chars().any(char::is_control)
        && (evidence.kind != RuntimeProfileEvidenceKind::ContentDigest
            || (evidence.value.len() == 64
                && evidence.value.bytes().all(|byte| byte.is_ascii_hexdigit())));
    if !backend_valid || !model_valid || !evidence_valid {
        return Err(RuntimeProfileManagerError::InvalidExternalDraft);
    }
    Ok(())
}

fn model_digest_kind_key(
    evidence_kind: RuntimeProfileEvidenceKind,
    algorithm: &str,
) -> &'static str {
    match (evidence_kind, algorithm) {
        (RuntimeProfileEvidenceKind::ContentDigest, "sha256") => "sha256",
        (RuntimeProfileEvidenceKind::ContentDigest, "ollama-digest") => "ollama_digest",
        _ => "evidence_fingerprint",
    }
}

fn validate_draft(
    draft: RuntimeProfileDraft,
) -> Result<(String, String), RuntimeProfileManagerError> {
    let name = draft.name.trim().to_owned();
    if name.is_empty()
        || name.chars().count() > MAX_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(RuntimeProfileManagerError::InvalidName);
    }
    let description = draft.description.trim().to_owned();
    if description.chars().count() > MAX_DESCRIPTION_CHARS
        || description
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\t'))
    {
        return Err(RuntimeProfileManagerError::InvalidDescription);
    }
    Ok((name, description))
}

fn runtime_change_requires_confirmation(
    runtime_state: EngineRuntimeState,
    active_model_id: Option<&str>,
    target_model_id: &str,
) -> bool {
    runtime_state != EngineRuntimeState::Running || active_model_id != Some(target_model_id)
}

fn profile_readiness(
    active_model_matches: bool,
    issues: &[RuntimeProfileIssue],
) -> RuntimeProfileReadiness {
    if issues.iter().any(|issue| {
        matches!(
            issue,
            RuntimeProfileIssue::EngineNotInstalled
                | RuntimeProfileIssue::BackendUnavailable
                | RuntimeProfileIssue::BackendIdentityChanged
                | RuntimeProfileIssue::EngineIncompatible
                | RuntimeProfileIssue::SupportCellMissing
                | RuntimeProfileIssue::SupportCellChanged
                | RuntimeProfileIssue::ModelUnavailable
        )
    }) {
        RuntimeProfileReadiness::NeedsRepair
    } else if !issues.is_empty() {
        RuntimeProfileReadiness::NeedsVerification
    } else if active_model_matches {
        RuntimeProfileReadiness::Active
    } else {
        RuntimeProfileReadiness::Ready
    }
}

fn push_profile_issue(issues: &mut Vec<RuntimeProfileIssue>, issue: RuntimeProfileIssue) {
    if !issues.contains(&issue) {
        issues.push(issue);
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        path::PathBuf,
        sync::{
            Mutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
    };

    use hal100_core::{SecretStore, SecretStoreError, SecretStoreOperation};
    use hal100_protocol::{
        EngineAdapterId, EngineQualificationReport, ExternalEngineModelSummary,
        InferenceAccelerator, InferenceArchitecture, InferenceDeployment,
        InferenceEngineDescriptor, InferenceEngineManifest, InferenceEngineSupportStatus,
        InferenceEngineSupportUnit, InferenceModelFormat, InferencePlatform, InferenceProtocol,
    };

    use super::*;
    use crate::{
        CredentialRegistry, EngineInspector, ExternalEngineInspectionFuture,
        ExternalEngineQualificationFuture, ExternalInferenceEngineAdapter, LlamaCppManager,
        StoredBackendRecord, UsageWriter, VerifiedEngineTarget,
    };

    #[test]
    fn runtime_profile_failures_preserve_stage_retry_and_recovery_semantics() {
        let unreachable =
            RuntimeProfileManagerError::ExternalEngine(ExternalEngineAdapterError::Unreachable)
                .failure();
        assert_eq!(
            unreachable,
            RuntimeProfileFailure::new(
                RuntimeProfileFailureCode::EngineUnreachable,
                RuntimeProfileFailureStage::Inspection,
                true,
                RuntimeProfileRecoveryAction::CheckService,
            )
        );

        let device_unproven = RuntimeProfileManagerError::SupportCellNotProven.failure();
        assert_eq!(
            device_unproven,
            RuntimeProfileFailure::new(
                RuntimeProfileFailureCode::RuntimeDeviceUnproven,
                RuntimeProfileFailureStage::Qualification,
                false,
                RuntimeProfileRecoveryAction::SelectSupportCell,
            )
        );

        let failed_with_recovery = RuntimeProfileManagerError::ActivationFailed {
            rollback_restored: false,
        }
        .failure();
        assert_eq!(
            failed_with_recovery.recovery_action,
            RuntimeProfileRecoveryAction::RecoverActivation
        );
        assert!(!failed_with_recovery.retryable);

        let failed_but_rolled_back = RuntimeProfileManagerError::ActivationFailed {
            rollback_restored: true,
        }
        .failure();
        assert_eq!(
            failed_but_rolled_back.recovery_action,
            RuntimeProfileRecoveryAction::Retry
        );
        assert!(failed_but_rolled_back.retryable);
    }

    #[test]
    fn qualified_external_model_binds_a_valid_deployment_fingerprint() {
        let model = ExternalEngineModelSummary {
            name: "mlx-community/Qwen3-0.6B-4bit".to_owned(),
            digest: "catalog-digest".to_owned(),
            size_bytes: 1,
            format: "mlx".to_owned(),
            family: None,
            parameter_size: None,
            quantization: None,
            evidence: RuntimeProfileEvidence {
                kind: RuntimeProfileEvidenceKind::CatalogIdentity,
                algorithm: "catalog-identity-v1".to_owned(),
                value: "catalog-digest".to_owned(),
            },
        };
        let qualification = ExternalProtocolQualification {
            capability_hash: "capability".to_owned(),
            observed_engine_version: Some("0.31.3".to_owned()),
            deployment_fingerprint: Some("ABCDEF0123456789".repeat(4)),
        };

        let qualified = qualified_external_model(model, &qualification).expect("valid identity");

        assert_eq!(qualified.digest, "abcdef0123456789".repeat(4));
        assert_eq!(
            qualified.evidence,
            RuntimeProfileEvidence {
                kind: RuntimeProfileEvidenceKind::DeploymentFingerprint,
                algorithm: "engine-deployment-fingerprint-v1".to_owned(),
                value: "abcdef0123456789".repeat(4),
            }
        );
    }

    #[test]
    fn qualified_external_model_rejects_malformed_deployment_fingerprint() {
        let model = ExternalEngineModelSummary {
            name: "model".to_owned(),
            digest: "catalog-digest".to_owned(),
            size_bytes: 1,
            format: "mlx".to_owned(),
            family: None,
            parameter_size: None,
            quantization: None,
            evidence: RuntimeProfileEvidence {
                kind: RuntimeProfileEvidenceKind::CatalogIdentity,
                algorithm: "catalog-identity-v1".to_owned(),
                value: "catalog-digest".to_owned(),
            },
        };
        let qualification = ExternalProtocolQualification {
            capability_hash: "capability".to_owned(),
            observed_engine_version: Some("0.31.3".to_owned()),
            deployment_fingerprint: Some("not-a-sha256".to_owned()),
        };

        assert!(matches!(
            qualified_external_model(model, &qualification),
            Err(RuntimeProfileManagerError::ProfileNeedsRepair)
        ));
    }

    #[test]
    fn support_cell_selection_is_required_for_mixed_formal_and_non_formal_units() {
        let manifest = InferenceEngineManifest {
            adapter_id: EngineAdapterId {
                engine: InferenceEngineKind::OpenVino,
                variant: "official-openai-server".to_owned(),
                contract_revision: ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
            },
            descriptor: InferenceEngineDescriptor {
                kind: InferenceEngineKind::OpenVino,
                display_name: "OpenVINO test".to_owned(),
                ownership: InferenceEngineOwnership::External,
                deployment: InferenceDeployment::Local,
                protocols: vec![InferenceProtocol::OpenAi],
                platforms: vec![InferencePlatform::Windows],
                architectures: vec![InferenceArchitecture::X86_64],
                accelerators: vec![InferenceAccelerator::Cpu, InferenceAccelerator::IntelGpu],
                model_formats: vec![InferenceModelFormat::OpenVino],
                managed_lifecycle: false,
            },
            support_units: vec![
                InferenceEngineSupportUnit {
                    platform: InferencePlatform::Windows,
                    architecture: InferenceArchitecture::X86_64,
                    accelerator: InferenceAccelerator::Cpu,
                    deployment: InferenceDeployment::Local,
                    status: InferenceEngineSupportStatus::VerifiedExternal,
                    evidence: Some(crate::support_evidence_for(
                        InferenceEngineKind::OpenVino,
                        Some(InferenceEngineSupportStatus::VerifiedExternal),
                    )),
                },
                InferenceEngineSupportUnit {
                    platform: InferencePlatform::Windows,
                    architecture: InferenceArchitecture::X86_64,
                    accelerator: InferenceAccelerator::IntelGpu,
                    deployment: InferenceDeployment::Local,
                    status: InferenceEngineSupportStatus::Connected,
                    evidence: None,
                },
            ],
        };
        let host = HostCapabilitySnapshot {
            platform: InferencePlatform::Windows,
            architecture: InferenceArchitecture::X86_64,
            cpu_brand: "test".to_owned(),
            device_model: "test".to_owned(),
            total_memory_bytes: 1,
            physical_cpu_cores: 1,
            logical_cpu_cores: 1,
            accelerators: vec![InferenceAccelerator::Cpu, InferenceAccelerator::IntelGpu],
            model_storage_path: "/tmp".to_owned(),
            model_storage_available_bytes: 1,
            probe_revision: "test".to_owned(),
        };
        assert!(matches!(
            resolve_support_cell(&manifest, Some(&host), None),
            Err(RuntimeProfileManagerError::SupportCellSelectionRequired)
        ));
        let cpu_cell = RuntimeProfileSupportCell {
            platform: InferencePlatform::Windows,
            architecture: InferenceArchitecture::X86_64,
            accelerator: InferenceAccelerator::Cpu,
            deployment: InferenceDeployment::Local,
        };
        assert_eq!(
            resolve_support_cell(&manifest, Some(&host), Some(&cpu_cell))
                .expect("explicit formal cell"),
            cpu_cell
        );
        let intel_gpu_cell = RuntimeProfileSupportCell {
            accelerator: InferenceAccelerator::IntelGpu,
            ..cpu_cell
        };
        assert!(matches!(
            resolve_support_cell(&manifest, Some(&host), Some(&intel_gpu_cell)),
            Err(RuntimeProfileManagerError::InvalidSupportCell)
        ));

        // Formal support does not strengthen an unresolved report. The mixed-device adapter also
        // cannot claim a fixed variant contract; only exact model-residency evidence proves it.
        assert!(!qualification_proves_support_cell(
            &manifest,
            cpu_cell,
            EngineRuntimeDeviceEvidence::Unresolved
        ));
        assert!(!qualification_proves_support_cell(
            &manifest,
            cpu_cell,
            EngineRuntimeDeviceEvidence::AdapterVariantContract {
                accelerator: InferenceAccelerator::Cpu,
            }
        ));
        let mut ambiguous = manifest;
        ambiguous.support_units[1].status = InferenceEngineSupportStatus::VerifiedExternal;
        ambiguous.support_units[1].evidence = Some(crate::support_evidence_for(
            InferenceEngineKind::OpenVino,
            Some(InferenceEngineSupportStatus::VerifiedExternal),
        ));
        assert!(!qualification_proves_support_cell(
            &ambiguous,
            cpu_cell,
            EngineRuntimeDeviceEvidence::Unresolved
        ));
        assert!(!qualification_proves_support_cell(
            &ambiguous,
            cpu_cell,
            EngineRuntimeDeviceEvidence::ModelResidencyObservation {
                accelerator: InferenceAccelerator::IntelGpu,
            }
        ));
        assert!(qualification_proves_support_cell(
            &ambiguous,
            cpu_cell,
            EngineRuntimeDeviceEvidence::ModelResidencyObservation {
                accelerator: InferenceAccelerator::Cpu,
            }
        ));

        let mut fixed_cpu = ambiguous;
        fixed_cpu.descriptor.accelerators = vec![InferenceAccelerator::Cpu];
        fixed_cpu
            .support_units
            .retain(|unit| unit.accelerator == InferenceAccelerator::Cpu);
        assert!(qualification_proves_support_cell(
            &fixed_cpu,
            cpu_cell,
            EngineRuntimeDeviceEvidence::AdapterVariantContract {
                accelerator: InferenceAccelerator::Cpu,
            }
        ));
    }

    #[test]
    fn deployment_fingerprint_allows_profile_identity_without_exposed_version() {
        let snapshot = ExternalEngineSnapshot {
            engine: InferenceEngineKind::MlcLlm,
            display_name: "MLC test".to_owned(),
            api_root: "http://127.0.0.1:8000/v1/".to_owned(),
            version: ENGINE_VERSION_NOT_EXPOSED.to_owned(),
            engine_version_exact: false,
            models: Vec::new(),
            model_catalog_complete: true,
        };
        let qualification = ExternalProtocolQualification {
            capability_hash: "capability".to_owned(),
            observed_engine_version: None,
            deployment_fingerprint: Some("a".repeat(64)),
        };
        assert_eq!(
            qualified_engine_version(&snapshot, &qualification)
                .expect("explicit no-version marker"),
            ENGINE_VERSION_NOT_EXPOSED
        );
        let without_identity = ExternalProtocolQualification {
            deployment_fingerprint: None,
            ..qualification
        };
        assert!(matches!(
            qualified_engine_version(&snapshot, &without_identity),
            Err(RuntimeProfileManagerError::ProfileNeedsRepair)
        ));
    }

    struct FakeExternalAdapter {
        manifest: InferenceEngineManifest,
        snapshot: Mutex<ExternalEngineSnapshot>,
        qualification: Option<EngineQualificationReport>,
        inspections: AtomicUsize,
        drift_on_inspection: Mutex<Option<(usize, String)>>,
        qualification_unavailable: AtomicBool,
    }

    impl FakeExternalAdapter {
        fn inspection_count(&self) -> usize {
            self.inspections.load(Ordering::Acquire)
        }

        fn set_digest(&self, digest: String) {
            let mut snapshot = self.snapshot.lock().expect("snapshot lock");
            snapshot.models[0].digest.clone_from(&digest);
            snapshot.models[0].evidence.value = digest;
        }

        fn set_drift_on_inspection(&self, inspection: usize, digest: String) {
            *self.drift_on_inspection.lock().expect("drift lock") = Some((inspection, digest));
        }

        fn disable_qualification(&self) {
            self.qualification_unavailable
                .store(true, Ordering::Release);
        }
    }

    impl EngineInspector for FakeExternalAdapter {
        fn manifest(&self) -> InferenceEngineManifest {
            self.manifest.clone()
        }

        fn default_target(&self) -> Option<VerifiedEngineTarget> {
            VerifiedEngineTarget::external_local(
                "test:ollama",
                &self.manifest(),
                &self.snapshot.lock().expect("snapshot lock").api_root,
                0,
            )
            .ok()
        }

        fn inspect<'a>(
            &'a self,
            target: &'a VerifiedEngineTarget,
        ) -> ExternalEngineInspectionFuture<'a> {
            let inspection = self.inspections.fetch_add(1, Ordering::AcqRel) + 1;
            let mut snapshot = self.snapshot.lock().expect("snapshot lock").clone();
            snapshot.api_root = target.origin().api_root().as_str().to_owned();
            if let Some((threshold, digest)) = self
                .drift_on_inspection
                .lock()
                .expect("drift lock")
                .as_ref()
                && inspection >= *threshold
            {
                snapshot.models[0].digest.clone_from(digest);
                snapshot.models[0].evidence.value.clone_from(digest);
            }
            Box::pin(async move { Ok(snapshot) })
        }

        fn qualify<'a>(
            &'a self,
            target: &'a VerifiedEngineTarget,
            model_id: &'a str,
        ) -> ExternalEngineQualificationFuture<'a> {
            if self.qualification_unavailable.load(Ordering::Acquire) {
                return Box::pin(async {
                    Err(ExternalEngineAdapterError::QualificationUnavailable)
                });
            }
            let report = self.qualification.clone();
            let adapter_id = target.adapter_id().clone();
            let model_id = model_id.to_owned();
            Box::pin(async move {
                let mut report = report
                    .filter(|report| report.adapter_id == adapter_id)
                    .ok_or(ExternalEngineAdapterError::QualificationUnavailable)?;
                report.model_id = model_id;
                Ok(report)
            })
        }
    }

    impl ExternalInferenceEngineAdapter for FakeExternalAdapter {}

    #[derive(Default)]
    struct MemorySecretStore {
        values: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl SecretStore for MemorySecretStore {
        fn read(&self, credential_id: &str) -> Result<Option<Vec<u8>>, SecretStoreError> {
            Ok(self
                .values
                .lock()
                .map_err(|_| SecretStoreError::new(SecretStoreOperation::Read))?
                .get(credential_id)
                .cloned())
        }

        fn write(&self, credential_id: &str, secret: &[u8]) -> Result<(), SecretStoreError> {
            self.values
                .lock()
                .map_err(|_| SecretStoreError::new(SecretStoreOperation::Write))?
                .insert(credential_id.to_owned(), secret.to_vec());
            Ok(())
        }

        fn delete(&self, credential_id: &str) -> Result<(), SecretStoreError> {
            self.values
                .lock()
                .map_err(|_| SecretStoreError::new(SecretStoreOperation::Delete))?
                .remove(credential_id);
            Ok(())
        }
    }

    struct ExternalProfileFixture {
        manager: RuntimeProfileManager,
        adapter: Arc<FakeExternalAdapter>,
        database: Arc<Database>,
        gateway: GatewayState,
        engine_root: PathBuf,
    }

    impl Drop for ExternalProfileFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.engine_root);
        }
    }

    fn external_profile_fixture() -> ExternalProfileFixture {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        database
            .upsert_backend(&StoredBackendRecord {
                id: "saved-ollama".to_owned(),
                display_name: "本机 Ollama".to_owned(),
                kind: "external_ollama".to_owned(),
                engine_kind: None,
                adapter_variant: None,
                api_root: "http://127.0.0.1:11434/v1/".to_owned(),
                auth_style: "none".to_owned(),
                credential_id: None,
                enabled: true,
                created_at_ms: 1,
                updated_at_ms: 1,
            })
            .expect("stored backend");
        let gateway = GatewayState::new(
            None,
            CredentialRegistry::new(Vec::new()),
            UsageWriter::start(database.clone()),
        )
        .expect("gateway");
        let backend_manager = Arc::new(BackendManager::new(
            database.clone(),
            gateway.clone(),
            Arc::new(MemorySecretStore::default()),
        ));
        backend_manager.restore().expect("restore backend");
        let engine_root = std::env::temp_dir().join(format!(
            "hal100-runtime-profile-test-{}",
            Uuid::new_v4().simple()
        ));
        let engine = Arc::new(
            LlamaCppManager::new(database.clone(), gateway.clone(), engine_root.clone())
                .expect("managed engine"),
        );
        let adapter = Arc::new(FakeExternalAdapter {
            manifest: InferenceEngineManifest {
                adapter_id: EngineAdapterId {
                    engine: InferenceEngineKind::Ollama,
                    variant: "official-loopback-api".to_owned(),
                    contract_revision: ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
                },
                descriptor: InferenceEngineDescriptor {
                    kind: InferenceEngineKind::Ollama,
                    display_name: "测试 Ollama".to_owned(),
                    ownership: InferenceEngineOwnership::External,
                    deployment: InferenceDeployment::Local,
                    protocols: vec![InferenceProtocol::OpenAi, InferenceProtocol::Ollama],
                    platforms: vec![InferencePlatform::MacOs],
                    architectures: vec![InferenceArchitecture::Aarch64],
                    accelerators: vec![InferenceAccelerator::Cpu],
                    model_formats: vec![InferenceModelFormat::Gguf],
                    managed_lifecycle: false,
                },
                support_units: vec![InferenceEngineSupportUnit {
                    platform: InferencePlatform::MacOs,
                    architecture: InferenceArchitecture::Aarch64,
                    accelerator: InferenceAccelerator::Cpu,
                    deployment: InferenceDeployment::Local,
                    status: InferenceEngineSupportStatus::VerifiedExternal,
                    evidence: Some(crate::support_evidence_for(
                        InferenceEngineKind::Ollama,
                        Some(InferenceEngineSupportStatus::VerifiedExternal),
                    )),
                }],
            },
            snapshot: Mutex::new(ExternalEngineSnapshot {
                engine: InferenceEngineKind::Ollama,
                display_name: "本机 Ollama".to_owned(),
                api_root: "http://127.0.0.1:11434/v1/".to_owned(),
                version: "0.12.6-test".to_owned(),
                engine_version_exact: true,
                models: vec![ExternalEngineModelSummary {
                    name: "qwen3:8b".to_owned(),
                    digest: "a".repeat(64),
                    size_bytes: 4_000_000_000,
                    format: "gguf".to_owned(),
                    family: Some("qwen3".to_owned()),
                    parameter_size: Some("8B".to_owned()),
                    quantization: Some("Q4_K_M".to_owned()),
                    evidence: RuntimeProfileEvidence {
                        kind: RuntimeProfileEvidenceKind::ContentDigest,
                        algorithm: "ollama-digest".to_owned(),
                        value: "a".repeat(64),
                    },
                }],
                model_catalog_complete: true,
            }),
            qualification: Some(EngineQualificationReport {
                adapter_id: EngineAdapterId {
                    engine: InferenceEngineKind::Ollama,
                    variant: "official-loopback-api".to_owned(),
                    contract_revision: ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
                },
                model_id: "qwen3:8b".to_owned(),
                protocol_capabilities: crate::ollama_agent_protocol_capabilities(),
                protocol_capability_hash: crate::ollama_agent_protocol_capability_hash(),
                observed_engine_version: Some("0.12.6-test".to_owned()),
                runtime_device_evidence: EngineRuntimeDeviceEvidence::ModelResidencyObservation {
                    accelerator: InferenceAccelerator::Cpu,
                },
                deployment_fingerprint: None,
            }),
            inspections: AtomicUsize::new(0),
            drift_on_inspection: Mutex::new(None),
            qualification_unavailable: AtomicBool::new(false),
        });
        let registry = Arc::new(
            ExternalInferenceEngineRegistry::new(vec![adapter.clone()]).expect("external registry"),
        );
        let manager = RuntimeProfileManager::with_external_context(
            database.clone(),
            engine,
            HostCapabilitySnapshot {
                platform: InferencePlatform::MacOs,
                architecture: InferenceArchitecture::Aarch64,
                cpu_brand: "Test".to_owned(),
                device_model: "Test".to_owned(),
                total_memory_bytes: 16 * 1024 * 1024 * 1024,
                physical_cpu_cores: 8,
                logical_cpu_cores: 8,
                accelerators: vec![InferenceAccelerator::Cpu],
                model_storage_path: "/tmp/models".to_owned(),
                model_storage_available_bytes: 1,
                probe_revision: "test".to_owned(),
            },
            backend_manager,
            gateway.clone(),
            registry,
        );
        ExternalProfileFixture {
            manager,
            adapter,
            database,
            gateway,
            engine_root,
        }
    }

    /// Fixture for an OpenAI-compatible engine that has a qualified deployment identity but no
    /// package-version endpoint. The manifest is marked formal here so this test exercises the
    /// post-promotion runtime-profile path without claiming that the real MLC-LM support cell has
    /// already been promoted in the checked-in acceptance ledger.
    fn deployment_fingerprint_profile_fixture() -> ExternalProfileFixture {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        database
            .upsert_backend(&StoredBackendRecord {
                id: "saved-mlc".to_owned(),
                display_name: "本机 MLC LLM".to_owned(),
                kind: "external_openai".to_owned(),
                engine_kind: Some(InferenceEngineKind::MlcLlm.storage_key().to_owned()),
                adapter_variant: Some("official-openai-metal".to_owned()),
                api_root: "http://127.0.0.1:8000/v1/".to_owned(),
                auth_style: "none".to_owned(),
                credential_id: None,
                enabled: true,
                created_at_ms: 1,
                updated_at_ms: 1,
            })
            .expect("stored MLC backend");
        let gateway = GatewayState::new(
            None,
            CredentialRegistry::new(Vec::new()),
            UsageWriter::start(database.clone()),
        )
        .expect("gateway");
        let backend_manager = Arc::new(BackendManager::new(
            database.clone(),
            gateway.clone(),
            Arc::new(MemorySecretStore::default()),
        ));
        backend_manager.restore().expect("restore backend");
        let engine_root = std::env::temp_dir().join(format!(
            "hal100-runtime-profile-fingerprint-test-{}",
            Uuid::new_v4().simple()
        ));
        let engine = Arc::new(
            LlamaCppManager::new(database.clone(), gateway.clone(), engine_root.clone())
                .expect("managed engine"),
        );
        let manifest = InferenceEngineManifest {
            adapter_id: EngineAdapterId {
                engine: InferenceEngineKind::MlcLlm,
                variant: "official-openai-metal".to_owned(),
                contract_revision: ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
            },
            descriptor: InferenceEngineDescriptor {
                kind: InferenceEngineKind::MlcLlm,
                display_name: "测试 MLC LLM".to_owned(),
                ownership: InferenceEngineOwnership::External,
                deployment: InferenceDeployment::Local,
                protocols: vec![InferenceProtocol::OpenAi],
                platforms: vec![InferencePlatform::MacOs],
                architectures: vec![InferenceArchitecture::Aarch64],
                accelerators: vec![InferenceAccelerator::Metal],
                model_formats: vec![InferenceModelFormat::Mlc],
                managed_lifecycle: false,
            },
            support_units: vec![InferenceEngineSupportUnit {
                platform: InferencePlatform::MacOs,
                architecture: InferenceArchitecture::Aarch64,
                accelerator: InferenceAccelerator::Metal,
                deployment: InferenceDeployment::Local,
                status: InferenceEngineSupportStatus::VerifiedExternal,
                evidence: Some(crate::support_evidence_for(
                    InferenceEngineKind::MlcLlm,
                    Some(InferenceEngineSupportStatus::VerifiedExternal),
                )),
            }],
        };
        let model_id = "Qwen3-0.6B-MLC";
        let qualification = EngineQualificationReport {
            adapter_id: manifest.adapter_id.clone(),
            model_id: model_id.to_owned(),
            protocol_capabilities: crate::mlc_llm_agent_protocol_capabilities(),
            protocol_capability_hash: crate::mlc_llm_agent_protocol_capability_hash(),
            observed_engine_version: None,
            runtime_device_evidence: EngineRuntimeDeviceEvidence::AdapterVariantContract {
                accelerator: InferenceAccelerator::Metal,
            },
            deployment_fingerprint: Some("a".repeat(64)),
        };
        let adapter = Arc::new(FakeExternalAdapter {
            manifest,
            snapshot: Mutex::new(ExternalEngineSnapshot {
                engine: InferenceEngineKind::MlcLlm,
                display_name: "本机 MLC LLM".to_owned(),
                api_root: "http://127.0.0.1:8000/v1/".to_owned(),
                version: ENGINE_VERSION_NOT_EXPOSED.to_owned(),
                engine_version_exact: false,
                models: vec![ExternalEngineModelSummary {
                    name: model_id.to_owned(),
                    digest: "catalog-model-v1".to_owned(),
                    size_bytes: 1_000_000_000,
                    format: "mlc".to_owned(),
                    family: Some("qwen3".to_owned()),
                    parameter_size: Some("0.6B".to_owned()),
                    quantization: Some("q4f16_1".to_owned()),
                    evidence: RuntimeProfileEvidence {
                        kind: RuntimeProfileEvidenceKind::CatalogIdentity,
                        algorithm: "catalog-identity-v1".to_owned(),
                        value: "catalog-model-v1".to_owned(),
                    },
                }],
                model_catalog_complete: true,
            }),
            qualification: Some(qualification),
            inspections: AtomicUsize::new(0),
            drift_on_inspection: Mutex::new(None),
            qualification_unavailable: AtomicBool::new(false),
        });
        let registry = Arc::new(
            ExternalInferenceEngineRegistry::new(vec![adapter.clone()]).expect("external registry"),
        );
        let manager = RuntimeProfileManager::with_external_context(
            database.clone(),
            engine,
            HostCapabilitySnapshot {
                platform: InferencePlatform::MacOs,
                architecture: InferenceArchitecture::Aarch64,
                cpu_brand: "Test".to_owned(),
                device_model: "Test".to_owned(),
                total_memory_bytes: 16 * 1024 * 1024 * 1024,
                physical_cpu_cores: 8,
                logical_cpu_cores: 8,
                accelerators: vec![InferenceAccelerator::Metal],
                model_storage_path: "/tmp/models".to_owned(),
                model_storage_available_bytes: 1,
                probe_revision: "test".to_owned(),
            },
            backend_manager,
            gateway.clone(),
            registry,
        );
        ExternalProfileFixture {
            manager,
            adapter,
            database,
            gateway,
            engine_root,
        }
    }

    struct CrossEngineProfileFixture {
        manager: RuntimeProfileManager,
        ollama: Arc<FakeExternalAdapter>,
        mlc: Arc<FakeExternalAdapter>,
        database: Arc<Database>,
        gateway: GatewayState,
        engine_root: PathBuf,
    }

    impl Drop for CrossEngineProfileFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.engine_root);
        }
    }

    /// Two formal manifests are intentionally scoped to this control-plane fixture. They prove
    /// adapter isolation and rollback semantics, not real MLC platform support or promotion.
    fn cross_engine_profile_fixture() -> CrossEngineProfileFixture {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        for backend in [
            StoredBackendRecord {
                id: "saved-ollama".to_owned(),
                display_name: "本机 Ollama".to_owned(),
                kind: "external_ollama".to_owned(),
                engine_kind: None,
                adapter_variant: None,
                api_root: "http://127.0.0.1:11434/v1/".to_owned(),
                auth_style: "none".to_owned(),
                credential_id: None,
                enabled: true,
                created_at_ms: 1,
                updated_at_ms: 1,
            },
            StoredBackendRecord {
                id: "saved-mlc".to_owned(),
                display_name: "本机 MLC LLM".to_owned(),
                kind: "external_openai".to_owned(),
                engine_kind: Some(InferenceEngineKind::MlcLlm.storage_key().to_owned()),
                adapter_variant: Some("official-openai-metal".to_owned()),
                api_root: "http://127.0.0.1:8000/v1/".to_owned(),
                auth_style: "none".to_owned(),
                credential_id: None,
                enabled: true,
                created_at_ms: 2,
                updated_at_ms: 2,
            },
        ] {
            database.upsert_backend(&backend).expect("stored backend");
        }
        let gateway = GatewayState::new(
            None,
            CredentialRegistry::new(Vec::new()),
            UsageWriter::start(database.clone()),
        )
        .expect("gateway");
        let backend_manager = Arc::new(BackendManager::new(
            database.clone(),
            gateway.clone(),
            Arc::new(MemorySecretStore::default()),
        ));
        backend_manager.restore().expect("restore backends");
        let engine_root = std::env::temp_dir().join(format!(
            "hal100-runtime-profile-cross-engine-test-{}",
            Uuid::new_v4().simple()
        ));
        let engine = Arc::new(
            LlamaCppManager::new(database.clone(), gateway.clone(), engine_root.clone())
                .expect("managed engine"),
        );

        let ollama_adapter_id = EngineAdapterId {
            engine: InferenceEngineKind::Ollama,
            variant: "official-loopback-api".to_owned(),
            contract_revision: ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
        };
        let ollama = Arc::new(FakeExternalAdapter {
            manifest: InferenceEngineManifest {
                adapter_id: ollama_adapter_id.clone(),
                descriptor: InferenceEngineDescriptor {
                    kind: InferenceEngineKind::Ollama,
                    display_name: "测试 Ollama".to_owned(),
                    ownership: InferenceEngineOwnership::External,
                    deployment: InferenceDeployment::Local,
                    protocols: vec![InferenceProtocol::OpenAi, InferenceProtocol::Ollama],
                    platforms: vec![InferencePlatform::MacOs],
                    architectures: vec![InferenceArchitecture::Aarch64],
                    accelerators: vec![InferenceAccelerator::Cpu],
                    model_formats: vec![InferenceModelFormat::Gguf],
                    managed_lifecycle: false,
                },
                support_units: vec![InferenceEngineSupportUnit {
                    platform: InferencePlatform::MacOs,
                    architecture: InferenceArchitecture::Aarch64,
                    accelerator: InferenceAccelerator::Cpu,
                    deployment: InferenceDeployment::Local,
                    status: InferenceEngineSupportStatus::VerifiedExternal,
                    evidence: Some(crate::support_evidence_for(
                        InferenceEngineKind::Ollama,
                        Some(InferenceEngineSupportStatus::VerifiedExternal),
                    )),
                }],
            },
            snapshot: Mutex::new(ExternalEngineSnapshot {
                engine: InferenceEngineKind::Ollama,
                display_name: "本机 Ollama".to_owned(),
                api_root: "http://127.0.0.1:11434/v1/".to_owned(),
                version: "0.12.6-test".to_owned(),
                engine_version_exact: true,
                models: vec![ExternalEngineModelSummary {
                    name: "qwen3:8b".to_owned(),
                    digest: "a".repeat(64),
                    size_bytes: 4_000_000_000,
                    format: "gguf".to_owned(),
                    family: Some("qwen3".to_owned()),
                    parameter_size: Some("8B".to_owned()),
                    quantization: Some("Q4_K_M".to_owned()),
                    evidence: RuntimeProfileEvidence {
                        kind: RuntimeProfileEvidenceKind::ContentDigest,
                        algorithm: "ollama-digest".to_owned(),
                        value: "a".repeat(64),
                    },
                }],
                model_catalog_complete: true,
            }),
            qualification: Some(EngineQualificationReport {
                adapter_id: ollama_adapter_id,
                model_id: "qwen3:8b".to_owned(),
                protocol_capabilities: crate::ollama_agent_protocol_capabilities(),
                protocol_capability_hash: crate::ollama_agent_protocol_capability_hash(),
                observed_engine_version: Some("0.12.6-test".to_owned()),
                runtime_device_evidence: EngineRuntimeDeviceEvidence::ModelResidencyObservation {
                    accelerator: InferenceAccelerator::Cpu,
                },
                deployment_fingerprint: None,
            }),
            inspections: AtomicUsize::new(0),
            drift_on_inspection: Mutex::new(None),
            qualification_unavailable: AtomicBool::new(false),
        });

        let mlc_adapter_id = EngineAdapterId {
            engine: InferenceEngineKind::MlcLlm,
            variant: "official-openai-metal".to_owned(),
            contract_revision: ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
        };
        let mlc = Arc::new(FakeExternalAdapter {
            manifest: InferenceEngineManifest {
                adapter_id: mlc_adapter_id.clone(),
                descriptor: InferenceEngineDescriptor {
                    kind: InferenceEngineKind::MlcLlm,
                    display_name: "测试 MLC LLM".to_owned(),
                    ownership: InferenceEngineOwnership::External,
                    deployment: InferenceDeployment::Local,
                    protocols: vec![InferenceProtocol::OpenAi],
                    platforms: vec![InferencePlatform::MacOs],
                    architectures: vec![InferenceArchitecture::Aarch64],
                    accelerators: vec![InferenceAccelerator::Metal],
                    model_formats: vec![InferenceModelFormat::Mlc],
                    managed_lifecycle: false,
                },
                support_units: vec![InferenceEngineSupportUnit {
                    platform: InferencePlatform::MacOs,
                    architecture: InferenceArchitecture::Aarch64,
                    accelerator: InferenceAccelerator::Metal,
                    deployment: InferenceDeployment::Local,
                    status: InferenceEngineSupportStatus::VerifiedExternal,
                    evidence: Some(crate::support_evidence_for(
                        InferenceEngineKind::MlcLlm,
                        Some(InferenceEngineSupportStatus::VerifiedExternal),
                    )),
                }],
            },
            snapshot: Mutex::new(ExternalEngineSnapshot {
                engine: InferenceEngineKind::MlcLlm,
                display_name: "本机 MLC LLM".to_owned(),
                api_root: "http://127.0.0.1:8000/v1/".to_owned(),
                version: ENGINE_VERSION_NOT_EXPOSED.to_owned(),
                engine_version_exact: false,
                models: vec![ExternalEngineModelSummary {
                    name: "Qwen3-0.6B-MLC".to_owned(),
                    digest: "catalog-model-v1".to_owned(),
                    size_bytes: 1_000_000_000,
                    format: "mlc".to_owned(),
                    family: Some("qwen3".to_owned()),
                    parameter_size: Some("0.6B".to_owned()),
                    quantization: Some("q4f16_1".to_owned()),
                    evidence: RuntimeProfileEvidence {
                        kind: RuntimeProfileEvidenceKind::CatalogIdentity,
                        algorithm: "catalog-identity-v1".to_owned(),
                        value: "catalog-model-v1".to_owned(),
                    },
                }],
                model_catalog_complete: true,
            }),
            qualification: Some(EngineQualificationReport {
                adapter_id: mlc_adapter_id,
                model_id: "Qwen3-0.6B-MLC".to_owned(),
                protocol_capabilities: crate::mlc_llm_agent_protocol_capabilities(),
                protocol_capability_hash: crate::mlc_llm_agent_protocol_capability_hash(),
                observed_engine_version: None,
                runtime_device_evidence: EngineRuntimeDeviceEvidence::AdapterVariantContract {
                    accelerator: InferenceAccelerator::Metal,
                },
                deployment_fingerprint: Some("b".repeat(64)),
            }),
            inspections: AtomicUsize::new(0),
            drift_on_inspection: Mutex::new(None),
            qualification_unavailable: AtomicBool::new(false),
        });
        let registry = Arc::new(
            ExternalInferenceEngineRegistry::new(vec![ollama.clone(), mlc.clone()])
                .expect("cross-engine registry"),
        );
        let manager = RuntimeProfileManager::with_external_context(
            database.clone(),
            engine,
            HostCapabilitySnapshot {
                platform: InferencePlatform::MacOs,
                architecture: InferenceArchitecture::Aarch64,
                cpu_brand: "Test".to_owned(),
                device_model: "Test".to_owned(),
                total_memory_bytes: 16 * 1024 * 1024 * 1024,
                physical_cpu_cores: 8,
                logical_cpu_cores: 8,
                accelerators: vec![InferenceAccelerator::Cpu, InferenceAccelerator::Metal],
                model_storage_path: "/tmp/models".to_owned(),
                model_storage_available_bytes: 1,
                probe_revision: "test".to_owned(),
            },
            backend_manager,
            gateway.clone(),
            registry,
        );
        CrossEngineProfileFixture {
            manager,
            ollama,
            mlc,
            database,
            gateway,
            engine_root,
        }
    }

    async fn save_fixture_profile(fixture: &ExternalProfileFixture) -> RuntimeProfileCatalog {
        fixture
            .manager
            .save_external(ExternalRuntimeProfileDraft {
                name: "外部代码助手".to_owned(),
                description: "已验证 Ollama 组合".to_owned(),
                backend_id: "saved-ollama".to_owned(),
                model_id: "qwen3:8b".to_owned(),
                expected_evidence: RuntimeProfileEvidence {
                    kind: RuntimeProfileEvidenceKind::ContentDigest,
                    algorithm: "ollama-digest".to_owned(),
                    value: "a".repeat(64),
                },
                support_cell: None,
            })
            .await
            .expect("save external profile")
    }

    #[tokio::test]
    async fn capability_catalog_observes_saved_instances_independently_and_deduplicates_default() {
        let fixture = external_profile_fixture();
        fixture
            .database
            .upsert_backend(&StoredBackendRecord {
                id: "saved-ollama-second".to_owned(),
                display_name: "第二个 Ollama".to_owned(),
                kind: "external_ollama".to_owned(),
                engine_kind: None,
                adapter_variant: None,
                api_root: "http://127.0.0.1:21434/v1/".to_owned(),
                auth_style: "none".to_owned(),
                credential_id: None,
                enabled: true,
                created_at_ms: 2,
                updated_at_ms: 2,
            })
            .expect("second stored backend");
        let host = fixture.manager.host_capabilities.clone().expect("host");

        let capabilities = fixture
            .manager
            .external_engine_capabilities(&host)
            .await
            .expect("capabilities");

        assert_eq!(capabilities.len(), 1);
        assert!(capabilities[0].compatibility.compatible);
        assert_eq!(capabilities[0].external_runtimes.len(), 2);
        assert_eq!(
            capabilities[0]
                .external_runtimes
                .iter()
                .map(|runtime| runtime.api_root.as_str())
                .collect::<Vec<_>>(),
            vec!["http://127.0.0.1:11434/v1/", "http://127.0.0.1:21434/v1/"]
        );
    }

    #[test]
    fn manager_projects_managed_and_external_adapters_through_one_manifest_registry() {
        let fixture = external_profile_fixture();
        let manifests = fixture.manager.manifest_registry().manifests();

        assert_eq!(manifests.len(), 2);
        assert_eq!(
            manifests[0].adapter_id.engine,
            InferenceEngineKind::LlamaCpp
        );
        assert_eq!(manifests[1].adapter_id.engine, InferenceEngineKind::Ollama);
        assert_eq!(
            manifests[1].support_units[0].status,
            InferenceEngineSupportStatus::VerifiedExternal
        );
    }

    #[test]
    fn expected_protocol_hash_is_specific_to_each_registered_engine() {
        let cases = [
            (
                InferenceEngineKind::Ollama,
                "official-loopback-api",
                crate::ollama_agent_protocol_capability_hash(),
            ),
            (
                InferenceEngineKind::Vllm,
                "official-openai-server",
                crate::vllm_agent_protocol_capability_hash(),
            ),
            (
                InferenceEngineKind::MlxLm,
                "official-http-server",
                crate::mlx_lm_agent_protocol_capability_hash(),
            ),
            (
                InferenceEngineKind::MlcLlm,
                "official-openai-metal",
                crate::mlc_llm_agent_protocol_capability_hash(),
            ),
            (
                InferenceEngineKind::MlcLlm,
                "official-openai-vulkan",
                crate::mlc_llm_agent_protocol_capability_hash(),
            ),
            (
                InferenceEngineKind::MlcLlm,
                "official-openai-cuda",
                crate::mlc_llm_agent_protocol_capability_hash(),
            ),
            (
                InferenceEngineKind::MlcLlm,
                "official-openai-rocm",
                crate::mlc_llm_agent_protocol_capability_hash(),
            ),
            (
                InferenceEngineKind::OpenVino,
                "ovms-openai-cpu",
                crate::openvino_agent_protocol_capability_hash(),
            ),
            (
                InferenceEngineKind::OpenVino,
                "ovms-openai-intel-gpu",
                crate::openvino_agent_protocol_capability_hash(),
            ),
            (
                InferenceEngineKind::OpenVino,
                "ovms-openai-intel-npu",
                crate::openvino_agent_protocol_capability_hash(),
            ),
            (
                InferenceEngineKind::Sglang,
                "official-openai-server",
                crate::sglang_agent_protocol_capability_hash(),
            ),
            (
                InferenceEngineKind::LmDeploy,
                "official-openai-server",
                crate::lmdeploy_agent_protocol_capability_hash(),
            ),
            (
                InferenceEngineKind::TensorRtLlm,
                "trtllm-serve-openai-server",
                crate::tensorrt_llm_agent_protocol_capability_hash(),
            ),
            (
                InferenceEngineKind::LlamaCpp,
                "hal100-managed-metal",
                OPENAI_CORE_CAPABILITY_HASH.to_owned(),
            ),
        ];
        for (engine, variant, expected) in cases {
            let adapter_id = EngineAdapterId {
                engine,
                variant: variant.to_owned(),
                contract_revision: ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
            };
            assert_eq!(
                expected_protocol_capability_hash(&adapter_id),
                Some(expected)
            );
        }
        let unknown_variant = EngineAdapterId {
            engine: InferenceEngineKind::Vllm,
            variant: "community-openai-server".to_owned(),
            contract_revision: ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
        };
        assert!(expected_protocol_capability_hash(&unknown_variant).is_none());
        let unknown_contract = EngineAdapterId {
            engine: InferenceEngineKind::Vllm,
            variant: "official-openai-server".to_owned(),
            contract_revision: "engine-contract-v2".to_owned(),
        };
        assert!(expected_protocol_capability_hash(&unknown_contract).is_none());
    }

    #[tokio::test]
    async fn external_profile_save_plan_apply_and_recheck_form_a_closed_loop() {
        let fixture = external_profile_fixture();
        let catalog = save_fixture_profile(&fixture).await;
        let profile = &catalog.profiles[0];
        assert_eq!(profile.ownership, InferenceEngineOwnership::External);
        assert_eq!(profile.context_window_tokens, None);
        assert_eq!(profile.readiness, RuntimeProfileReadiness::Ready);
        assert!(matches!(
            fixture.manager.plan_activation(&profile.id),
            Err(RuntimeProfileManagerError::ExternalVerificationRequired)
        ));

        let plan = fixture
            .manager
            .plan_activation_verified(&profile.id)
            .await
            .expect("verified plan");
        assert!(plan.requires_confirmation);
        assert_eq!(plan.backend_id.as_deref(), Some("saved-ollama"));
        assert_eq!(plan.context_window_tokens, None);
        let result = fixture
            .manager
            .apply_activation(&plan.plan_id)
            .await
            .expect("activate external profile");
        assert_eq!(result.ownership, InferenceEngineOwnership::External);
        assert_eq!(result.active_backend_id.as_deref(), Some("saved-ollama"));
        assert_eq!(result.active_model_id, "qwen3:8b");
        assert!(result.managed_runtime.is_none());
        assert!(
            fixture
                .database
                .runtime_activation_journals()
                .expect("activation journal")
                .is_empty()
        );
        assert!(
            fixture
                .manager
                .verify_active_profile(&profile.id)
                .await
                .expect("live active profile proof")
        );
        fixture.adapter.set_digest("b".repeat(64));
        assert!(
            !fixture
                .manager
                .verify_active_profile(&profile.id)
                .await
                .expect("live drift proof")
        );
        let verified_catalog = fixture
            .manager
            .catalog_verified()
            .await
            .expect("verified catalog after drift");
        assert_eq!(
            verified_catalog.profiles[0].readiness,
            RuntimeProfileReadiness::NeedsVerification
        );
        assert!(
            verified_catalog.profiles[0]
                .issues
                .contains(&RuntimeProfileIssue::ModelIntegrityChanged)
        );
        assert!(verified_catalog.active_profile_id.is_none());
        let reverified = fixture
            .manager
            .reverify_external(&profile.id)
            .await
            .expect("explicitly reverify external snapshot");
        assert_eq!(
            reverified.active_profile_id.as_deref(),
            Some(profile.id.as_str())
        );
        assert_eq!(
            reverified.profiles[0].readiness,
            RuntimeProfileReadiness::Active
        );
        assert!(reverified.profiles[0].issues.is_empty());
        assert!(
            fixture
                .manager
                .verify_active_profile(&profile.id)
                .await
                .expect("reverified active profile proof")
        );
        assert_eq!(
            result.catalog.active_profile_id.as_deref(),
            Some(profile.id.as_str())
        );
        let routing = fixture.gateway.routing_snapshot();
        assert_eq!(routing.active_backend_id.as_deref(), Some("saved-ollama"));
        assert_eq!(routing.active_resolved_model.as_deref(), Some("qwen3:8b"));
        assert_eq!(
            fixture
                .database
                .active_gateway_route()
                .expect("persisted active route")
                .expect("active route")
                .resolved_model
                .as_deref(),
            Some("qwen3:8b")
        );
    }

    #[tokio::test]
    async fn external_ollama_profile_save_requires_live_qualification() {
        let fixture = external_profile_fixture();
        fixture.adapter.disable_qualification();

        let result = fixture
            .manager
            .save_external(ExternalRuntimeProfileDraft {
                name: "无实时资格的 Ollama 方案".to_owned(),
                description: "资格请求不可用时不得保存".to_owned(),
                backend_id: "saved-ollama".to_owned(),
                model_id: "qwen3:8b".to_owned(),
                expected_evidence: RuntimeProfileEvidence {
                    kind: RuntimeProfileEvidenceKind::ContentDigest,
                    algorithm: "ollama-digest".to_owned(),
                    value: "a".repeat(64),
                },
                support_cell: None,
            })
            .await;

        assert!(matches!(
            result,
            Err(RuntimeProfileManagerError::ExternalEngine(
                ExternalEngineAdapterError::QualificationUnavailable
            ))
        ));
        assert!(
            fixture
                .database
                .runtime_profiles()
                .expect("runtime profiles")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn deployment_fingerprint_profile_saves_activates_and_verifies_without_package_version() {
        let fixture = deployment_fingerprint_profile_fixture();
        let catalog = fixture
            .manager
            .save_external(ExternalRuntimeProfileDraft {
                name: "MLC 部署身份方案".to_owned(),
                description: "无包版本、由部署指纹绑定".to_owned(),
                backend_id: "saved-mlc".to_owned(),
                model_id: "Qwen3-0.6B-MLC".to_owned(),
                expected_evidence: RuntimeProfileEvidence {
                    kind: RuntimeProfileEvidenceKind::CatalogIdentity,
                    algorithm: "catalog-identity-v1".to_owned(),
                    value: "catalog-model-v1".to_owned(),
                },
                support_cell: None,
            })
            .await
            .expect("save deployment fingerprint profile");
        let profile = &catalog.profiles[0];
        assert_eq!(profile.engine, InferenceEngineKind::MlcLlm.storage_key());
        assert_eq!(profile.engine_version, ENGINE_VERSION_NOT_EXPOSED);
        assert_eq!(profile.readiness, RuntimeProfileReadiness::Ready);
        assert_eq!(
            profile.evidence,
            RuntimeProfileEvidence {
                kind: RuntimeProfileEvidenceKind::DeploymentFingerprint,
                algorithm: "engine-deployment-fingerprint-v1".to_owned(),
                value: "a".repeat(64),
            }
        );
        let stored = fixture
            .database
            .runtime_profiles()
            .expect("stored profile")
            .into_iter()
            .next()
            .expect("profile record");
        assert_eq!(stored.engine_version, ENGINE_VERSION_NOT_EXPOSED);
        assert_eq!(stored.model_digest, "a".repeat(64));
        assert_eq!(stored.evidence_kind, "deployment_fingerprint");

        let plan = fixture
            .manager
            .plan_activation_verified(&profile.id)
            .await
            .expect("verified fingerprint plan");
        assert_eq!(plan.engine_version, ENGINE_VERSION_NOT_EXPOSED);
        assert_eq!(plan.backend_id.as_deref(), Some("saved-mlc"));
        let result = fixture
            .manager
            .apply_activation(&plan.plan_id)
            .await
            .expect("activate fingerprint profile");
        assert_eq!(result.ownership, InferenceEngineOwnership::External);
        assert_eq!(result.active_backend_id.as_deref(), Some("saved-mlc"));
        assert_eq!(result.active_model_id, "Qwen3-0.6B-MLC");
        assert!(result.managed_runtime.is_none());
        assert!(
            fixture
                .manager
                .verify_active_profile(&profile.id)
                .await
                .expect("live fingerprint proof")
        );
        assert!(
            fixture
                .database
                .runtime_activation_journals()
                .expect("activation journal")
                .is_empty()
        );
        let routing = fixture.gateway.routing_snapshot();
        assert_eq!(routing.active_backend_id.as_deref(), Some("saved-mlc"));
        assert_eq!(
            routing.active_resolved_model.as_deref(),
            Some("Qwen3-0.6B-MLC")
        );
    }

    #[tokio::test]
    async fn verified_catalog_batches_profiles_by_verified_engine_target() {
        let fixture = external_profile_fixture();
        fixture
            .adapter
            .snapshot
            .lock()
            .expect("snapshot lock")
            .models
            .push(ExternalEngineModelSummary {
                name: "qwen3:14b".to_owned(),
                digest: "d".repeat(64),
                size_bytes: 8_000_000_000,
                format: "gguf".to_owned(),
                family: Some("qwen3".to_owned()),
                parameter_size: Some("14B".to_owned()),
                quantization: Some("Q4_K_M".to_owned()),
                evidence: RuntimeProfileEvidence {
                    kind: RuntimeProfileEvidenceKind::ContentDigest,
                    algorithm: "ollama-digest".to_owned(),
                    value: "d".repeat(64),
                },
            });
        save_fixture_profile(&fixture).await;
        fixture
            .manager
            .save_external(ExternalRuntimeProfileDraft {
                name: "外部分析助手".to_owned(),
                description: "第二个已验证模型".to_owned(),
                backend_id: "saved-ollama".to_owned(),
                model_id: "qwen3:14b".to_owned(),
                expected_evidence: RuntimeProfileEvidence {
                    kind: RuntimeProfileEvidenceKind::ContentDigest,
                    algorithm: "ollama-digest".to_owned(),
                    value: "d".repeat(64),
                },
                support_cell: None,
            })
            .await
            .expect("save second profile");
        let stored = fixture
            .database
            .runtime_profiles()
            .expect("stored profiles")
            .into_iter()
            .next()
            .expect("stored profile");
        let registry = fixture
            .manager
            .external_engines
            .as_ref()
            .expect("external registry");
        let target = fixture
            .manager
            .verified_target_for_profile(&stored, registry)
            .expect("verified target");
        fixture
            .manager
            .external_observations
            .as_ref()
            .expect("observation service")
            .invalidate(&target)
            .await;
        let before = fixture.adapter.inspection_count();

        let catalog = fixture
            .manager
            .catalog_verified()
            .await
            .expect("verified catalog");

        assert_eq!(catalog.profiles.len(), 2);
        assert_eq!(fixture.adapter.inspection_count(), before + 1);
        assert!(
            catalog
                .profiles
                .iter()
                .all(|profile| profile.readiness == RuntimeProfileReadiness::Ready)
        );
    }

    #[tokio::test]
    async fn concurrent_external_profile_saves_serialize_and_keep_one_identity() {
        let fixture = external_profile_fixture();
        let draft = || ExternalRuntimeProfileDraft {
            name: "外部代码助手".to_owned(),
            description: "已验证 Ollama 组合".to_owned(),
            backend_id: "saved-ollama".to_owned(),
            model_id: "qwen3:8b".to_owned(),
            expected_evidence: RuntimeProfileEvidence {
                kind: RuntimeProfileEvidenceKind::ContentDigest,
                algorithm: "ollama-digest".to_owned(),
                value: "a".repeat(64),
            },
            support_cell: None,
        };

        let (left, right) = tokio::join!(
            fixture.manager.save_external(draft()),
            fixture.manager.save_external(draft())
        );
        assert!(left.is_ok() ^ right.is_ok());
        let error = left.err().or_else(|| right.err()).expect("duplicate error");
        assert!(matches!(
            error,
            RuntimeProfileManagerError::DuplicateProfile
        ));
        assert_eq!(
            fixture
                .database
                .runtime_profiles()
                .expect("stored profiles")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn external_digest_drift_after_plan_invalidates_authority_without_switching() {
        let fixture = external_profile_fixture();
        let catalog = save_fixture_profile(&fixture).await;
        let plan = fixture
            .manager
            .plan_activation_verified(&catalog.profiles[0].id)
            .await
            .expect("verified plan");
        fixture.adapter.set_digest("b".repeat(64));

        assert!(matches!(
            fixture.manager.apply_activation(&plan.plan_id).await,
            Err(RuntimeProfileManagerError::ProfileChanged)
        ));
        assert!(
            fixture
                .gateway
                .routing_snapshot()
                .active_backend_id
                .is_none()
        );
        assert!(
            fixture
                .database
                .active_gateway_route()
                .expect("active route")
                .is_none()
        );
    }

    #[tokio::test]
    async fn support_cell_drift_after_plan_invalidates_authority_without_switching() {
        let fixture = external_profile_fixture();
        let catalog = save_fixture_profile(&fixture).await;
        let plan = fixture
            .manager
            .plan_activation_verified(&catalog.profiles[0].id)
            .await
            .expect("verified plan");
        let stored = fixture
            .database
            .runtime_profile(&catalog.profiles[0].id)
            .expect("stored profile")
            .expect("profile");
        let drifted = RuntimeProfileSupportCell {
            platform: InferencePlatform::MacOs,
            architecture: InferenceArchitecture::Aarch64,
            accelerator: InferenceAccelerator::Metal,
            deployment: InferenceDeployment::Local,
        };
        assert_ne!(stored.support_cell, Some(drifted));
        fixture
            .database
            .reverify_runtime_profile(
                &stored.id,
                &StoredRuntimeProfileVerification {
                    model_digest: stored.model_digest.clone(),
                    evidence_kind: stored.evidence_kind.clone(),
                    evidence_algorithm: stored.evidence_algorithm.clone(),
                    evidence_value: stored.evidence_value.clone(),
                    engine_version: stored.engine_version.clone(),
                    capacity_tier: stored.capacity_tier.clone(),
                    context_window_tokens: stored.context_window_tokens,
                    capacity_revision: stored.capacity_revision.clone(),
                    support_cell: Some(drifted),
                },
                stored.updated_at_ms,
            )
            .expect("persist support-cell drift");

        assert!(matches!(
            fixture.manager.apply_activation(&plan.plan_id).await,
            Err(RuntimeProfileManagerError::ProfileChanged)
        ));
        assert!(
            fixture
                .gateway
                .routing_snapshot()
                .active_backend_id
                .is_none()
        );
        assert!(
            fixture
                .database
                .runtime_activation_journals()
                .expect("activation journal")
                .is_empty()
        );
        assert_eq!(
            fixture
                .database
                .runtime_profile(&stored.id)
                .expect("stored profile")
                .expect("profile")
                .support_cell,
            Some(drifted)
        );
    }

    #[tokio::test]
    async fn route_drift_after_plan_invalidates_the_bound_activation_authority() {
        let fixture = external_profile_fixture();
        let catalog = save_fixture_profile(&fixture).await;
        let plan = fixture
            .manager
            .plan_activation_verified(&catalog.profiles[0].id)
            .await
            .expect("verified plan");
        fixture
            .manager
            .backend_manager
            .as_ref()
            .expect("backend manager")
            .activate_resolved_backend("saved-ollama", "other-model")
            .await
            .expect("external route drift");

        assert!(matches!(
            fixture.manager.apply_activation(&plan.plan_id).await,
            Err(RuntimeProfileManagerError::ProfileChanged)
        ));
        assert_eq!(
            fixture
                .gateway
                .routing_snapshot()
                .active_resolved_model
                .as_deref(),
            Some("other-model")
        );
        assert!(
            fixture
                .database
                .runtime_activation_journals()
                .expect("activation journal")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn external_identity_drift_before_plan_is_not_silently_reverified() {
        let fixture = external_profile_fixture();
        let catalog = save_fixture_profile(&fixture).await;
        fixture.adapter.set_digest("b".repeat(64));

        assert!(matches!(
            fixture
                .manager
                .plan_activation_verified(&catalog.profiles[0].id)
                .await,
            Err(RuntimeProfileManagerError::ProfileChanged)
        ));
        assert!(
            fixture
                .gateway
                .routing_snapshot()
                .active_backend_id
                .is_none()
        );
    }

    #[tokio::test]
    async fn failed_post_switch_recheck_restores_the_previous_route() {
        let fixture = external_profile_fixture();
        let catalog = save_fixture_profile(&fixture).await;
        let plan = fixture
            .manager
            .plan_activation_verified(&catalog.profiles[0].id)
            .await
            .expect("verified plan");
        fixture.adapter.set_drift_on_inspection(4, "c".repeat(64));

        assert!(matches!(
            fixture.manager.apply_activation(&plan.plan_id).await,
            Err(RuntimeProfileManagerError::ActivationFailed {
                rollback_restored: true
            })
        ));
        assert!(
            fixture
                .gateway
                .routing_snapshot()
                .active_backend_id
                .is_none()
        );
        assert!(
            fixture
                .database
                .active_gateway_route()
                .expect("active route")
                .is_none()
        );
        assert!(
            fixture
                .database
                .runtime_activation_journals()
                .expect("activation journal")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn failed_cross_engine_switch_restores_the_exact_previous_engine_route() {
        let fixture = cross_engine_profile_fixture();
        fixture
            .manager
            .save_external(ExternalRuntimeProfileDraft {
                name: "MLC Metal 方案".to_owned(),
                description: "跨引擎回滚的原活动方案".to_owned(),
                backend_id: "saved-mlc".to_owned(),
                model_id: "Qwen3-0.6B-MLC".to_owned(),
                expected_evidence: RuntimeProfileEvidence {
                    kind: RuntimeProfileEvidenceKind::CatalogIdentity,
                    algorithm: "catalog-identity-v1".to_owned(),
                    value: "catalog-model-v1".to_owned(),
                },
                support_cell: None,
            })
            .await
            .expect("save MLC profile");
        let catalog = fixture
            .manager
            .save_external(ExternalRuntimeProfileDraft {
                name: "Ollama CPU 方案".to_owned(),
                description: "跨引擎回滚的目标方案".to_owned(),
                backend_id: "saved-ollama".to_owned(),
                model_id: "qwen3:8b".to_owned(),
                expected_evidence: RuntimeProfileEvidence {
                    kind: RuntimeProfileEvidenceKind::ContentDigest,
                    algorithm: "ollama-digest".to_owned(),
                    value: "a".repeat(64),
                },
                support_cell: None,
            })
            .await
            .expect("save Ollama profile");
        let mlc_profile_id = catalog
            .profiles
            .iter()
            .find(|profile| profile.engine == InferenceEngineKind::MlcLlm.storage_key())
            .expect("MLC profile")
            .id
            .clone();
        let ollama_profile_id = catalog
            .profiles
            .iter()
            .find(|profile| profile.engine == InferenceEngineKind::Ollama.storage_key())
            .expect("Ollama profile")
            .id
            .clone();

        let mlc_plan = fixture
            .manager
            .plan_activation_verified(&mlc_profile_id)
            .await
            .expect("plan initial MLC activation");
        fixture
            .manager
            .apply_activation(&mlc_plan.plan_id)
            .await
            .expect("activate initial MLC route");
        assert_eq!(
            fixture
                .gateway
                .routing_snapshot()
                .active_backend_id
                .as_deref(),
            Some("saved-mlc")
        );
        assert_eq!(
            fixture
                .gateway
                .routing_snapshot()
                .active_resolved_model
                .as_deref(),
            Some("Qwen3-0.6B-MLC")
        );

        let mlc_inspections_before_ollama = fixture.mlc.inspection_count();
        let ollama_plan = fixture
            .manager
            .plan_activation_verified(&ollama_profile_id)
            .await
            .expect("plan cross-engine switch");
        assert_eq!(
            fixture.mlc.inspection_count(),
            mlc_inspections_before_ollama
        );
        fixture
            .ollama
            .set_drift_on_inspection(fixture.ollama.inspection_count() + 2, "c".repeat(64));

        let error = fixture
            .manager
            .apply_activation(&ollama_plan.plan_id)
            .await
            .expect_err("post-switch Ollama drift must fail");
        assert!(matches!(
            &error,
            RuntimeProfileManagerError::ActivationFailed {
                rollback_restored: true
            }
        ));
        assert_eq!(
            error.failure(),
            RuntimeProfileFailure::new(
                RuntimeProfileFailureCode::ActivationFailed,
                RuntimeProfileFailureStage::Activation,
                true,
                RuntimeProfileRecoveryAction::Retry,
            )
        );
        assert_eq!(
            fixture.mlc.inspection_count(),
            mlc_inspections_before_ollama
        );
        let routing = fixture.gateway.routing_snapshot();
        assert_eq!(routing.active_backend_id.as_deref(), Some("saved-mlc"));
        assert_eq!(
            routing.active_resolved_model.as_deref(),
            Some("Qwen3-0.6B-MLC")
        );
        let stored_route = fixture
            .database
            .active_gateway_route()
            .expect("stored route")
            .expect("restored route");
        assert_eq!(stored_route.backend_id, "saved-mlc");
        assert_eq!(
            stored_route.resolved_model.as_deref(),
            Some("Qwen3-0.6B-MLC")
        );
        assert!(
            fixture
                .database
                .runtime_activation_journals()
                .expect("activation journals")
                .is_empty()
        );
        assert!(
            fixture
                .manager
                .verify_active_profile(&mlc_profile_id)
                .await
                .expect("previous MLC profile remains active")
        );
        assert!(
            !fixture
                .manager
                .verify_active_profile(&ollama_profile_id)
                .await
                .expect("failed Ollama profile is not active")
        );
    }

    #[tokio::test]
    async fn startup_recovery_compensates_a_durable_route_switch_before_accepting_new_work() {
        let fixture = external_profile_fixture();
        let previous_runtime = fixture.manager.engine.status().expect("managed runtime");
        let mut journal = fixture
            .manager
            .begin_activation_journal("profile-after-crash", None, &previous_runtime)
            .expect("journal before switch");
        fixture
            .manager
            .backend_manager
            .as_ref()
            .expect("backend manager")
            .activate_resolved_backend("saved-ollama", "qwen3:8b")
            .await
            .expect("simulate switched route");
        fixture
            .manager
            .advance_activation_journal(&mut journal, RuntimeActivationPhase::RouteSwitched)
            .expect("persist route switch");

        assert!(
            fixture
                .manager
                .recover_incomplete_activation()
                .await
                .unwrap()
        );
        assert!(
            fixture
                .gateway
                .routing_snapshot()
                .active_backend_id
                .is_none()
        );
        assert!(
            fixture
                .database
                .runtime_activation_journals()
                .expect("activation journal")
                .is_empty()
        );
    }

    #[test]
    fn metadata_validation_is_bounded_and_rejects_control_characters() {
        assert_eq!(
            validate_draft(RuntimeProfileDraft {
                name: " 代码助手 ".to_owned(),
                description: " 已验证 ".to_owned(),
            })
            .expect("valid profile metadata"),
            ("代码助手".to_owned(), "已验证".to_owned())
        );
        assert!(matches!(
            validate_draft(RuntimeProfileDraft {
                name: "bad\0name".to_owned(),
                description: String::new(),
            }),
            Err(RuntimeProfileManagerError::InvalidName)
        ));
        assert!(matches!(
            validate_draft(RuntimeProfileDraft {
                name: "ok".to_owned(),
                description: "x".repeat(MAX_DESCRIPTION_CHARS + 1),
            }),
            Err(RuntimeProfileManagerError::InvalidDescription)
        ));
    }

    #[test]
    fn starting_or_switching_a_profile_requires_native_confirmation() {
        assert!(runtime_change_requires_confirmation(
            EngineRuntimeState::Stopped,
            None,
            "model-a",
        ));
        assert!(runtime_change_requires_confirmation(
            EngineRuntimeState::Running,
            Some("model-a"),
            "model-b",
        ));
        assert!(!runtime_change_requires_confirmation(
            EngineRuntimeState::Running,
            Some("model-a"),
            "model-a",
        ));
    }

    #[test]
    fn active_model_with_a_changed_snapshot_requires_verification() {
        assert_eq!(
            profile_readiness(true, &[RuntimeProfileIssue::CapacityPolicyChanged]),
            RuntimeProfileReadiness::NeedsVerification,
        );
        assert_eq!(
            profile_readiness(true, &[RuntimeProfileIssue::ModelUnavailable]),
            RuntimeProfileReadiness::NeedsRepair,
        );
        assert_eq!(
            profile_readiness(true, &[RuntimeProfileIssue::EngineIncompatible]),
            RuntimeProfileReadiness::NeedsRepair,
        );
        assert_eq!(
            profile_readiness(true, &[]),
            RuntimeProfileReadiness::Active,
        );
    }
}
