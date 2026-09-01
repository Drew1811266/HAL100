use std::{
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::Arc,
    time::Duration,
};

use hal100_protocol::{
    ENGINE_ADAPTER_CONTRACT_REVISION, ENGINE_PROTOCOL_CAPABILITY_REVISION, EngineAdapterId,
    EngineProtocolCapability, EngineProtocolCapabilitySet, EngineQualificationReport,
    EngineRuntimeDeviceEvidence, ExternalEngineModelSummary, ExternalEngineSnapshot,
    HostCapabilitySnapshot, InferenceAccelerator, InferenceArchitecture, InferenceDeployment,
    InferenceEngineDescriptor, InferenceEngineKind, InferenceEngineManifest,
    InferenceEngineOwnership, InferenceEngineSupportEvidenceSummary, InferenceEngineSupportStatus,
    InferenceEngineSupportUnit, InferenceModelFormat, InferencePlatform, InferenceProtocol,
    RuntimeProfileEvidence, RuntimeProfileEvidenceKind, RuntimeProfileReviewedPerformance,
    RuntimeProfileSupportCell,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::openai_protocol_qualification::{
    OpenAiQualificationOptions, OpenAiQualificationReasoningEffort, qualify_openai_agent_protocol,
};
use crate::{
    BoundedEngineHttpClient, EngineHttpError, EngineRequestAuth, InferenceEngineManifestRegistry,
    VerifiedEngineTarget,
};

const OLLAMA_API_ROOT: &str = "http://127.0.0.1:11434/v1/";
const MAX_VERSION_BODY_BYTES: usize = 64 * 1024;
const MAX_TAGS_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_PS_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_EXTERNAL_MODELS: usize = 4096;
const MAX_MODEL_NAME_BYTES: usize = 256;
const MAX_MODEL_DETAIL_BYTES: usize = 128;
const MAX_OLLAMA_MODEL_ID_BYTES: usize = 1024;

pub fn ollama_agent_protocol_capabilities() -> EngineProtocolCapabilitySet {
    protocol_capability_set([
        EngineProtocolCapability::ModelsList,
        EngineProtocolCapability::ChatCompletionsUnary,
        EngineProtocolCapability::ChatCompletionsStream,
        EngineProtocolCapability::UsagePromptCompletion,
        EngineProtocolCapability::ToolCallsSingle,
    ])
}

pub fn ollama_agent_protocol_capability_hash() -> String {
    protocol_capability_hash(&ollama_agent_protocol_capabilities())
}

pub type ExternalEngineInspectionFuture<'a> = Pin<
    Box<
        dyn Future<Output = Result<ExternalEngineSnapshot, ExternalEngineAdapterError>> + Send + 'a,
    >,
>;
pub type ExternalEngineQualificationFuture<'a> = Pin<
    Box<
        dyn Future<Output = Result<EngineQualificationReport, ExternalEngineAdapterError>>
            + Send
            + 'a,
    >,
>;

/// Read-only boundary for a user-owned inference service.
///
/// External adapters may inspect only fixed, validated API endpoints. Engine-specific formal
/// qualification may additionally read a bounded set of identity files below one canonical,
/// explicitly selected local deployment root; it must not enumerate outside that root or mutate
/// any file. Adapters do not receive or expose install, process, arbitrary file, environment,
/// credential, pull, create or delete authority.
pub trait EngineInspector: Send + Sync {
    fn manifest(&self) -> InferenceEngineManifest;
    /// Canonical protocol-capability identity for this adapter variant, when the adapter exposes
    /// a fixed qualification contract. Returning `None` keeps custom fixtures and future
    /// variants from being promoted until they provide their own typed contract.
    fn protocol_capability_hash(&self) -> Option<String> {
        None
    }
    fn descriptor(&self) -> InferenceEngineDescriptor {
        self.manifest().descriptor
    }
    fn default_target(&self) -> Option<VerifiedEngineTarget> {
        None
    }
    fn inspect<'a>(
        &'a self,
        target: &'a VerifiedEngineTarget,
    ) -> ExternalEngineInspectionFuture<'a>;
    fn qualify<'a>(
        &'a self,
        _target: &'a VerifiedEngineTarget,
        _model_id: &'a str,
    ) -> ExternalEngineQualificationFuture<'a> {
        Box::pin(async { Err(ExternalEngineAdapterError::QualificationUnavailable) })
    }
}

/// Marker boundary for inspectors of user-owned services. It grants no lifecycle authority.
pub trait ExternalInferenceEngineAdapter: EngineInspector {}

#[derive(Debug, Error)]
pub enum ExternalEngineAdapterError {
    #[error("无法创建外部推理引擎探测客户端")]
    Client,
    #[error("外部推理引擎探测地址无效")]
    InvalidEndpoint,
    #[error("外部推理引擎当前不可达")]
    Unreachable,
    #[error("外部推理引擎返回了无效或过大的响应")]
    InvalidResponse,
    #[error("外部推理引擎适配器身份重复或越权")]
    InvalidAdapterRegistry,
    #[error("运行方案引用的外部推理引擎适配器不可用")]
    AdapterUnavailable,
    #[error("外部推理引擎没有实现受控协议资格验证")]
    QualificationUnavailable,
    #[error("外部推理引擎未通过受控协议资格验证")]
    QualificationFailed,
    #[error("外部推理引擎缺少与支持单元匹配的验收证据")]
    AcceptanceEvidenceUnavailable,
}

#[derive(Clone)]
pub struct ExternalInferenceEngineRegistry {
    adapters: Arc<HashMap<EngineAdapterId, Arc<dyn ExternalInferenceEngineAdapter>>>,
    adapter_ids_by_kind: Arc<HashMap<InferenceEngineKind, Vec<EngineAdapterId>>>,
    manifests: InferenceEngineManifestRegistry,
    reviewed_acceptance: Arc<crate::InferenceEngineAcceptanceLedger>,
}

impl ExternalInferenceEngineRegistry {
    pub fn standard() -> Result<Self, ExternalEngineAdapterError> {
        Self::new(Self::standard_adapters()?)
    }

    fn standard_adapters()
    -> Result<Vec<Arc<dyn ExternalInferenceEngineAdapter>>, ExternalEngineAdapterError> {
        Ok(vec![
            Arc::new(OllamaExternalEngineAdapter::new()?),
            Arc::new(crate::VllmExternalEngineAdapter::new()?),
            Arc::new(crate::MlxLmExternalEngineAdapter::new()?),
            Arc::new(crate::MlcLlmExternalEngineAdapter::for_accelerator(
                InferenceAccelerator::Metal,
            )?),
            Arc::new(crate::MlcLlmExternalEngineAdapter::for_accelerator(
                InferenceAccelerator::Vulkan,
            )?),
            Arc::new(crate::MlcLlmExternalEngineAdapter::for_accelerator(
                InferenceAccelerator::Cuda,
            )?),
            Arc::new(crate::MlcLlmExternalEngineAdapter::for_accelerator(
                InferenceAccelerator::Rocm,
            )?),
            Arc::new(crate::OpenVinoExternalEngineAdapter::for_accelerator(
                InferenceAccelerator::Cpu,
            )?),
            Arc::new(crate::OpenVinoExternalEngineAdapter::for_accelerator(
                InferenceAccelerator::IntelGpu,
            )?),
            Arc::new(crate::OpenVinoExternalEngineAdapter::for_accelerator(
                InferenceAccelerator::IntelNpu,
            )?),
            Arc::new(crate::SglangExternalEngineAdapter::new()?),
            Arc::new(crate::LmDeployExternalEngineAdapter::new()?),
            Arc::new(crate::TensorRtLlmExternalEngineAdapter::new()?),
        ])
    }

    pub fn new(
        adapters: Vec<Arc<dyn ExternalInferenceEngineAdapter>>,
    ) -> Result<Self, ExternalEngineAdapterError> {
        let manifests = InferenceEngineManifestRegistry::new(
            adapters.iter().map(|adapter| adapter.manifest()).collect(),
        )
        .map_err(|_| ExternalEngineAdapterError::InvalidAdapterRegistry)?;
        let mut by_id = HashMap::with_capacity(adapters.len());
        let mut ids_by_kind = HashMap::<InferenceEngineKind, Vec<EngineAdapterId>>::new();
        for adapter in adapters {
            let manifest = adapter.manifest();
            let descriptor = &manifest.descriptor;
            if descriptor.ownership != InferenceEngineOwnership::External
                || descriptor.managed_lifecycle
                || by_id.insert(manifest.adapter_id.clone(), adapter).is_some()
            {
                return Err(ExternalEngineAdapterError::InvalidAdapterRegistry);
            }
            ids_by_kind
                .entry(descriptor.kind)
                .or_default()
                .push(manifest.adapter_id);
        }
        for ids in ids_by_kind.values_mut() {
            ids.sort_by(|left, right| left.variant.cmp(&right.variant));
        }
        Ok(Self {
            adapters: Arc::new(by_id),
            adapter_ids_by_kind: Arc::new(ids_by_kind),
            manifests,
            reviewed_acceptance: Arc::new(crate::InferenceEngineAcceptanceLedger {
                schema_version: crate::INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION,
                records: Vec::new(),
            }),
        })
    }

    /// Build a registry only after an explicit, reviewed acceptance ledger has qualified every
    /// formal support cell. The ordinary constructor remains useful while an adapter is still
    /// `connected` or `reserved`; this gate is the promotion boundary and fails closed when the
    /// checked-in ledger has not yet received real platform evidence.
    pub fn new_with_acceptance_evidence(
        adapters: Vec<Arc<dyn ExternalInferenceEngineAdapter>>,
        ledger: &crate::InferenceEngineAcceptanceLedger,
    ) -> Result<Self, ExternalEngineAdapterError> {
        let mut registry = Self::new(adapters)?;
        ledger
            .validate()
            .map_err(|_| ExternalEngineAdapterError::AcceptanceEvidenceUnavailable)?;
        validate_ledger_protocol_capability_hashes(&registry, ledger)?;
        for manifest in registry.manifest_registry().manifests() {
            ledger
                .validate_manifest(&manifest)
                .map_err(|_| ExternalEngineAdapterError::AcceptanceEvidenceUnavailable)?;
        }
        registry.reviewed_acceptance = Arc::new(ledger.clone());
        Ok(registry)
    }

    /// Build a registry whose support-cell status is derived from reviewed acceptance records.
    ///
    /// A checked-in adapter manifest intentionally starts with `connected` cells. Once a human
    /// has reviewed a complete ledger record, this constructor promotes only the exact matching
    /// platform/architecture/accelerator/deployment cell in an in-memory manifest wrapper. The
    /// upstream adapter implementation, endpoint policy and checked-in manifest remain unchanged;
    /// missing records therefore leave cells connected and fail closed in normal profile flows.
    pub fn new_with_reviewed_acceptance_evidence(
        adapters: Vec<Arc<dyn ExternalInferenceEngineAdapter>>,
        ledger: &crate::InferenceEngineAcceptanceLedger,
    ) -> Result<Self, ExternalEngineAdapterError> {
        let mut promoted = Vec::with_capacity(adapters.len());
        for adapter in adapters {
            let base_manifest = adapter.manifest();
            let expected_protocol_capability_hash = adapter.protocol_capability_hash();
            let manifest = promote_manifest_from_ledger(
                &base_manifest,
                expected_protocol_capability_hash.as_deref(),
                ledger,
                true,
            )?;
            promoted.push(Arc::new(ReviewedSupportCellAdapter {
                delegate: adapter,
                manifest,
            }) as Arc<dyn ExternalInferenceEngineAdapter>);
        }
        let mut registry = Self::new(promoted)?;
        validate_ledger_protocol_capability_hashes(&registry, ledger)?;
        registry.reviewed_acceptance = Arc::new(ledger.clone());
        Ok(registry)
    }

    /// Build the production registry with any reviewed ledger promotions that are available.
    ///
    /// This variant is intentionally compatible with an empty ledger and with existing formal
    /// cells whose historical evidence has not yet been imported. It promotes only connected
    /// cells that have a matching reviewed record; the strict constructor above remains available
    /// for a release/promotion gate that requires every formal cell to have a ledger record.
    pub fn new_with_reviewed_acceptance_promotions(
        adapters: Vec<Arc<dyn ExternalInferenceEngineAdapter>>,
        ledger: &crate::InferenceEngineAcceptanceLedger,
    ) -> Result<Self, ExternalEngineAdapterError> {
        let mut promoted = Vec::with_capacity(adapters.len());
        for adapter in adapters {
            let base_manifest = adapter.manifest();
            let expected_protocol_capability_hash = adapter.protocol_capability_hash();
            let manifest = promote_manifest_from_ledger(
                &base_manifest,
                expected_protocol_capability_hash.as_deref(),
                ledger,
                false,
            )?;
            promoted.push(Arc::new(ReviewedSupportCellAdapter {
                delegate: adapter,
                manifest,
            }) as Arc<dyn ExternalInferenceEngineAdapter>);
        }
        let mut registry = Self::new(promoted)?;
        validate_ledger_protocol_capability_hashes(&registry, ledger)?;
        registry.reviewed_acceptance = Arc::new(ledger.clone());
        Ok(registry)
    }

    /// Compose the standard adapters with checked-in reviewed promotions. Every reviewed record
    /// affects only its exact support cell; support levels without a matching record are preserved.
    pub fn standard_with_reviewed_acceptance_promotions() -> Result<Self, ExternalEngineAdapterError>
    {
        let ledger = crate::InferenceEngineAcceptanceLedger::standard()
            .map_err(|_| ExternalEngineAdapterError::AcceptanceEvidenceUnavailable)?;
        Self::standard_with_reviewed_acceptance_ledger(&ledger)
    }

    /// Compose the standard adapters with an explicitly supplied candidate ledger.
    ///
    /// This is used by the review/import tooling before a candidate file replaces the checked-in
    /// ledger. The candidate is parsed and validated by the caller; this method additionally
    /// proves every record maps to a declared support cell and applies the same in-memory
    /// promotion projection used by the desktop runtime.
    pub fn standard_with_reviewed_acceptance_ledger(
        ledger: &crate::InferenceEngineAcceptanceLedger,
    ) -> Result<Self, ExternalEngineAdapterError> {
        Self::new_with_reviewed_acceptance_promotions(Self::standard_adapters()?, ledger)
    }

    pub fn adapters(&self) -> Vec<Arc<dyn ExternalInferenceEngineAdapter>> {
        let mut entries = self.adapters.iter().collect::<Vec<_>>();
        entries.sort_by(|(left, _), (right, _)| {
            left.engine
                .storage_key()
                .cmp(right.engine.storage_key())
                .then_with(|| left.variant.cmp(&right.variant))
        });
        entries
            .into_iter()
            .map(|(_, adapter)| adapter.clone())
            .collect()
    }

    pub fn adapter(
        &self,
        kind: InferenceEngineKind,
    ) -> Option<Arc<dyn ExternalInferenceEngineAdapter>> {
        let ids = self.adapter_ids_by_kind.get(&kind)?;
        if ids.len() != 1 {
            return None;
        }
        self.adapters.get(&ids[0]).cloned()
    }

    pub fn adapter_by_id(
        &self,
        adapter_id: &EngineAdapterId,
    ) -> Option<Arc<dyn ExternalInferenceEngineAdapter>> {
        self.adapters.get(adapter_id).cloned()
    }

    pub fn manifest_registry(&self) -> InferenceEngineManifestRegistry {
        self.manifests.clone()
    }

    /// Return reviewed performance only when it applies to this exact saved runtime identity.
    ///
    /// Matching is intentionally stricter than support-cell promotion: adapter/cell, origin,
    /// configuration revision, engine identity, typed model evidence and native device class must
    /// all agree. A miss is ordinary `None`; it never falls back to another model, device or
    /// instance and never affects activation authority.
    #[allow(clippy::too_many_arguments)]
    pub fn reviewed_performance_for_runtime_profile(
        &self,
        adapter_id: &EngineAdapterId,
        support_cell: RuntimeProfileSupportCell,
        host: &HostCapabilitySnapshot,
        origin_fingerprint: &str,
        config_revision: u64,
        engine_version: &str,
        model_evidence: &RuntimeProfileEvidence,
    ) -> Option<RuntimeProfileReviewedPerformance> {
        let record = self.reviewed_acceptance.record_for(
            adapter_id,
            support_cell.platform,
            support_cell.architecture,
            support_cell.accelerator,
            support_cell.deployment,
        )?;
        if !matches!(
            record.status,
            InferenceEngineSupportStatus::Managed | InferenceEngineSupportStatus::VerifiedExternal
        ) || record.origin_fingerprint != origin_fingerprint
            || record.config_revision != config_revision
            || !record
                .model_evidence
                .as_ref()
                .is_some_and(|expected| expected.matches_runtime_evidence(model_evidence))
        {
            return None;
        }
        let engine_identity_matches = record.engine_version.as_deref().map_or_else(
            || {
                model_evidence.kind == RuntimeProfileEvidenceKind::DeploymentFingerprint
                    && record.deployment_fingerprint.as_deref()
                        == Some(model_evidence.value.as_str())
            },
            |expected| expected == engine_version,
        );
        if !engine_identity_matches {
            return None;
        }
        let current_attestation =
            crate::InferenceEngineAcceptanceHostAttestation::from_host_snapshot(
                host,
                support_cell.accelerator,
            )
            .ok()?;
        if record.host_attestation.as_ref() != Some(&current_attestation) {
            return None;
        }
        let stability = record.stability.as_ref()?;
        let sample_completion_tokens_per_second_milli = stability
            .total_completion_tokens
            .checked_mul(1_000_000)?
            .checked_div(stability.wall_time_ms)?;
        Some(RuntimeProfileReviewedPerformance {
            workload_revision: stability.workload_revision.clone(),
            attempts: stability.attempts,
            concurrency: stability.concurrency,
            p95_latency_ms: stability.p95_latency_ms,
            max_latency_ms: stability.max_latency_ms,
            total_prompt_tokens: stability.total_prompt_tokens,
            total_completion_tokens: stability.total_completion_tokens,
            wall_time_ms: stability.wall_time_ms,
            sample_completion_tokens_per_second_milli,
            reviewed_at_ms: record.verified_at_ms,
        })
    }

    pub fn verified_local_target(
        &self,
        kind: InferenceEngineKind,
        instance_id: &str,
        api_root: &str,
        config_revision: i64,
    ) -> Result<VerifiedEngineTarget, ExternalEngineAdapterError> {
        let adapter = self
            .adapter(kind)
            .ok_or(ExternalEngineAdapterError::AdapterUnavailable)?;
        let config_revision = u64::try_from(config_revision)
            .map_err(|_| ExternalEngineAdapterError::InvalidEndpoint)?;
        VerifiedEngineTarget::external_local(
            instance_id,
            &adapter.manifest(),
            api_root,
            config_revision,
        )
        .map_err(|_| ExternalEngineAdapterError::InvalidEndpoint)
    }

    pub fn verified_local_target_by_id(
        &self,
        adapter_id: &EngineAdapterId,
        instance_id: &str,
        api_root: &str,
        config_revision: u64,
    ) -> Result<VerifiedEngineTarget, ExternalEngineAdapterError> {
        self.verified_local_target_by_id_with_auth(
            adapter_id,
            instance_id,
            api_root,
            config_revision,
            EngineRequestAuth::None,
        )
    }

    pub fn verified_local_target_by_id_with_auth(
        &self,
        adapter_id: &EngineAdapterId,
        instance_id: &str,
        api_root: &str,
        config_revision: u64,
        request_auth: EngineRequestAuth,
    ) -> Result<VerifiedEngineTarget, ExternalEngineAdapterError> {
        let adapter = self
            .adapter_by_id(adapter_id)
            .ok_or(ExternalEngineAdapterError::AdapterUnavailable)?;
        VerifiedEngineTarget::external_local_with_auth(
            instance_id,
            &adapter.manifest(),
            api_root,
            config_revision,
            request_auth,
        )
        .map_err(|_| ExternalEngineAdapterError::InvalidEndpoint)
    }

    pub async fn inspect(
        &self,
        kind: InferenceEngineKind,
    ) -> Result<ExternalEngineSnapshot, ExternalEngineAdapterError> {
        let adapter = self
            .adapter(kind)
            .ok_or(ExternalEngineAdapterError::AdapterUnavailable)?;
        let target = adapter
            .default_target()
            .ok_or(ExternalEngineAdapterError::AdapterUnavailable)?;
        adapter.inspect(&target).await
    }

    pub async fn inspect_target(
        &self,
        target: &VerifiedEngineTarget,
    ) -> Result<ExternalEngineSnapshot, ExternalEngineAdapterError> {
        self.adapter_by_id(target.adapter_id())
            .ok_or(ExternalEngineAdapterError::AdapterUnavailable)?
            .inspect(target)
            .await
    }

    pub async fn qualify_target(
        &self,
        target: &VerifiedEngineTarget,
        model_id: &str,
    ) -> Result<EngineQualificationReport, ExternalEngineAdapterError> {
        self.adapter_by_id(target.adapter_id())
            .ok_or(ExternalEngineAdapterError::AdapterUnavailable)?
            .qualify(target, model_id)
            .await
    }
}

/// Delegates all network behavior to an adapter while carrying a ledger-derived, in-memory
/// support projection. Keeping this wrapper private prevents callers from manufacturing a
/// promoted adapter without going through the reviewed-ledger constructor.
#[derive(Clone)]
struct ReviewedSupportCellAdapter {
    delegate: Arc<dyn ExternalInferenceEngineAdapter>,
    manifest: InferenceEngineManifest,
}

impl EngineInspector for ReviewedSupportCellAdapter {
    fn manifest(&self) -> InferenceEngineManifest {
        self.manifest.clone()
    }

    fn protocol_capability_hash(&self) -> Option<String> {
        self.delegate.protocol_capability_hash()
    }

    fn default_target(&self) -> Option<VerifiedEngineTarget> {
        self.delegate.default_target()
    }

    fn inspect<'a>(
        &'a self,
        target: &'a VerifiedEngineTarget,
    ) -> ExternalEngineInspectionFuture<'a> {
        self.delegate.inspect(target)
    }

    fn qualify<'a>(
        &'a self,
        target: &'a VerifiedEngineTarget,
        model_id: &'a str,
    ) -> ExternalEngineQualificationFuture<'a> {
        self.delegate.qualify(target, model_id)
    }
}

impl ExternalInferenceEngineAdapter for ReviewedSupportCellAdapter {}

fn promote_manifest_from_ledger(
    base: &InferenceEngineManifest,
    expected_protocol_capability_hash: Option<&str>,
    ledger: &crate::InferenceEngineAcceptanceLedger,
    require_formal_records: bool,
) -> Result<InferenceEngineManifest, ExternalEngineAdapterError> {
    ledger
        .validate()
        .map_err(|_| ExternalEngineAdapterError::AcceptanceEvidenceUnavailable)?;
    let mut manifest = base.clone();
    for unit in &mut manifest.support_units {
        let Some(record) = ledger.record_for(
            &manifest.adapter_id,
            unit.platform,
            unit.architecture,
            unit.accelerator,
            unit.deployment,
        ) else {
            continue;
        };
        if base.descriptor.ownership == InferenceEngineOwnership::External
            && record.status != InferenceEngineSupportStatus::VerifiedExternal
        {
            return Err(ExternalEngineAdapterError::AcceptanceEvidenceUnavailable);
        }
        match expected_protocol_capability_hash {
            Some(expected) if record.protocol_capability_hash == expected => {}
            _ => {
                return Err(ExternalEngineAdapterError::AcceptanceEvidenceUnavailable);
            }
        }
        if matches!(
            unit.status,
            InferenceEngineSupportStatus::Managed | InferenceEngineSupportStatus::VerifiedExternal
        ) && unit.status != record.status
        {
            return Err(ExternalEngineAdapterError::AcceptanceEvidenceUnavailable);
        }
        unit.status = record.status;
        unit.evidence = Some(InferenceEngineSupportEvidenceSummary::for_status(
            record.status,
        ));
    }
    if require_formal_records {
        ledger
            .validate_manifest(&manifest)
            .map_err(|_| ExternalEngineAdapterError::AcceptanceEvidenceUnavailable)?;
    } else if ledger.records.iter().any(|record| {
        record.adapter_id == base.adapter_id
            && !base.support_units.iter().any(|unit| {
                unit.platform == record.platform
                    && unit.architecture == record.architecture
                    && unit.accelerator == record.accelerator
                    && unit.deployment == record.deployment
            })
    }) {
        return Err(ExternalEngineAdapterError::AcceptanceEvidenceUnavailable);
    }
    Ok(manifest)
}

fn validate_ledger_protocol_capability_hashes(
    registry: &ExternalInferenceEngineRegistry,
    ledger: &crate::InferenceEngineAcceptanceLedger,
) -> Result<(), ExternalEngineAdapterError> {
    for record in &ledger.records {
        let Some(adapter) = registry.adapter_by_id(&record.adapter_id) else {
            return Err(ExternalEngineAdapterError::AcceptanceEvidenceUnavailable);
        };
        let Some(expected) = adapter.protocol_capability_hash() else {
            return Err(ExternalEngineAdapterError::AcceptanceEvidenceUnavailable);
        };
        if record.protocol_capability_hash != expected {
            return Err(ExternalEngineAdapterError::AcceptanceEvidenceUnavailable);
        }
    }
    Ok(())
}

pub fn protocol_capability_set(
    capabilities: impl IntoIterator<Item = EngineProtocolCapability>,
) -> EngineProtocolCapabilitySet {
    let mut capabilities = capabilities.into_iter().collect::<Vec<_>>();
    capabilities.sort_unstable();
    capabilities.dedup();
    EngineProtocolCapabilitySet {
        revision: ENGINE_PROTOCOL_CAPABILITY_REVISION.to_owned(),
        capabilities,
    }
}

pub fn protocol_capability_hash(capabilities: &EngineProtocolCapabilitySet) -> String {
    let mut canonical = String::from(capabilities.revision.as_str());
    let mut normalized = capabilities.capabilities.clone();
    normalized.sort_unstable();
    normalized.dedup();
    for capability in normalized {
        canonical.push('\0');
        canonical.push_str(capability.storage_key());
    }
    Sha256::digest(canonical.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Clone)]
pub struct OllamaExternalEngineAdapter {
    http: BoundedEngineHttpClient,
    qualification_http: BoundedEngineHttpClient,
}

impl OllamaExternalEngineAdapter {
    pub fn new() -> Result<Self, ExternalEngineAdapterError> {
        Ok(Self {
            http: BoundedEngineHttpClient::new("ollama-read-only-adapter")
                .map_err(map_http_error)?,
            qualification_http: BoundedEngineHttpClient::with_timeouts(
                "ollama-qualification-adapter",
                Duration::from_secs(2),
                Duration::from_secs(120),
            )
            .map_err(map_http_error)?,
        })
    }

    /// Request controls shared by Ollama qualification and its real-service stability probe.
    ///
    /// Ollama thinking-capable models enable reasoning by default. Its OpenAI-compatible API
    /// documents `reasoning_effort: "none"` as the bounded way to disable that reasoning so the
    /// fixed qualification token budget measures tool/stream support rather than hidden thought.
    pub fn openai_qualification_options() -> OpenAiQualificationOptions {
        OpenAiQualificationOptions {
            reasoning_effort: Some(OpenAiQualificationReasoningEffort::Disabled),
            ..OpenAiQualificationOptions::default()
        }
    }

    async fn inspect_inner(
        &self,
        target: &VerifiedEngineTarget,
    ) -> Result<ExternalEngineSnapshot, ExternalEngineAdapterError> {
        if target.adapter_id() != &self.manifest().adapter_id {
            return Err(ExternalEngineAdapterError::AdapterUnavailable);
        }
        let version_body = self
            .http
            .get_bounded(target, "/api/version", MAX_VERSION_BODY_BYTES)
            .await
            .map_err(map_http_error)?;
        let version = parse_version(&version_body)?;
        let (models, model_catalog_complete) = match self
            .http
            .get_bounded(target, "/api/tags", MAX_TAGS_BODY_BYTES)
            .await
            .map_err(map_http_error)
            .and_then(|body| parse_models(&body))
        {
            Ok(models) => (models, true),
            Err(_) => (Vec::new(), false),
        };
        Ok(ExternalEngineSnapshot {
            engine: InferenceEngineKind::Ollama,
            display_name: "本机 Ollama".to_owned(),
            api_root: target.origin().api_root().as_str().to_owned(),
            version,
            engine_version_exact: true,
            models,
            model_catalog_complete,
        })
    }

    async fn qualify_inner(
        &self,
        target: &VerifiedEngineTarget,
        model_id: &str,
    ) -> Result<EngineQualificationReport, ExternalEngineAdapterError> {
        if target.adapter_id() != &self.manifest().adapter_id {
            return Err(ExternalEngineAdapterError::AdapterUnavailable);
        }
        if model_id.is_empty() || model_id.len() > MAX_OLLAMA_MODEL_ID_BYTES {
            return Err(ExternalEngineAdapterError::InvalidResponse);
        }
        let version = parse_version(
            &self
                .qualification_http
                .get_bounded(target, "/api/version", MAX_VERSION_BODY_BYTES)
                .await
                .map_err(map_http_error)?,
        )?;
        let observation = qualify_openai_agent_protocol(
            &self.qualification_http,
            target,
            model_id,
            &Self::openai_qualification_options(),
        )
        .await?;
        // The protocol probe leaves the qualified model resident. Ollama's official `/api/ps`
        // response therefore provides a model-bound runtime-placement observation rather than
        // a host-capability guess. CPU residency is portable; on a loopback macOS target any
        // non-zero device allocation is Metal. Other operating systems need a future
        // backend-specific identity before HAL100 may distinguish CUDA/ROCm/Vulkan.
        let observed_accelerator = parse_running_accelerator(
            &self
                .qualification_http
                .get_bounded(target, "/api/ps", MAX_PS_BODY_BYTES)
                .await
                .map_err(map_http_error)?,
            model_id,
        )?;
        let deployment_fingerprint = observation.system_fingerprint.map(|fingerprint| {
            let accelerator = observed_accelerator
                .map(InferenceAccelerator::storage_key)
                .unwrap_or("unresolved");
            Sha256::digest(
                format!("ollama-deployment-v2\0{fingerprint}\0{model_id}\0{accelerator}")
                    .as_bytes(),
            )
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
        });
        let protocol_capabilities = ollama_agent_protocol_capabilities();
        Ok(EngineQualificationReport {
            adapter_id: self.manifest().adapter_id,
            model_id: model_id.to_owned(),
            protocol_capabilities,
            protocol_capability_hash: ollama_agent_protocol_capability_hash(),
            observed_engine_version: Some(version),
            runtime_device_evidence: observed_accelerator.map_or(
                EngineRuntimeDeviceEvidence::Unresolved,
                |accelerator| EngineRuntimeDeviceEvidence::ModelResidencyObservation {
                    accelerator,
                },
            ),
            deployment_fingerprint,
        })
    }
}

impl EngineInspector for OllamaExternalEngineAdapter {
    fn manifest(&self) -> InferenceEngineManifest {
        InferenceEngineManifest {
            adapter_id: EngineAdapterId {
                engine: InferenceEngineKind::Ollama,
                variant: "official-loopback-api".to_owned(),
                contract_revision: ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
            },
            descriptor: InferenceEngineDescriptor {
                kind: InferenceEngineKind::Ollama,
                display_name: "用户所有的本机 Ollama".to_owned(),
                ownership: InferenceEngineOwnership::External,
                deployment: InferenceDeployment::Local,
                protocols: vec![InferenceProtocol::OpenAi, InferenceProtocol::Ollama],
                platforms: vec![
                    InferencePlatform::MacOs,
                    InferencePlatform::Windows,
                    InferencePlatform::Linux,
                ],
                architectures: vec![
                    InferenceArchitecture::Aarch64,
                    InferenceArchitecture::X86_64,
                ],
                accelerators: vec![
                    InferenceAccelerator::Cpu,
                    InferenceAccelerator::Metal,
                    InferenceAccelerator::Cuda,
                    InferenceAccelerator::Rocm,
                    InferenceAccelerator::Vulkan,
                ],
                model_formats: vec![InferenceModelFormat::Gguf],
                managed_lifecycle: false,
            },
            support_units: vec![
                InferenceEngineSupportUnit {
                    platform: InferencePlatform::MacOs,
                    architecture: InferenceArchitecture::Aarch64,
                    accelerator: InferenceAccelerator::Cpu,
                    deployment: InferenceDeployment::Local,
                    status: InferenceEngineSupportStatus::VerifiedExternal,
                    evidence: Some(crate::support_evidence_for(
                        InferenceEngineKind::Ollama,
                        Some(InferenceEngineSupportStatus::VerifiedExternal),
                    )),
                },
                InferenceEngineSupportUnit {
                    platform: InferencePlatform::MacOs,
                    architecture: InferenceArchitecture::Aarch64,
                    accelerator: InferenceAccelerator::Metal,
                    deployment: InferenceDeployment::Local,
                    status: InferenceEngineSupportStatus::VerifiedExternal,
                    evidence: Some(crate::support_evidence_for(
                        InferenceEngineKind::Ollama,
                        Some(InferenceEngineSupportStatus::VerifiedExternal),
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
                InferenceEngineSupportUnit {
                    platform: InferencePlatform::Linux,
                    architecture: InferenceArchitecture::X86_64,
                    accelerator: InferenceAccelerator::Cpu,
                    deployment: InferenceDeployment::Local,
                    status: InferenceEngineSupportStatus::Reserved,
                    evidence: None,
                },
            ],
        }
    }

    fn protocol_capability_hash(&self) -> Option<String> {
        Some(ollama_agent_protocol_capability_hash())
    }

    fn default_target(&self) -> Option<VerifiedEngineTarget> {
        VerifiedEngineTarget::external_local(
            "discovery:ollama",
            &self.manifest(),
            OLLAMA_API_ROOT,
            0,
        )
        .ok()
    }

    fn inspect<'a>(
        &'a self,
        target: &'a VerifiedEngineTarget,
    ) -> ExternalEngineInspectionFuture<'a> {
        Box::pin(self.inspect_inner(target))
    }

    fn qualify<'a>(
        &'a self,
        target: &'a VerifiedEngineTarget,
        model_id: &'a str,
    ) -> ExternalEngineQualificationFuture<'a> {
        Box::pin(self.qualify_inner(target, model_id))
    }
}

impl ExternalInferenceEngineAdapter for OllamaExternalEngineAdapter {}

fn map_http_error(error: EngineHttpError) -> ExternalEngineAdapterError {
    match error {
        EngineHttpError::Client => ExternalEngineAdapterError::Client,
        EngineHttpError::Target => ExternalEngineAdapterError::InvalidEndpoint,
        EngineHttpError::Unreachable => ExternalEngineAdapterError::Unreachable,
        EngineHttpError::InvalidResponse => ExternalEngineAdapterError::InvalidResponse,
    }
}

#[derive(Deserialize)]
struct OllamaVersionResponse {
    version: String,
}

#[derive(Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModel>,
}

#[derive(Deserialize)]
struct OllamaModel {
    name: String,
    model: String,
    size: u64,
    digest: String,
    details: OllamaModelDetails,
}

#[derive(Deserialize)]
struct OllamaModelDetails {
    format: String,
    family: Option<String>,
    parameter_size: Option<String>,
    quantization_level: Option<String>,
}

#[derive(Deserialize)]
struct OllamaPsResponse {
    models: Vec<OllamaRunningModel>,
}

#[derive(Deserialize)]
struct OllamaRunningModel {
    name: String,
    model: String,
    size_vram: u64,
}

fn parse_running_accelerator(
    body: &[u8],
    model_id: &str,
) -> Result<Option<InferenceAccelerator>, ExternalEngineAdapterError> {
    let response = serde_json::from_slice::<OllamaPsResponse>(body)
        .map_err(|_| ExternalEngineAdapterError::InvalidResponse)?;
    if response.models.len() > MAX_EXTERNAL_MODELS {
        return Err(ExternalEngineAdapterError::InvalidResponse);
    }
    let mut matched_size_vram = None;
    for model in response.models {
        validate_text(&model.name, MAX_MODEL_NAME_BYTES)?;
        validate_text(&model.model, MAX_MODEL_NAME_BYTES)?;
        if model.name != model.model {
            return Err(ExternalEngineAdapterError::InvalidResponse);
        }
        if model.model == model_id && matched_size_vram.replace(model.size_vram).is_some() {
            return Err(ExternalEngineAdapterError::InvalidResponse);
        }
    }
    match matched_size_vram.ok_or(ExternalEngineAdapterError::InvalidResponse)? {
        0 => Ok(Some(InferenceAccelerator::Cpu)),
        _ if cfg!(target_os = "macos") => Ok(Some(InferenceAccelerator::Metal)),
        _ => Ok(None),
    }
}

fn parse_version(body: &[u8]) -> Result<String, ExternalEngineAdapterError> {
    let response = serde_json::from_slice::<OllamaVersionResponse>(body)
        .map_err(|_| ExternalEngineAdapterError::InvalidResponse)?;
    validate_text(&response.version, 128)?;
    Ok(response.version)
}

fn parse_models(
    body: &[u8],
) -> Result<Vec<ExternalEngineModelSummary>, ExternalEngineAdapterError> {
    let response = serde_json::from_slice::<OllamaTagsResponse>(body)
        .map_err(|_| ExternalEngineAdapterError::InvalidResponse)?;
    if response.models.len() > MAX_EXTERNAL_MODELS {
        return Err(ExternalEngineAdapterError::InvalidResponse);
    }
    let mut names = HashSet::with_capacity(response.models.len());
    let mut models = Vec::with_capacity(response.models.len());
    for model in response.models {
        validate_text(&model.name, MAX_MODEL_NAME_BYTES)?;
        validate_text(&model.model, MAX_MODEL_NAME_BYTES)?;
        if model.name != model.model
            || model.digest.len() != 64
            || !model.digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !names.insert(model.model.clone())
        {
            return Err(ExternalEngineAdapterError::InvalidResponse);
        }
        validate_text(&model.details.format, MAX_MODEL_DETAIL_BYTES)?;
        validate_optional_text(model.details.family.as_deref(), MAX_MODEL_DETAIL_BYTES)?;
        validate_optional_text(
            model.details.parameter_size.as_deref(),
            MAX_MODEL_DETAIL_BYTES,
        )?;
        validate_optional_text(
            model.details.quantization_level.as_deref(),
            MAX_MODEL_DETAIL_BYTES,
        )?;
        let digest = model.digest.to_ascii_lowercase();
        models.push(ExternalEngineModelSummary {
            name: model.model,
            digest: digest.clone(),
            size_bytes: model.size,
            format: model.details.format,
            family: model.details.family,
            parameter_size: model.details.parameter_size,
            quantization: model.details.quantization_level,
            evidence: RuntimeProfileEvidence {
                kind: RuntimeProfileEvidenceKind::ContentDigest,
                algorithm: "ollama-digest".to_owned(),
                value: digest,
            },
        });
    }
    models.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(models)
}

fn validate_optional_text(
    value: Option<&str>,
    max_bytes: usize,
) -> Result<(), ExternalEngineAdapterError> {
    match value {
        Some(value) => validate_text(value, max_bytes),
        None => Ok(()),
    }
}

fn validate_text(value: &str, max_bytes: usize) -> Result<(), ExternalEngineAdapterError> {
    if value.is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        return Err(ExternalEngineAdapterError::InvalidResponse);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use axum::{
        Json, Router,
        response::IntoResponse,
        routing::{get, post},
    };
    use serde_json::{Value, json};

    use super::*;

    struct VariantInspector {
        variant: &'static str,
        status: InferenceEngineSupportStatus,
        protocol_hash: Option<&'static str>,
    }

    impl EngineInspector for VariantInspector {
        fn manifest(&self) -> InferenceEngineManifest {
            let adapter = OllamaExternalEngineAdapter::new().expect("Ollama adapter");
            let mut manifest = adapter.manifest();
            manifest.adapter_id.variant = self.variant.to_owned();
            for unit in &mut manifest.support_units {
                unit.status = self.status;
                unit.evidence = Some(InferenceEngineSupportEvidenceSummary::for_status(
                    self.status,
                ));
            }
            manifest
        }

        fn protocol_capability_hash(&self) -> Option<String> {
            self.protocol_hash.map(ToOwned::to_owned)
        }

        fn inspect<'a>(
            &'a self,
            _target: &'a VerifiedEngineTarget,
        ) -> ExternalEngineInspectionFuture<'a> {
            Box::pin(async { Err(ExternalEngineAdapterError::Unreachable) })
        }
    }

    impl ExternalInferenceEngineAdapter for VariantInspector {}

    async fn adapter_for(
        app: Router,
    ) -> (
        OllamaExternalEngineAdapter,
        VerifiedEngineTarget,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let address = listener.local_addr().expect("address");
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("test server");
        });
        let adapter = OllamaExternalEngineAdapter::new().expect("adapter");
        let target = VerifiedEngineTarget::external_local(
            "test-ollama",
            &adapter.manifest(),
            &format!("http://127.0.0.1:{}/v1/", address.port()),
            1,
        )
        .expect("target");
        (adapter, target, task)
    }

    #[tokio::test]
    async fn inspects_version_and_bounded_model_identity_without_mutation_endpoints() {
        let digest = "a".repeat(64);
        let app = Router::new()
            .route(
                "/api/version",
                get(|| async { Json(json!({"version":"0.12.6-test"})) }),
            )
            .route(
                "/api/tags",
                get(move || {
                    let digest = digest.clone();
                    async move {
                        Json(json!({"models":[{
                            "name":"qwen3:8b",
                            "model":"qwen3:8b",
                            "size":4_000_000_000_u64,
                            "digest":digest,
                            "details":{
                                "format":"gguf",
                                "family":"qwen3",
                                "parameter_size":"8.2B",
                                "quantization_level":"Q4_K_M"
                            }
                        }]}))
                    }
                }),
            );
        let (adapter, target, task) = adapter_for(app).await;

        let snapshot = adapter.inspect(&target).await.expect("snapshot");
        assert_eq!(snapshot.version, "0.12.6-test");
        assert!(snapshot.model_catalog_complete);
        assert_eq!(snapshot.models.len(), 1);
        assert_eq!(snapshot.models[0].name, "qwen3:8b");
        assert_eq!(snapshot.models[0].quantization.as_deref(), Some("Q4_K_M"));
        assert_eq!(
            adapter.descriptor().ownership,
            InferenceEngineOwnership::External
        );
        assert!(!adapter.descriptor().managed_lifecycle);
        task.abort();
    }

    #[tokio::test]
    async fn keeps_identity_but_marks_model_catalog_incomplete_when_tags_are_invalid() {
        let app = Router::new()
            .route(
                "/api/version",
                get(|| async { Json(json!({"version":"0.12.6-test"})) }),
            )
            .route(
                "/api/tags",
                get(|| async { Json(json!({"models":[{"name":"bad"}]})) }),
            );
        let (adapter, target, task) = adapter_for(app).await;

        let snapshot = adapter.inspect(&target).await.expect("version identity");
        assert!(!snapshot.model_catalog_complete);
        assert!(snapshot.models.is_empty());
        task.abort();
    }

    #[tokio::test]
    async fn qualifies_ollama_openai_agent_protocol_and_binds_version() {
        let app = Router::new()
            .route(
                "/api/version",
                get(|| async { Json(json!({"version":"0.12.6-test"})) }),
            )
            .route(
                "/v1/chat/completions",
                post(|Json(body): Json<Value>| async move {
                    assert_eq!(
                        body.get("reasoning_effort").and_then(Value::as_str),
                        Some("none")
                    );
                    if body.get("stream").and_then(Value::as_bool) == Some(true) {
                        (
                            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                            concat!(
                                "data: {\"choices\":[{\"delta\":{\"content\":\"OK\"},\"finish_reason\":null}],\"system_fingerprint\":\"ollama-fingerprint-v1\"}\n\n",
                                "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"system_fingerprint\":\"ollama-fingerprint-v1\"}\n\n",
                                "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":1},\"system_fingerprint\":\"ollama-fingerprint-v1\"}\n\n",
                                "data: [DONE]\n\n"
                            ),
                        )
                            .into_response()
                    } else {
                        Json(json!({
                            "choices": [{"message": {"tool_calls": [{"function": {
                                "name": "hal100_protocol_probe",
                                "arguments": "{\"value\":\"ok\"}"
                            }}]}}],
                            "system_fingerprint":"ollama-fingerprint-v1",
                            "usage": {"prompt_tokens": 12, "completion_tokens": 4}
                        }))
                            .into_response()
                    }
                }),
            )
            .route(
                "/api/ps",
                get(|| async {
                    Json(json!({"models":[{
                        "name":"qwen3:8b",
                        "model":"qwen3:8b",
                        "size_vram":2_000_000_000_u64
                    }]}))
                }),
            );
        let (adapter, target, task) = adapter_for(app).await;

        let report = adapter
            .qualify(&target, "qwen3:8b")
            .await
            .expect("Ollama qualification");
        assert_eq!(
            report.observed_engine_version.as_deref(),
            Some("0.12.6-test")
        );
        assert_eq!(
            report.protocol_capability_hash,
            ollama_agent_protocol_capability_hash()
        );
        if cfg!(target_os = "macos") {
            assert_eq!(
                report.runtime_device_evidence,
                EngineRuntimeDeviceEvidence::ModelResidencyObservation {
                    accelerator: InferenceAccelerator::Metal,
                }
            );
        } else {
            assert_eq!(
                report.runtime_device_evidence,
                EngineRuntimeDeviceEvidence::Unresolved
            );
        }
        assert!(report.deployment_fingerprint.is_some());
        assert_eq!(
            report.protocol_capabilities,
            ollama_agent_protocol_capabilities()
        );
        task.abort();
    }

    #[test]
    fn running_model_observation_proves_cpu_and_rejects_ambiguous_identity() {
        assert_eq!(
            parse_running_accelerator(
                br#"{"models":[{"name":"qwen3:8b","model":"qwen3:8b","size_vram":0}]}"#,
                "qwen3:8b",
            )
            .expect("CPU observation"),
            Some(InferenceAccelerator::Cpu)
        );
        assert!(matches!(
            parse_running_accelerator(
                br#"{"models":[{"name":"other","model":"other","size_vram":0}]}"#,
                "qwen3:8b",
            ),
            Err(ExternalEngineAdapterError::InvalidResponse)
        ));
    }

    #[test]
    fn rejects_remote_or_credential_bearing_endpoints() {
        let adapter = OllamaExternalEngineAdapter::new().expect("adapter");
        assert!(
            VerifiedEngineTarget::external_local(
                "remote",
                &adapter.manifest(),
                "https://ollama.com/v1/",
                1,
            )
            .is_err()
        );
        assert!(
            VerifiedEngineTarget::external_local(
                "credential",
                &adapter.manifest(),
                "http://user:secret@127.0.0.1:11434/v1/",
                1,
            )
            .is_err()
        );
    }

    #[test]
    fn registry_keeps_variants_distinct_and_fails_closed_for_legacy_kind_lookup() {
        let first = Arc::new(VariantInspector {
            variant: "first",
            status: InferenceEngineSupportStatus::VerifiedExternal,
            protocol_hash: None,
        });
        let second = Arc::new(VariantInspector {
            variant: "second",
            status: InferenceEngineSupportStatus::VerifiedExternal,
            protocol_hash: None,
        });
        let first_id = first.manifest().adapter_id;
        let second_id = second.manifest().adapter_id;
        let registry =
            ExternalInferenceEngineRegistry::new(vec![first, second]).expect("variant registry");

        assert!(registry.adapter(InferenceEngineKind::Ollama).is_none());
        assert!(registry.adapter_by_id(&first_id).is_some());
        assert!(registry.adapter_by_id(&second_id).is_some());
        assert!(
            registry
                .verified_local_target(
                    InferenceEngineKind::Ollama,
                    "legacy-ambiguous",
                    OLLAMA_API_ROOT,
                    1,
                )
                .is_err()
        );
    }

    #[test]
    fn reviewed_acceptance_evidence_promotes_only_the_matching_support_cell() {
        let adapter = Arc::new(VariantInspector {
            variant: "reviewed-cell",
            status: InferenceEngineSupportStatus::Connected,
            protocol_hash: Some("abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"),
        });
        let base_manifest = adapter.manifest();
        assert_eq!(
            base_manifest.support_units[0].status,
            InferenceEngineSupportStatus::Connected
        );
        let status = InferenceEngineSupportStatus::VerifiedExternal;
        let profile_model_evidence = RuntimeProfileEvidence {
            kind: RuntimeProfileEvidenceKind::CatalogIdentity,
            algorithm: "acceptance-test-model-id".to_owned(),
            value: "acceptance-test-model".to_owned(),
        };
        let host = HostCapabilitySnapshot {
            platform: InferencePlatform::MacOs,
            architecture: InferenceArchitecture::Aarch64,
            cpu_brand: "Acceptance Fixture CPU".to_owned(),
            device_model: "AcceptanceFixtureModel".to_owned(),
            total_memory_bytes: 32 * 1024 * 1024 * 1024,
            physical_cpu_cores: 8,
            logical_cpu_cores: 16,
            accelerators: vec![InferenceAccelerator::Cpu],
            model_storage_path: "/redacted-model-storage".to_owned(),
            model_storage_available_bytes: 100,
            probe_revision: "host-capabilities-v3".to_owned(),
        };
        let host_attestation = crate::InferenceEngineAcceptanceHostAttestation::from_host_snapshot(
            &host,
            InferenceAccelerator::Cpu,
        )
        .expect("native host attestation");
        let ledger = crate::InferenceEngineAcceptanceLedger {
            schema_version: crate::INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION,
            records: vec![crate::InferenceEngineAcceptanceRecord {
                id: "reviewed-ollama-cell".to_owned(),
                adapter_id: base_manifest.adapter_id.clone(),
                instance_id: "reviewed:ollama".to_owned(),
                origin_fingerprint:
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned(),
                config_revision: 1,
                protocol_capability_hash:
                    "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789".to_owned(),
                platform: InferencePlatform::MacOs,
                architecture: InferenceArchitecture::Aarch64,
                accelerator: InferenceAccelerator::Cpu,
                deployment: InferenceDeployment::Local,
                status,
                verified_at_ms: 1,
                engine_version: Some("0.12.6".to_owned()),
                deployment_fingerprint: None,
                model_revision: Some("ollama/qwen3-8b".to_owned()),
                host_summary: Some("macos/aarch64/cpu".to_owned()),
                host_attestation: Some(host_attestation),
                model_evidence: Some(
                    crate::InferenceEngineAcceptanceModelEvidence::from_runtime_evidence(
                        &profile_model_evidence,
                    )
                    .expect("redacted model evidence"),
                ),
                stability: Some(crate::engine_acceptance_evidence::test_stability_profile()),
                resilience: Some(crate::InferenceEngineAcceptanceResilience::complete()),
                evidence: InferenceEngineSupportEvidenceSummary::for_status(status)
                    .verified
                    .into_iter()
                    .map(|kind| crate::InferenceEngineAcceptanceEvidence {
                        kind,
                        source: "docs/ITERATION_60_CHECKPOINT.md".to_owned(),
                        assertion: "reviewed support-cell evidence".to_owned(),
                    })
                    .collect(),
            }],
        };
        ledger.validate().expect("reviewed ledger");
        let registry = ExternalInferenceEngineRegistry::new_with_reviewed_acceptance_evidence(
            vec![adapter],
            &ledger,
        )
        .expect("promoted registry");
        let promoted = registry
            .manifest_registry()
            .manifest(&base_manifest.adapter_id)
            .expect("promoted manifest");
        assert_eq!(
            promoted.support_units[0].status,
            InferenceEngineSupportStatus::VerifiedExternal
        );
        assert_eq!(
            promoted.support_units[0].evidence,
            Some(InferenceEngineSupportEvidenceSummary::for_status(status))
        );

        let support_cell = RuntimeProfileSupportCell {
            platform: InferencePlatform::MacOs,
            architecture: InferenceArchitecture::Aarch64,
            accelerator: InferenceAccelerator::Cpu,
            deployment: InferenceDeployment::Local,
        };
        let reviewed = registry
            .reviewed_performance_for_runtime_profile(
                &base_manifest.adapter_id,
                support_cell,
                &host,
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                1,
                "0.12.6",
                &profile_model_evidence,
            )
            .expect("exact saved identity receives reviewed performance");
        assert_eq!(reviewed.p95_latency_ms, 90);
        assert_eq!(reviewed.sample_completion_tokens_per_second_milli, 40_000);

        let different_model = RuntimeProfileEvidence {
            value: "another-model".to_owned(),
            ..profile_model_evidence.clone()
        };
        let mut different_host = host.clone();
        different_host.device_model = "AnotherDeviceClass".to_owned();
        for candidate in [
            registry.reviewed_performance_for_runtime_profile(
                &base_manifest.adapter_id,
                support_cell,
                &host,
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                1,
                "0.12.6",
                &different_model,
            ),
            registry.reviewed_performance_for_runtime_profile(
                &base_manifest.adapter_id,
                support_cell,
                &different_host,
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                1,
                "0.12.6",
                &profile_model_evidence,
            ),
            registry.reviewed_performance_for_runtime_profile(
                &base_manifest.adapter_id,
                support_cell,
                &host,
                "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
                1,
                "0.12.6",
                &profile_model_evidence,
            ),
            registry.reviewed_performance_for_runtime_profile(
                &base_manifest.adapter_id,
                support_cell,
                &host,
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                2,
                "0.12.6",
                &profile_model_evidence,
            ),
            registry.reviewed_performance_for_runtime_profile(
                &base_manifest.adapter_id,
                support_cell,
                &host,
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                1,
                "0.12.7",
                &profile_model_evidence,
            ),
        ] {
            assert!(
                candidate.is_none(),
                "reviewed data must fail closed on scope drift"
            );
        }
    }

    #[test]
    fn acceptance_evidence_protocol_hash_must_match_the_adapter_variant() {
        let expected_hash = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
        let adapter = Arc::new(VariantInspector {
            variant: "strict-hash",
            status: InferenceEngineSupportStatus::Reserved,
            protocol_hash: Some(expected_hash),
        });
        let manifest = adapter.manifest();
        let unit = manifest.support_units[0].clone();
        let ledger = crate::InferenceEngineAcceptanceLedger {
            schema_version: crate::INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION,
            records: vec![crate::InferenceEngineAcceptanceRecord {
                id: "strict-hash-record".to_owned(),
                adapter_id: manifest.adapter_id.clone(),
                instance_id: "strict:hash".to_owned(),
                origin_fingerprint:
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned(),
                config_revision: 1,
                protocol_capability_hash:
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned(),
                platform: unit.platform,
                architecture: unit.architecture,
                accelerator: unit.accelerator,
                deployment: unit.deployment,
                status: unit.status,
                verified_at_ms: 1,
                engine_version: None,
                deployment_fingerprint: None,
                model_revision: None,
                host_summary: None,
                host_attestation: None,
                model_evidence: None,
                stability: None,
                resilience: None,
                evidence: Vec::new(),
            }],
        };
        ledger.validate().expect("hash mismatch fixture");

        assert_eq!(
            ExternalInferenceEngineRegistry::new_with_acceptance_evidence(
                vec![adapter.clone()],
                &ledger,
            )
            .err()
            .map(|error| error.to_string()),
            Some("外部推理引擎缺少与支持单元匹配的验收证据".to_owned())
        );
        assert_eq!(
            ExternalInferenceEngineRegistry::new_with_reviewed_acceptance_promotions(
                vec![adapter],
                &ledger,
            )
            .err()
            .map(|error| error.to_string()),
            Some("外部推理引擎缺少与支持单元匹配的验收证据".to_owned())
        );

        let mut unknown_ledger = ledger.clone();
        unknown_ledger.records[0].adapter_id.variant = "unknown-variant".to_owned();
        unknown_ledger.validate().expect("unknown adapter fixture");
        let unknown_adapter = Arc::new(VariantInspector {
            variant: "strict-hash",
            status: InferenceEngineSupportStatus::Reserved,
            protocol_hash: Some(expected_hash),
        });
        assert_eq!(
            ExternalInferenceEngineRegistry::new_with_reviewed_acceptance_evidence(
                vec![unknown_adapter],
                &unknown_ledger,
            )
            .err()
            .map(|error| error.to_string()),
            Some("外部推理引擎缺少与支持单元匹配的验收证据".to_owned())
        );
    }

    #[test]
    fn standard_registry_can_apply_reviewed_promotions_without_empty_ledger_startup_failure() {
        let registry =
            ExternalInferenceEngineRegistry::standard_with_reviewed_acceptance_promotions()
                .expect("empty checked-in ledger is a valid startup state");
        let mlx = registry
            .manifest_registry()
            .manifests_for_engine(InferenceEngineKind::MlxLm);
        assert_eq!(mlx.len(), 1);
        assert_eq!(
            mlx[0].support_units[0].status,
            InferenceEngineSupportStatus::VerifiedExternal
        );
        let vllm = registry
            .manifest_registry()
            .manifests_for_engine(InferenceEngineKind::Vllm);
        assert_eq!(vllm.len(), 1);
        assert!(
            vllm[0]
                .support_units
                .iter()
                .all(|unit| unit.status == InferenceEngineSupportStatus::Connected)
        );
    }

    #[test]
    fn standard_registry_exposes_connected_and_verified_standard_external_contracts() {
        let registry = ExternalInferenceEngineRegistry::standard().expect("standard registry");
        let vllm = registry
            .manifest_registry()
            .manifests_for_engine(InferenceEngineKind::Vllm);

        assert_eq!(vllm.len(), 1);
        assert!(
            vllm[0]
                .support_units
                .iter()
                .all(|unit| unit.status == InferenceEngineSupportStatus::Connected)
        );
        assert!(registry.adapter(InferenceEngineKind::Vllm).is_some());
        let mlx_lm = registry
            .manifest_registry()
            .manifests_for_engine(InferenceEngineKind::MlxLm);
        assert_eq!(mlx_lm.len(), 1);
        assert!(
            mlx_lm[0]
                .support_units
                .iter()
                .all(|unit| unit.status == InferenceEngineSupportStatus::VerifiedExternal)
        );
        assert!(registry.adapter(InferenceEngineKind::MlxLm).is_some());
        let mlc_llm = registry
            .manifest_registry()
            .manifests_for_engine(InferenceEngineKind::MlcLlm);
        assert_eq!(mlc_llm.len(), 4);
        assert!(
            mlc_llm
                .iter()
                .flat_map(|manifest| &manifest.support_units)
                .all(|unit| unit.status == InferenceEngineSupportStatus::Connected)
        );
        assert!(registry.adapter(InferenceEngineKind::MlcLlm).is_none());
        let openvino = registry
            .manifest_registry()
            .manifests_for_engine(InferenceEngineKind::OpenVino);
        assert_eq!(openvino.len(), 3);
        assert!(
            openvino
                .iter()
                .flat_map(|manifest| &manifest.support_units)
                .all(|unit| unit.status == InferenceEngineSupportStatus::Connected)
        );
        assert!(registry.adapter(InferenceEngineKind::OpenVino).is_none());
        for variant in [
            "ovms-openai-cpu",
            "ovms-openai-intel-gpu",
            "ovms-openai-intel-npu",
        ] {
            assert!(
                registry
                    .adapter_by_id(&EngineAdapterId {
                        engine: InferenceEngineKind::OpenVino,
                        variant: variant.to_owned(),
                        contract_revision: ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
                    })
                    .is_some()
            );
        }
        let sglang = registry
            .manifest_registry()
            .manifests_for_engine(InferenceEngineKind::Sglang);
        assert_eq!(sglang.len(), 1);
        assert!(
            sglang[0]
                .support_units
                .iter()
                .all(|unit| unit.status == InferenceEngineSupportStatus::Connected)
        );
        assert!(registry.adapter(InferenceEngineKind::Sglang).is_some());
        let lmdeploy = registry
            .manifest_registry()
            .manifests_for_engine(InferenceEngineKind::LmDeploy);
        assert_eq!(lmdeploy.len(), 1);
        assert!(
            lmdeploy[0]
                .support_units
                .iter()
                .all(|unit| unit.status == InferenceEngineSupportStatus::Connected)
        );
        assert!(registry.adapter(InferenceEngineKind::LmDeploy).is_some());
        let tensorrt_llm = registry
            .manifest_registry()
            .manifests_for_engine(InferenceEngineKind::TensorRtLlm);
        assert_eq!(tensorrt_llm.len(), 1);
        assert!(
            tensorrt_llm[0]
                .support_units
                .iter()
                .all(|unit| unit.status == InferenceEngineSupportStatus::Connected)
        );
        assert!(registry.adapter(InferenceEngineKind::TensorRtLlm).is_some());
    }

    #[test]
    fn standard_registry_matches_the_versioned_support_matrix() {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Matrix {
            schema_version: u16,
            engines: Vec<MatrixEngine>,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct MatrixEngine {
            engine: InferenceEngineKind,
            adapter_variant: String,
            contract_revision: String,
            support_units: Vec<InferenceEngineSupportUnit>,
        }

        let matrix = serde_json::from_str::<Matrix>(include_str!(
            "../../../contracts/inference-engines/v1-support-matrix.json"
        ))
        .expect("support matrix contract");
        assert_eq!(matrix.schema_version, 1);
        let matrix_engine_kinds = matrix
            .engines
            .iter()
            .map(|entry| entry.engine.storage_key())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(matrix_engine_kinds.len(), InferenceEngineKind::ALL.len());

        let registry = ExternalInferenceEngineRegistry::standard().expect("standard registry");
        for entry in matrix.engines {
            if entry.engine == InferenceEngineKind::LlamaCpp {
                // The managed llama.cpp adapter is owned by EngineState, not this external
                // registry; its matrix row is still checked for presence and stable identity.
                assert_eq!(entry.adapter_variant, "hal100-managed-metal");
                continue;
            }
            let adapter_id = EngineAdapterId {
                engine: entry.engine,
                variant: entry.adapter_variant.clone(),
                contract_revision: entry.contract_revision.clone(),
            };
            let adapter = registry
                .adapter_by_id(&adapter_id)
                .expect("matrix entry has one exact external adapter");
            let manifest = adapter.manifest();
            assert_eq!(
                manifest.adapter_id.contract_revision,
                entry.contract_revision
            );
            let actual_units = manifest
                .support_units
                .iter()
                .map(|unit| {
                    (
                        unit.platform,
                        unit.architecture,
                        unit.accelerator,
                        unit.deployment,
                        unit.status,
                    )
                })
                .collect::<Vec<_>>();
            let matrix_units = entry
                .support_units
                .iter()
                .map(|unit| {
                    (
                        unit.platform,
                        unit.architecture,
                        unit.accelerator,
                        unit.deployment,
                        unit.status,
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(actual_units, matrix_units);
        }
    }

    #[test]
    fn acceptance_evidence_gate_fails_closed_until_formal_cells_are_imported() {
        let ledger = crate::InferenceEngineAcceptanceLedger::standard().expect("ledger");
        let adapter = Arc::new(OllamaExternalEngineAdapter::new().expect("Ollama adapter"));
        assert_eq!(
            ExternalInferenceEngineRegistry::new_with_acceptance_evidence(vec![adapter], &ledger)
                .err()
                .map(|error| error.to_string()),
            Some("外部推理引擎缺少与支持单元匹配的验收证据".to_owned())
        );
    }

    #[test]
    fn protocol_capability_hash_is_canonical_for_order_and_duplicates() {
        let canonical = protocol_capability_set([
            EngineProtocolCapability::ModelsList,
            EngineProtocolCapability::ToolCallsSingle,
        ]);
        let noncanonical = EngineProtocolCapabilitySet {
            revision: canonical.revision.clone(),
            capabilities: vec![
                EngineProtocolCapability::ToolCallsSingle,
                EngineProtocolCapability::ModelsList,
                EngineProtocolCapability::ToolCallsSingle,
            ],
        };
        assert_eq!(
            protocol_capability_hash(&canonical),
            protocol_capability_hash(&noncanonical)
        );
    }
}
