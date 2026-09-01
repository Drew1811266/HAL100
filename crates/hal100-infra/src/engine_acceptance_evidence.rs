use std::{collections::HashSet, fs::OpenOptions, io::Write, path::Path};

use hal100_protocol::{
    EngineAdapterId, HostCapabilitySnapshot, InferenceAccelerator, InferenceArchitecture,
    InferenceDeployment, InferenceEngineKind, InferenceEngineManifest,
    InferenceEngineSupportEvidenceKind, InferenceEngineSupportStatus, InferencePlatform,
    RuntimeProfileEvidence, RuntimeProfileEvidenceKind,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION: u16 = 4;
pub const INFERENCE_ENGINE_ACCEPTANCE_RUN_SCHEMA_VERSION: u16 = 4;
pub const INFERENCE_ENGINE_NATIVE_HOST_ATTESTATION_REVISION: &str = "native-host-attestation-v1";
const LEGACY_INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION: u16 = 1;
const TYPED_HOST_INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION: u16 = 2;
const PERFORMANCE_INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION: u16 = 3;
const NATIVE_HOST_PROBE_REVISION: &str = "host-capabilities-v3";
const MAX_LEDGER_BYTES: usize = 2 * 1024 * 1024;
const MAX_RUN_BYTES: usize = 512 * 1024;
const MAX_RECORDS: usize = 512;
const MAX_RECORD_ID_BYTES: usize = 96;
const MAX_INSTANCE_ID_BYTES: usize = 128;
const MAX_CONFIG_REVISION: u64 = i64::MAX as u64;
const MAX_TEXT_BYTES: usize = 512;
const MAX_SOURCE_BYTES: usize = 256;
const MAX_EVIDENCE_PER_RECORD: usize = 7;

/// A single explicit live-acceptance run emitted for human review.
///
/// This artifact is intentionally not a promotion record. It may contain a partial evidence
/// set (for example, a protocol/lifecycle run without stability evidence) and therefore cannot
/// satisfy the formal-support ledger gate by itself. Test entry points only emit it when the
/// caller opts in explicitly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InferenceEngineAcceptanceRun {
    pub schema_version: u16,
    pub run_id: String,
    pub adapter_id: EngineAdapterId,
    /// Rust-validated identity of the concrete service instance that produced this run.
    /// This is not an endpoint and grants no execution authority.
    pub instance_id: String,
    /// SHA-256 of the validated origin; the raw API root never enters the artifact.
    pub origin_fingerprint: String,
    /// Backend/configuration revision bound to the acceptance target.
    pub config_revision: u64,
    /// Canonical hash of the protocol capabilities proven by the qualification probe.
    pub protocol_capability_hash: String,
    pub platform: InferencePlatform,
    pub architecture: InferenceArchitecture,
    pub accelerator: InferenceAccelerator,
    pub deployment: InferenceDeployment,
    pub outcome: InferenceEngineAcceptanceRunOutcome,
    pub observed_at_ms: i64,
    #[serde(default)]
    pub engine_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment_fingerprint: Option<String>,
    #[serde(default)]
    pub model_revision: Option<String>,
    #[serde(default)]
    pub host_summary: Option<String>,
    /// Privacy-safe, typed evidence of the native host class used for this exact support cell.
    ///
    /// The fingerprint binds non-secret hardware-class facts from `NativeSystemProbe`; it never
    /// contains a serial number, storage path, endpoint, command or credential. Run schema v4
    /// accepts only fresh native-probe attestations, never the legacy summary marker.
    pub host_attestation: InferenceEngineAcceptanceHostAttestation,
    /// Privacy-safe fingerprint of the exact typed model evidence used by the lifecycle and
    /// stability probes. This prevents one model's measurements from being generalized to every
    /// model served by the same engine.
    pub model_evidence: InferenceEngineAcceptanceModelEvidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stability: Option<InferenceEngineAcceptanceStability>,
    /// Structured control-plane resilience checks captured by the acceptance harness.
    ///
    /// This remains optional on a live run because an ordinary protocol/lifecycle probe may be
    /// intentionally partial. A run cannot be imported as a formal support record unless all
    /// three checks are present and pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resilience: Option<InferenceEngineAcceptanceResilience>,
    pub evidence: Vec<InferenceEngineAcceptanceEvidence>,
}

/// Aggregate, redacted measurements from the bounded repeated-request acceptance probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InferenceEngineAcceptanceStability {
    pub workload_revision: String,
    pub attempts: u16,
    pub concurrency: u8,
    pub p95_latency_ms: u64,
    pub max_latency_ms: u64,
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub wall_time_ms: u64,
}

/// Redacted identity of one exact runtime-profile model evidence value.
///
/// The raw evidence value can be a user model identifier or deployment path, so acceptance
/// artifacts retain only its kind, bounded algorithm and a domain-separated SHA-256. The same
/// constructor is used when matching a saved runtime profile to reviewed performance evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InferenceEngineAcceptanceModelEvidence {
    pub kind: RuntimeProfileEvidenceKind,
    pub algorithm: String,
    pub value_fingerprint: String,
}

impl InferenceEngineAcceptanceModelEvidence {
    pub fn from_runtime_evidence(
        evidence: &RuntimeProfileEvidence,
    ) -> Result<Self, InferenceEngineAcceptanceEvidenceError> {
        if !valid_component(&evidence.algorithm, MAX_RECORD_ID_BYTES)
            || !valid_text(&evidence.value, MAX_TEXT_BYTES)
        {
            return Err(InferenceEngineAcceptanceEvidenceError::InvalidModelEvidence);
        }
        let mut hasher = Sha256::new();
        hash_component(&mut hasher, "hal100-acceptance-model-evidence-v1");
        hash_component(&mut hasher, model_evidence_kind_key(evidence.kind));
        hash_component(&mut hasher, evidence.algorithm.as_str());
        hash_component(&mut hasher, evidence.value.as_str());
        Ok(Self {
            kind: evidence.kind,
            algorithm: evidence.algorithm.clone(),
            value_fingerprint: format!("{:x}", hasher.finalize()),
        })
    }

    pub fn matches_runtime_evidence(&self, evidence: &RuntimeProfileEvidence) -> bool {
        Self::from_runtime_evidence(evidence).as_ref() == Ok(self)
    }

    fn validate(&self) -> Result<(), InferenceEngineAcceptanceEvidenceError> {
        if !valid_component(&self.algorithm, MAX_RECORD_ID_BYTES)
            || !valid_fingerprint(&self.value_fingerprint)
        {
            return Err(InferenceEngineAcceptanceEvidenceError::InvalidModelEvidence);
        }
        Ok(())
    }
}

/// Redacted evidence that HAL100's control plane preserves safety under interruption and
/// recovery. These checks exercise the shared gateway/runtime-profile transaction boundary; they
/// do not expose endpoint, credential, command or process details.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InferenceEngineAcceptanceResilience {
    pub cancellation_verified: bool,
    pub failed_switch_rollback_verified: bool,
    pub restart_compensation_verified: bool,
}

impl InferenceEngineAcceptanceResilience {
    pub const fn complete() -> Self {
        Self {
            cancellation_verified: true,
            failed_switch_rollback_verified: true,
            restart_compensation_verified: true,
        }
    }

    pub const fn all_passed(self) -> bool {
        self.cancellation_verified
            && self.failed_switch_rollback_verified
            && self.restart_compensation_verified
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InferenceEngineAcceptanceRunOutcome {
    Passed,
    Failed,
}

/// Revision discriminator for acceptance host evidence.
///
/// `LegacyHostSummaryV1` exists only so the three reviewed v1 records remain readable after the
/// v2 ledger migration. New runs and all append/requalification operations reject it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InferenceEngineAcceptanceHostAttestationKind {
    LegacyHostSummaryV1,
    NativeHostProbeV1,
}

/// Redacted host-class evidence bound to one exact platform support cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InferenceEngineAcceptanceHostAttestation {
    pub kind: InferenceEngineAcceptanceHostAttestationKind,
    pub platform: InferencePlatform,
    pub architecture: InferenceArchitecture,
    pub accelerator: InferenceAccelerator,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_class_fingerprint: Option<String>,
}

impl InferenceEngineAcceptanceHostAttestation {
    /// Build a stable, privacy-safe device-class attestation from a native capability snapshot.
    /// Volatile storage capacity/path values are deliberately excluded from the fingerprint.
    pub fn from_host_snapshot(
        host: &HostCapabilitySnapshot,
        accelerator: InferenceAccelerator,
    ) -> Result<Self, InferenceEngineAcceptanceEvidenceError> {
        if host.probe_revision != NATIVE_HOST_PROBE_REVISION
            || !valid_text(&host.cpu_brand, MAX_TEXT_BYTES)
            || !valid_text(&host.device_model, MAX_TEXT_BYTES)
            || matches!(host.cpu_brand.as_str(), "Linux CPU")
            || matches!(host.device_model.as_str(), "Linux host")
            || host.total_memory_bytes == 0
            || host.physical_cpu_cores == 0
            || host.logical_cpu_cores == 0
            || host.logical_cpu_cores < host.physical_cpu_cores
            || host.accelerators.is_empty()
            || host.accelerators.len() > 16
            || !host.accelerators.contains(&accelerator)
        {
            return Err(InferenceEngineAcceptanceEvidenceError::InvalidHostAttestation);
        }
        let mut accelerator_keys = host
            .accelerators
            .iter()
            .copied()
            .map(accelerator_key)
            .collect::<Vec<_>>();
        accelerator_keys.sort_unstable();
        if accelerator_keys.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(InferenceEngineAcceptanceEvidenceError::InvalidHostAttestation);
        }

        let mut hasher = Sha256::new();
        hash_component(
            &mut hasher,
            INFERENCE_ENGINE_NATIVE_HOST_ATTESTATION_REVISION,
        );
        hash_component(&mut hasher, platform_key(host.platform));
        hash_component(&mut hasher, architecture_key(host.architecture));
        hash_component(&mut hasher, accelerator_key(accelerator));
        hash_component(&mut hasher, host.probe_revision.as_str());
        hash_component(&mut hasher, host.cpu_brand.trim());
        hash_component(&mut hasher, host.device_model.trim());
        hasher.update(host.total_memory_bytes.to_be_bytes());
        hasher.update(host.physical_cpu_cores.to_be_bytes());
        hasher.update(host.logical_cpu_cores.to_be_bytes());
        for key in accelerator_keys {
            hash_component(&mut hasher, key);
        }

        Ok(Self {
            kind: InferenceEngineAcceptanceHostAttestationKind::NativeHostProbeV1,
            platform: host.platform,
            architecture: host.architecture,
            accelerator,
            probe_revision: Some(host.probe_revision.clone()),
            device_class_fingerprint: Some(format!("{:x}", hasher.finalize())),
        })
    }

    fn validate_for_cell(
        &self,
        platform: InferencePlatform,
        architecture: InferenceArchitecture,
        accelerator: InferenceAccelerator,
        allow_legacy: bool,
    ) -> Result<(), InferenceEngineAcceptanceEvidenceError> {
        if self.platform != platform
            || self.architecture != architecture
            || self.accelerator != accelerator
        {
            return Err(InferenceEngineAcceptanceEvidenceError::InvalidHostAttestation);
        }
        match self.kind {
            InferenceEngineAcceptanceHostAttestationKind::LegacyHostSummaryV1
                if allow_legacy
                    && self.probe_revision.is_none()
                    && self.device_class_fingerprint.is_none() =>
            {
                Ok(())
            }
            InferenceEngineAcceptanceHostAttestationKind::NativeHostProbeV1
                if self.probe_revision.as_deref() == Some(NATIVE_HOST_PROBE_REVISION)
                    && self
                        .device_class_fingerprint
                        .as_deref()
                        .is_some_and(valid_fingerprint) =>
            {
                Ok(())
            }
            _ => Err(InferenceEngineAcceptanceEvidenceError::InvalidHostAttestation),
        }
    }

    fn is_native(&self) -> bool {
        self.kind == InferenceEngineAcceptanceHostAttestationKind::NativeHostProbeV1
    }
}

#[cfg(test)]
pub(crate) fn test_native_host_attestation(
    platform: InferencePlatform,
    architecture: InferenceArchitecture,
    accelerator: InferenceAccelerator,
) -> InferenceEngineAcceptanceHostAttestation {
    InferenceEngineAcceptanceHostAttestation {
        kind: InferenceEngineAcceptanceHostAttestationKind::NativeHostProbeV1,
        platform,
        architecture,
        accelerator,
        probe_revision: Some(NATIVE_HOST_PROBE_REVISION.to_owned()),
        device_class_fingerprint: Some(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned(),
        ),
    }
}

#[cfg(test)]
pub(crate) fn test_stability_profile() -> InferenceEngineAcceptanceStability {
    InferenceEngineAcceptanceStability {
        workload_revision: crate::openai_protocol_qualification::OPENAI_STABILITY_WORKLOAD_REVISION
            .to_owned(),
        attempts: 20,
        concurrency: 4,
        p95_latency_ms: 90,
        max_latency_ms: 100,
        total_prompt_tokens: 40,
        total_completion_tokens: 20,
        wall_time_ms: 500,
    }
}

#[cfg(test)]
pub(crate) fn test_model_evidence() -> InferenceEngineAcceptanceModelEvidence {
    InferenceEngineAcceptanceModelEvidence::from_runtime_evidence(&RuntimeProfileEvidence {
        kind: RuntimeProfileEvidenceKind::CatalogIdentity,
        algorithm: "acceptance-test-model-id".to_owned(),
        value: "acceptance-test-model".to_owned(),
    })
    .expect("bounded test model evidence")
}

/// Versioned, redacted acceptance evidence imported from an explicit test run.
///
/// The ledger is deliberately separate from an engine manifest: a manifest declares what the
/// adapter supports, while this ledger records which exact support cell has actually been tested.
/// It contains no credentials, endpoints, commands, process arguments, model paths or raw model
/// output. The checked-in ledger is empty until a real acceptance run produces a reviewed record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InferenceEngineAcceptanceLedger {
    pub schema_version: u16,
    pub records: Vec<InferenceEngineAcceptanceRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InferenceEngineAcceptanceRecord {
    pub id: String,
    pub adapter_id: EngineAdapterId,
    /// Rust-validated identity of the concrete service instance used for acceptance.
    pub instance_id: String,
    /// SHA-256 of the validated origin; raw endpoints and credentials are never persisted.
    pub origin_fingerprint: String,
    /// Backend/configuration revision bound to the acceptance target.
    pub config_revision: u64,
    /// Canonical hash of the protocol capabilities proven by the qualification probe.
    pub protocol_capability_hash: String,
    pub platform: InferencePlatform,
    pub architecture: InferenceArchitecture,
    pub accelerator: InferenceAccelerator,
    pub deployment: InferenceDeployment,
    pub status: InferenceEngineSupportStatus,
    pub verified_at_ms: i64,
    #[serde(default)]
    pub engine_version: Option<String>,
    #[serde(default)]
    pub deployment_fingerprint: Option<String>,
    #[serde(default)]
    pub model_revision: Option<String>,
    #[serde(default)]
    pub host_summary: Option<String>,
    /// V2+ host evidence. It is optional only while decoding the immutable v1 ledger.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_attestation: Option<InferenceEngineAcceptanceHostAttestation>,
    /// V4 model-scope evidence. Historical v1-v3 records keep this absent rather than pretending
    /// their free-form model revision is equivalent to a runtime-profile evidence value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_evidence: Option<InferenceEngineAcceptanceModelEvidence>,
    /// V3 workload-bound aggregate performance measurements. Historical v1/v2 records keep this
    /// absent rather than fabricating values that were discarded by the old importer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stability: Option<InferenceEngineAcceptanceStability>,
    /// The same bounded control-plane resilience evidence that was reviewed for the source run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resilience: Option<InferenceEngineAcceptanceResilience>,
    pub evidence: Vec<InferenceEngineAcceptanceEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InferenceEngineAcceptanceEvidence {
    pub kind: InferenceEngineSupportEvidenceKind,
    pub source: String,
    pub assertion: String,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum InferenceEngineAcceptanceEvidenceError {
    #[error("推理引擎验收证据账本JSON无效")]
    InvalidJson,
    #[error("推理引擎验收证据账本版本不受支持")]
    UnsupportedSchemaVersion,
    #[error("推理引擎验收证据账本超出大小或记录上限")]
    BoundsExceeded,
    #[error("推理引擎验收证据记录无效")]
    InvalidRecord,
    #[error("推理引擎验收证据记录ID重复")]
    DuplicateRecord,
    #[error("推理引擎验收证据支持格重复")]
    DuplicateSupportCell,
    #[error("正式支持单元缺少真实验收证据记录")]
    MissingFormalRecord,
    #[error("推理引擎验收证据记录与manifest支持格不一致")]
    RecordMismatch,
    #[error("推理引擎验收运行产物无效")]
    InvalidRun,
    #[error("推理引擎验收运行产物输出不可用")]
    RunOutputUnavailable,
    #[error("推理引擎验收宿主设备证据无效")]
    InvalidHostAttestation,
    #[error("推理引擎验收模型证据无效")]
    InvalidModelEvidence,
}

impl InferenceEngineAcceptanceRun {
    pub fn validate(&self) -> Result<(), InferenceEngineAcceptanceEvidenceError> {
        if self.schema_version != INFERENCE_ENGINE_ACCEPTANCE_RUN_SCHEMA_VERSION {
            return Err(InferenceEngineAcceptanceEvidenceError::UnsupportedSchemaVersion);
        }
        if !valid_component(&self.run_id, MAX_RECORD_ID_BYTES)
            || !valid_component(&self.adapter_id.variant, MAX_RECORD_ID_BYTES)
            || !valid_component(&self.adapter_id.contract_revision, MAX_RECORD_ID_BYTES)
            || !valid_instance_id(&self.instance_id)
            || !valid_fingerprint(&self.origin_fingerprint)
            || self.config_revision == 0
            || self.config_revision > MAX_CONFIG_REVISION
            || !valid_fingerprint(&self.protocol_capability_hash)
            || self.observed_at_ms <= 0
            || self.evidence.len() > MAX_EVIDENCE_PER_RECORD
        {
            return Err(InferenceEngineAcceptanceEvidenceError::InvalidRun);
        }
        if matches!(self.outcome, InferenceEngineAcceptanceRunOutcome::Passed)
            && self.evidence.is_empty()
        {
            return Err(InferenceEngineAcceptanceEvidenceError::InvalidRun);
        }
        self.host_attestation.validate_for_cell(
            self.platform,
            self.architecture,
            self.accelerator,
            false,
        )?;
        if !self.host_attestation.is_native() {
            return Err(InferenceEngineAcceptanceEvidenceError::InvalidHostAttestation);
        }
        self.model_evidence
            .validate()
            .map_err(|_| InferenceEngineAcceptanceEvidenceError::InvalidRun)?;
        let expected_host_summary =
            canonical_host_summary(self.platform, self.architecture, self.accelerator);
        if self.host_summary.as_deref() != Some(expected_host_summary.as_str()) {
            return Err(InferenceEngineAcceptanceEvidenceError::InvalidHostAttestation);
        }
        for value in [
            self.engine_version.as_deref(),
            self.model_revision.as_deref(),
            self.host_summary.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if !valid_text(value, MAX_TEXT_BYTES) {
                return Err(InferenceEngineAcceptanceEvidenceError::InvalidRun);
            }
        }
        if self
            .deployment_fingerprint
            .as_deref()
            .is_some_and(|value| !valid_fingerprint(value))
        {
            return Err(InferenceEngineAcceptanceEvidenceError::InvalidRun);
        }
        if let Some(stability) = &self.stability {
            validate_stability(stability)
                .map_err(|_| InferenceEngineAcceptanceEvidenceError::InvalidRun)?;
        }
        let mut kinds = HashSet::with_capacity(self.evidence.len());
        for evidence in &self.evidence {
            if !kinds.insert(evidence.kind) || !valid_evidence(evidence) {
                return Err(InferenceEngineAcceptanceEvidenceError::InvalidRun);
            }
        }
        let has_stability_evidence = self
            .evidence
            .iter()
            .any(|evidence| evidence.kind == InferenceEngineSupportEvidenceKind::Stability);
        if has_stability_evidence != self.stability.is_some() {
            return Err(InferenceEngineAcceptanceEvidenceError::InvalidRun);
        }
        Ok(())
    }

    pub fn to_json(&self) -> Result<Vec<u8>, InferenceEngineAcceptanceEvidenceError> {
        self.validate()?;
        serde_json::to_vec_pretty(self)
            .map_err(|_| InferenceEngineAcceptanceEvidenceError::InvalidRun)
    }

    /// Convert a passed run into a formal ledger record only after all seven evidence kinds and
    /// complete control-plane resilience evidence are present. This is an in-memory review step;
    /// callers still need to explicitly write the resulting record into the checked-in ledger and
    /// run its manifest gate.
    pub fn into_formal_record(
        self,
        status: InferenceEngineSupportStatus,
    ) -> Result<InferenceEngineAcceptanceRecord, InferenceEngineAcceptanceEvidenceError> {
        self.validate()?;
        if !matches!(
            status,
            InferenceEngineSupportStatus::Managed | InferenceEngineSupportStatus::VerifiedExternal
        ) || !matches!(self.outcome, InferenceEngineAcceptanceRunOutcome::Passed)
        {
            return Err(InferenceEngineAcceptanceEvidenceError::InvalidRecord);
        }
        let record = InferenceEngineAcceptanceRecord {
            id: self.run_id,
            adapter_id: self.adapter_id,
            instance_id: self.instance_id,
            origin_fingerprint: self.origin_fingerprint,
            config_revision: self.config_revision,
            protocol_capability_hash: self.protocol_capability_hash,
            platform: self.platform,
            architecture: self.architecture,
            accelerator: self.accelerator,
            deployment: self.deployment,
            status,
            verified_at_ms: self.observed_at_ms,
            engine_version: self.engine_version,
            deployment_fingerprint: self.deployment_fingerprint,
            model_revision: self.model_revision,
            host_summary: self.host_summary,
            host_attestation: Some(self.host_attestation),
            model_evidence: Some(self.model_evidence),
            stability: self.stability,
            resilience: self.resilience,
            evidence: self.evidence,
        };
        validate_record(&record, INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION)?;
        Ok(record)
    }

    /// Convert a passed run into a formal record after an explicit human review of the model
    /// identity.
    ///
    /// Live test artifacts deliberately contain a `model-id-sha256:*` correlation value instead
    /// of a model revision that can be independently checked. A reviewer must provide the exact
    /// immutable revision (for example, a repository commit or a deployment build identifier)
    /// before this method can produce a promotion record. The supplied revision is validated by
    /// the same bounded record gate and is never inferred from the model name.
    pub fn into_formal_record_with_model_revision(
        mut self,
        status: InferenceEngineSupportStatus,
        model_revision: &str,
    ) -> Result<InferenceEngineAcceptanceRecord, InferenceEngineAcceptanceEvidenceError> {
        if !valid_text(model_revision, MAX_TEXT_BYTES)
            || model_revision.starts_with("model-id-sha256:")
        {
            return Err(InferenceEngineAcceptanceEvidenceError::InvalidRecord);
        }
        self.model_revision = Some(model_revision.to_owned());
        self.into_formal_record(status)
    }
}

/// Serialize one live run to an explicitly selected, create-new output file.
///
/// The function never creates parent directories and never overwrites an existing path. This is
/// intended for ignored acceptance tests with an explicit output path, not for normal runtime
/// code or user-provided application input.
pub fn write_acceptance_run_exclusive(
    path: &Path,
    run: &InferenceEngineAcceptanceRun,
) -> Result<(), InferenceEngineAcceptanceEvidenceError> {
    let bytes = run.to_json()?;
    if bytes.len() > MAX_RUN_BYTES {
        return Err(InferenceEngineAcceptanceEvidenceError::BoundsExceeded);
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| InferenceEngineAcceptanceEvidenceError::RunOutputUnavailable)?;
    file.write_all(&bytes)
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_all())
        .map_err(|_| InferenceEngineAcceptanceEvidenceError::RunOutputUnavailable)
}

impl InferenceEngineAcceptanceLedger {
    pub fn parse(bytes: &[u8]) -> Result<Self, InferenceEngineAcceptanceEvidenceError> {
        if bytes.len() > MAX_LEDGER_BYTES {
            return Err(InferenceEngineAcceptanceEvidenceError::BoundsExceeded);
        }
        let ledger = serde_json::from_slice::<Self>(bytes)
            .map_err(|_| InferenceEngineAcceptanceEvidenceError::InvalidJson)?;
        ledger.validate()?;
        Ok(ledger)
    }

    pub fn standard() -> Result<Self, InferenceEngineAcceptanceEvidenceError> {
        Self::parse(include_bytes!(
            "../../../contracts/inference-engines/v4-acceptance-evidence.json"
        ))
    }

    pub fn validate(&self) -> Result<(), InferenceEngineAcceptanceEvidenceError> {
        if !matches!(
            self.schema_version,
            LEGACY_INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION
                | TYPED_HOST_INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION
                | PERFORMANCE_INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION
                | INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION
        ) {
            return Err(InferenceEngineAcceptanceEvidenceError::UnsupportedSchemaVersion);
        }
        if self.records.len() > MAX_RECORDS {
            return Err(InferenceEngineAcceptanceEvidenceError::BoundsExceeded);
        }
        let mut ids = HashSet::with_capacity(self.records.len());
        let mut support_cells = HashSet::with_capacity(self.records.len());
        for record in &self.records {
            if !ids.insert(record.id.clone()) {
                return Err(InferenceEngineAcceptanceEvidenceError::DuplicateRecord);
            }
            if !support_cells.insert((
                record.adapter_id.clone(),
                record.platform,
                record.architecture,
                record.accelerator,
                record.deployment,
            )) {
                return Err(InferenceEngineAcceptanceEvidenceError::DuplicateSupportCell);
            }
            validate_record(record, self.schema_version)?;
        }
        Ok(())
    }

    /// Append one already reviewed record without partially mutating the ledger on failure.
    ///
    /// The record is validated before insertion and the complete candidate ledger is validated
    /// again after insertion. This method is intentionally in-memory; writing the checked-in
    /// contract remains an explicit repository operation.
    pub fn append_reviewed_record(
        &mut self,
        record: InferenceEngineAcceptanceRecord,
    ) -> Result<(), InferenceEngineAcceptanceEvidenceError> {
        self.validate()?;
        if self.schema_version != INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION
            || !record
                .host_attestation
                .as_ref()
                .is_some_and(InferenceEngineAcceptanceHostAttestation::is_native)
        {
            return Err(InferenceEngineAcceptanceEvidenceError::InvalidHostAttestation);
        }
        validate_record(&record, self.schema_version)?;
        if self.records.iter().any(|existing| existing.id == record.id) {
            return Err(InferenceEngineAcceptanceEvidenceError::DuplicateRecord);
        }
        if self.records.iter().any(|existing| {
            existing.adapter_id == record.adapter_id
                && existing.platform == record.platform
                && existing.architecture == record.architecture
                && existing.accelerator == record.accelerator
                && existing.deployment == record.deployment
        }) {
            return Err(InferenceEngineAcceptanceEvidenceError::DuplicateSupportCell);
        }
        self.records.push(record);
        if let Err(error) = self.validate() {
            self.records.pop();
            return Err(error);
        }
        Ok(())
    }

    /// Atomically supersede one reviewed record with a newer run for the exact same support cell.
    ///
    /// Re-qualification is intentionally explicit: callers must name the old record id, and the
    /// replacement cannot move evidence between adapters, platforms, architectures,
    /// accelerators or deployments. The checked-in ledger is still written only as a separate
    /// create-new candidate by the import CLI.
    pub fn replace_reviewed_record(
        &mut self,
        existing_record_id: &str,
        record: InferenceEngineAcceptanceRecord,
    ) -> Result<(), InferenceEngineAcceptanceEvidenceError> {
        self.validate()?;
        if self.schema_version != INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION
            || !record
                .host_attestation
                .as_ref()
                .is_some_and(InferenceEngineAcceptanceHostAttestation::is_native)
        {
            return Err(InferenceEngineAcceptanceEvidenceError::InvalidHostAttestation);
        }
        validate_record(&record, self.schema_version)?;
        let index = self
            .records
            .iter()
            .position(|existing| existing.id == existing_record_id)
            .ok_or(InferenceEngineAcceptanceEvidenceError::RecordMismatch)?;
        let existing = &self.records[index];
        if existing.adapter_id != record.adapter_id
            || existing.platform != record.platform
            || existing.architecture != record.architecture
            || existing.accelerator != record.accelerator
            || existing.deployment != record.deployment
        {
            return Err(InferenceEngineAcceptanceEvidenceError::RecordMismatch);
        }
        if self
            .records
            .iter()
            .enumerate()
            .any(|(other_index, other)| other_index != index && other.id == record.id)
        {
            return Err(InferenceEngineAcceptanceEvidenceError::DuplicateRecord);
        }
        let previous = std::mem::replace(&mut self.records[index], record);
        if let Err(error) = self.validate() {
            self.records[index] = previous;
            return Err(error);
        }
        Ok(())
    }

    /// Review and append a complete formal record derived from a live run artifact.
    pub fn append_run_as_formal(
        &mut self,
        run: InferenceEngineAcceptanceRun,
        status: InferenceEngineSupportStatus,
    ) -> Result<(), InferenceEngineAcceptanceEvidenceError> {
        self.append_reviewed_record(run.into_formal_record(status)?)
    }

    /// Require a reviewed record for every formal support cell in a manifest.
    ///
    /// This is intentionally opt-in rather than part of the generic manifest constructor: test
    /// fixtures and adapters can still be assembled before a platform has real evidence, while
    /// the standard promotion gate can require this exact check.
    pub fn validate_manifest(
        &self,
        manifest: &InferenceEngineManifest,
    ) -> Result<(), InferenceEngineAcceptanceEvidenceError> {
        self.validate()?;
        for unit in &manifest.support_units {
            let record = self.record_for(
                &manifest.adapter_id,
                unit.platform,
                unit.architecture,
                unit.accelerator,
                unit.deployment,
            );
            let Some(record) = record else {
                if matches!(
                    unit.status,
                    InferenceEngineSupportStatus::Managed
                        | InferenceEngineSupportStatus::VerifiedExternal
                ) {
                    return Err(InferenceEngineAcceptanceEvidenceError::MissingFormalRecord);
                }
                continue;
            };
            if record.status != unit.status {
                return Err(InferenceEngineAcceptanceEvidenceError::RecordMismatch);
            }
        }
        if self.records.iter().any(|record| {
            record.adapter_id == manifest.adapter_id
                && !manifest.support_units.iter().any(|unit| {
                    unit.platform == record.platform
                        && unit.architecture == record.architecture
                        && unit.accelerator == record.accelerator
                        && unit.deployment == record.deployment
                })
        }) {
            return Err(InferenceEngineAcceptanceEvidenceError::RecordMismatch);
        }
        Ok(())
    }

    pub fn record_for(
        &self,
        adapter_id: &EngineAdapterId,
        platform: InferencePlatform,
        architecture: InferenceArchitecture,
        accelerator: InferenceAccelerator,
        deployment: InferenceDeployment,
    ) -> Option<&InferenceEngineAcceptanceRecord> {
        self.records.iter().find(|record| {
            record.adapter_id == *adapter_id
                && record.platform == platform
                && record.architecture == architecture
                && record.accelerator == accelerator
                && record.deployment == deployment
        })
    }
}

fn validate_record(
    record: &InferenceEngineAcceptanceRecord,
    schema_version: u16,
) -> Result<(), InferenceEngineAcceptanceEvidenceError> {
    if !valid_component(&record.id, MAX_RECORD_ID_BYTES)
        || !valid_component(&record.adapter_id.variant, MAX_RECORD_ID_BYTES)
        || !valid_component(&record.adapter_id.contract_revision, MAX_RECORD_ID_BYTES)
        || !valid_instance_id(&record.instance_id)
        || !valid_fingerprint(&record.origin_fingerprint)
        || record.config_revision == 0
        || record.config_revision > MAX_CONFIG_REVISION
        || !valid_fingerprint(&record.protocol_capability_hash)
        || record.verified_at_ms <= 0
        || record.evidence.len() > MAX_EVIDENCE_PER_RECORD
    {
        return Err(InferenceEngineAcceptanceEvidenceError::InvalidRecord);
    }
    if record.evidence.is_empty()
        && matches!(
            record.status,
            InferenceEngineSupportStatus::Managed | InferenceEngineSupportStatus::VerifiedExternal
        )
    {
        return Err(InferenceEngineAcceptanceEvidenceError::InvalidRecord);
    }
    if record
        .deployment_fingerprint
        .as_deref()
        .is_some_and(|value| !valid_fingerprint(value))
    {
        return Err(InferenceEngineAcceptanceEvidenceError::InvalidRecord);
    }
    if record.engine_version.as_deref().is_some_and(|value| {
        !valid_text(value, MAX_TEXT_BYTES) || value == "qualification-required"
    }) {
        return Err(InferenceEngineAcceptanceEvidenceError::InvalidRecord);
    }
    match schema_version {
        LEGACY_INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION => {
            if record.host_attestation.is_some() || record.model_evidence.is_some() {
                return Err(InferenceEngineAcceptanceEvidenceError::InvalidRecord);
            }
        }
        TYPED_HOST_INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION
        | PERFORMANCE_INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION
        | INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION => {
            if schema_version < INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION
                && record.model_evidence.is_some()
            {
                return Err(InferenceEngineAcceptanceEvidenceError::InvalidRecord);
            }
            if let Some(host_attestation) = &record.host_attestation {
                host_attestation.validate_for_cell(
                    record.platform,
                    record.architecture,
                    record.accelerator,
                    true,
                )?;
                if host_attestation.kind
                    == InferenceEngineAcceptanceHostAttestationKind::LegacyHostSummaryV1
                    && !known_legacy_v1_host_record(record)
                {
                    return Err(InferenceEngineAcceptanceEvidenceError::InvalidHostAttestation);
                }
                let expected_host_summary = canonical_host_summary(
                    record.platform,
                    record.architecture,
                    record.accelerator,
                );
                if record.host_summary.as_deref() != Some(expected_host_summary.as_str()) {
                    return Err(InferenceEngineAcceptanceEvidenceError::InvalidHostAttestation);
                }
            }
        }
        _ => {
            return Err(InferenceEngineAcceptanceEvidenceError::UnsupportedSchemaVersion);
        }
    }
    let expected =
        hal100_protocol::InferenceEngineSupportEvidenceSummary::for_status(record.status);
    if let Some(stability) = &record.stability {
        validate_stability(stability)?;
    }
    if let Some(model_evidence) = &record.model_evidence {
        model_evidence.validate()?;
    }
    let mut kinds = HashSet::with_capacity(record.evidence.len());
    for evidence in &record.evidence {
        if !kinds.insert(evidence.kind)
            || !expected.verified.contains(&evidence.kind)
            || !valid_evidence(evidence)
        {
            return Err(InferenceEngineAcceptanceEvidenceError::InvalidRecord);
        }
    }
    if matches!(
        record.status,
        InferenceEngineSupportStatus::Managed | InferenceEngineSupportStatus::VerifiedExternal
    ) && (record.evidence.len() != expected.verified.len()
        || (record.engine_version.is_none() && record.deployment_fingerprint.is_none())
        || record.model_revision.as_deref().is_none_or(|value| {
            !valid_text(value, MAX_TEXT_BYTES) || value.starts_with("model-id-sha256:")
        })
        || record
            .host_summary
            .as_deref()
            .is_none_or(|value| !valid_text(value, MAX_TEXT_BYTES))
        || (matches!(
            schema_version,
            TYPED_HOST_INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION
                | PERFORMANCE_INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION
                | INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION
        ) && record.host_attestation.is_none())
        || (matches!(
            schema_version,
            PERFORMANCE_INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION
                | INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION
        ) && record.stability.is_none()
            && !known_pre_v3_measurement_record(record))
        || (schema_version == INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION
            && record.model_evidence.is_none()
            && !known_pre_v4_model_evidence_record(record))
        || !record
            .resilience
            .is_some_and(|resilience| resilience.all_passed()))
    {
        return Err(InferenceEngineAcceptanceEvidenceError::InvalidRecord);
    }
    Ok(())
}

fn validate_stability(
    stability: &InferenceEngineAcceptanceStability,
) -> Result<(), InferenceEngineAcceptanceEvidenceError> {
    if stability.workload_revision
        != crate::openai_protocol_qualification::OPENAI_STABILITY_WORKLOAD_REVISION
        || stability.attempts == 0
        || stability.attempts > 100
        || stability.concurrency == 0
        || stability.concurrency > 16
        || stability.p95_latency_ms > stability.max_latency_ms
        || stability.max_latency_ms > 120_000
        || stability.total_prompt_tokens == 0
        || stability.total_prompt_tokens > 10_000_000
        || stability.total_completion_tokens == 0
        || stability.total_completion_tokens > 10_000_000
        || stability.wall_time_ms == 0
        || stability.wall_time_ms > 3_600_000
    {
        return Err(InferenceEngineAcceptanceEvidenceError::InvalidRecord);
    }
    Ok(())
}

fn valid_fingerprint(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Exact migration allowlist for the three formal records that predate native host attestation.
/// This prevents a newly authored v2 ledger from using the legacy marker to create another
/// support claim while preserving the original reviewed cells byte-for-byte at the identity
/// boundary.
fn known_legacy_v1_host_record(record: &InferenceEngineAcceptanceRecord) -> bool {
    let identity = (
        record.id.as_str(),
        record.adapter_id.engine,
        record.adapter_id.variant.as_str(),
        record.platform,
        record.architecture,
        record.accelerator,
        record.deployment,
        record.verified_at_ms,
    );
    matches!(
        identity,
        (
            "acceptance-run-c3c9f2742e584ab19b613d32255bea82",
            InferenceEngineKind::Ollama,
            "official-loopback-api",
            InferencePlatform::MacOs,
            InferenceArchitecture::Aarch64,
            InferenceAccelerator::Metal,
            InferenceDeployment::Local,
            1_788_145_725_668,
        ) | (
            "acceptance-run-6a5e1d232cb644b2a7ba7db85b1b910c",
            InferenceEngineKind::MlxLm,
            "official-http-server",
            InferencePlatform::MacOs,
            InferenceArchitecture::Aarch64,
            InferenceAccelerator::Metal,
            InferenceDeployment::Local,
            1_788_144_651_619,
        ) | (
            "acceptance-run-6a23349a43514f41bf8bfa8b811ac69f",
            InferenceEngineKind::Ollama,
            "official-loopback-api",
            InferencePlatform::MacOs,
            InferenceArchitecture::Aarch64,
            InferenceAccelerator::Cpu,
            InferenceDeployment::Local,
            1_788_145_651_933,
        )
    )
}

/// These three records were reviewed before v3 preserved structured measurements in the formal
/// ledger. Their stability evidence text remains historical, but no numeric profile is inferred.
fn known_pre_v3_measurement_record(record: &InferenceEngineAcceptanceRecord) -> bool {
    known_legacy_v1_host_record(record)
}

/// The same three records predate v4's typed model-evidence fingerprint. Their human-reviewed
/// model revision remains historical context but is never treated as a runtime evidence match.
fn known_pre_v4_model_evidence_record(record: &InferenceEngineAcceptanceRecord) -> bool {
    known_legacy_v1_host_record(record)
}

fn hash_component(hasher: &mut Sha256, value: &str) {
    hasher.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    hasher.update(value.as_bytes());
}

const fn platform_key(platform: InferencePlatform) -> &'static str {
    match platform {
        InferencePlatform::MacOs => "macos",
        InferencePlatform::Windows => "windows",
        InferencePlatform::Linux => "linux",
    }
}

const fn architecture_key(architecture: InferenceArchitecture) -> &'static str {
    match architecture {
        InferenceArchitecture::Aarch64 => "aarch64",
        InferenceArchitecture::X86_64 => "x86_64",
    }
}

const fn accelerator_key(accelerator: InferenceAccelerator) -> &'static str {
    match accelerator {
        InferenceAccelerator::Cpu => "cpu",
        InferenceAccelerator::Metal => "metal",
        InferenceAccelerator::Cuda => "cuda",
        InferenceAccelerator::Rocm => "rocm",
        InferenceAccelerator::Vulkan => "vulkan",
        InferenceAccelerator::Sycl => "sycl",
        InferenceAccelerator::IntelGpu => "intel_gpu",
        InferenceAccelerator::IntelNpu => "intel_npu",
    }
}

const fn model_evidence_kind_key(kind: RuntimeProfileEvidenceKind) -> &'static str {
    match kind {
        RuntimeProfileEvidenceKind::ContentDigest => "content_digest",
        RuntimeProfileEvidenceKind::RepositoryRevision => "repository_revision",
        RuntimeProfileEvidenceKind::DeploymentFingerprint => "deployment_fingerprint",
        RuntimeProfileEvidenceKind::CatalogIdentity => "catalog_identity",
    }
}

fn canonical_host_summary(
    platform: InferencePlatform,
    architecture: InferenceArchitecture,
    accelerator: InferenceAccelerator,
) -> String {
    format!(
        "{}/{}/{}",
        platform_key(platform),
        architecture_key(architecture),
        accelerator_key(accelerator)
    )
}

fn valid_instance_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_INSTANCE_ID_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b':'))
}

fn valid_evidence(evidence: &InferenceEngineAcceptanceEvidence) -> bool {
    valid_evidence_source(&evidence.source) && valid_text(&evidence.assertion, MAX_TEXT_BYTES)
}

fn valid_evidence_source(source: &str) -> bool {
    valid_text(source, MAX_SOURCE_BYTES)
        && !source.starts_with('/')
        && !source.starts_with("./")
        && !source.contains("../")
        && !source.contains('\\')
        && !source.contains("://")
        && !source.contains("//")
        && (source == "README.md"
            || ["contracts/", "crates/", "docs/"]
                .iter()
                .any(|prefix| source.starts_with(prefix)))
}

fn valid_component(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'/'))
}

fn valid_text(value: &str, max_bytes: usize) -> bool {
    !value.is_empty() && value.len() <= max_bytes && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use hal100_protocol::{
        InferenceEngineDescriptor, InferenceEngineKind, InferenceEngineOwnership,
        InferenceEngineSupportEvidenceSummary, InferenceEngineSupportUnit, InferenceModelFormat,
        InferenceProtocol,
    };

    use super::*;

    fn host_snapshot(
        platform: InferencePlatform,
        architecture: InferenceArchitecture,
        accelerator: InferenceAccelerator,
    ) -> HostCapabilitySnapshot {
        let mut accelerators = vec![InferenceAccelerator::Cpu];
        if accelerator != InferenceAccelerator::Cpu {
            accelerators.push(accelerator);
        }
        HostCapabilitySnapshot {
            platform,
            architecture,
            cpu_brand: "Acceptance Fixture CPU".to_owned(),
            device_model: "AcceptanceFixtureModel".to_owned(),
            total_memory_bytes: 32 * 1024 * 1024 * 1024,
            physical_cpu_cores: 8,
            logical_cpu_cores: 16,
            accelerators,
            model_storage_path: "/redacted-model-storage".to_owned(),
            model_storage_available_bytes: 100,
            probe_revision: NATIVE_HOST_PROBE_REVISION.to_owned(),
        }
    }

    fn native_host_attestation(
        platform: InferencePlatform,
        architecture: InferenceArchitecture,
        accelerator: InferenceAccelerator,
    ) -> InferenceEngineAcceptanceHostAttestation {
        InferenceEngineAcceptanceHostAttestation::from_host_snapshot(
            &host_snapshot(platform, architecture, accelerator),
            accelerator,
        )
        .expect("native host attestation fixture")
    }

    fn measured_stability(max_latency_ms: u64) -> InferenceEngineAcceptanceStability {
        InferenceEngineAcceptanceStability {
            workload_revision:
                crate::openai_protocol_qualification::OPENAI_STABILITY_WORKLOAD_REVISION.to_owned(),
            attempts: 20,
            concurrency: 4,
            p95_latency_ms: max_latency_ms.saturating_sub(10),
            max_latency_ms,
            total_prompt_tokens: 40,
            total_completion_tokens: 20,
            wall_time_ms: 500,
        }
    }

    fn formal_record() -> InferenceEngineAcceptanceRecord {
        let status = InferenceEngineSupportStatus::VerifiedExternal;
        InferenceEngineAcceptanceRecord {
            id: "mlx-lm-macos-aarch64-metal-20260827".to_owned(),
            adapter_id: EngineAdapterId {
                engine: InferenceEngineKind::MlxLm,
                variant: "official-http-server".to_owned(),
                contract_revision: hal100_protocol::ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
            },
            instance_id: "acceptance:mlx-lm".to_owned(),
            origin_fingerprint: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .to_owned(),
            config_revision: 1,
            protocol_capability_hash:
                "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789".to_owned(),
            platform: InferencePlatform::MacOs,
            architecture: InferenceArchitecture::Aarch64,
            accelerator: InferenceAccelerator::Metal,
            deployment: InferenceDeployment::Local,
            status,
            verified_at_ms: 1,
            engine_version: Some("0.31.3".to_owned()),
            deployment_fingerprint: None,
            model_revision: Some("mlx-community/Qwen3-0.6B-4bit".to_owned()),
            host_summary: Some("macos/aarch64/metal".to_owned()),
            host_attestation: Some(native_host_attestation(
                InferencePlatform::MacOs,
                InferenceArchitecture::Aarch64,
                InferenceAccelerator::Metal,
            )),
            model_evidence: Some(test_model_evidence()),
            stability: Some(measured_stability(250)),
            resilience: Some(InferenceEngineAcceptanceResilience::complete()),
            evidence: InferenceEngineSupportEvidenceSummary::for_status(status)
                .verified
                .into_iter()
                .map(|kind| InferenceEngineAcceptanceEvidence {
                    kind,
                    source: "docs/ITERATION_54_CHECKPOINT.md".to_owned(),
                    assertion: "explicit acceptance assertion".to_owned(),
                })
                .collect(),
        }
    }

    fn run_artifact() -> InferenceEngineAcceptanceRun {
        InferenceEngineAcceptanceRun {
            schema_version: INFERENCE_ENGINE_ACCEPTANCE_RUN_SCHEMA_VERSION,
            run_id: "run-20260828-vllm".to_owned(),
            adapter_id: EngineAdapterId {
                engine: InferenceEngineKind::Vllm,
                variant: "official-openai-server".to_owned(),
                contract_revision: hal100_protocol::ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
            },
            instance_id: "acceptance:vllm".to_owned(),
            origin_fingerprint: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .to_owned(),
            config_revision: 1,
            protocol_capability_hash:
                "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789".to_owned(),
            platform: InferencePlatform::Linux,
            architecture: InferenceArchitecture::X86_64,
            accelerator: InferenceAccelerator::Cuda,
            deployment: InferenceDeployment::Local,
            outcome: InferenceEngineAcceptanceRunOutcome::Passed,
            observed_at_ms: 1,
            engine_version: Some("0.10.2".to_owned()),
            deployment_fingerprint: None,
            model_revision: Some("model-id-sha256:0123456789abcdef".to_owned()),
            host_summary: Some("linux/x86_64/cuda".to_owned()),
            host_attestation: native_host_attestation(
                InferencePlatform::Linux,
                InferenceArchitecture::X86_64,
                InferenceAccelerator::Cuda,
            ),
            model_evidence: test_model_evidence(),
            stability: None,
            resilience: None,
            evidence: [
                InferenceEngineSupportEvidenceKind::OfficialContract,
                InferenceEngineSupportEvidenceKind::ProtocolQualification,
                InferenceEngineSupportEvidenceKind::PlatformRuntime,
                InferenceEngineSupportEvidenceKind::EngineIdentity,
                InferenceEngineSupportEvidenceKind::ModelDeploymentIdentity,
                InferenceEngineSupportEvidenceKind::RuntimeProfileLifecycle,
            ]
            .into_iter()
            .map(|kind| InferenceEngineAcceptanceEvidence {
                kind,
                source: "crates/hal100-infra/tests/vllm_live_acceptance.rs".to_owned(),
                assertion: "explicit live acceptance assertion".to_owned(),
            })
            .collect(),
        }
    }

    #[test]
    fn checked_in_ledger_is_versioned_and_contains_only_reviewed_standard_claims() {
        let ledger = InferenceEngineAcceptanceLedger::standard().expect("standard evidence ledger");
        assert_eq!(
            ledger.schema_version,
            INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION
        );
        ledger.validate().expect("valid checked-in ledger");
        assert_eq!(ledger.records.len(), 3);
        assert!(
            ledger
                .records
                .iter()
                .all(|record| record.stability.is_none())
        );
        assert!(ledger.records.iter().all(|record| matches!(
            record.status,
            InferenceEngineSupportStatus::Managed | InferenceEngineSupportStatus::VerifiedExternal
        )));
        crate::ExternalInferenceEngineRegistry::standard_with_reviewed_acceptance_ledger(&ledger)
            .expect("every checked-in record maps to a standard support cell and protocol hash");
    }

    #[test]
    fn native_host_attestation_is_stable_private_and_device_class_bound() {
        let mut first = host_snapshot(
            InferencePlatform::Linux,
            InferenceArchitecture::X86_64,
            InferenceAccelerator::Cuda,
        );
        first.accelerators.reverse();
        first.model_storage_path = "/private/first/model/path".to_owned();
        first.model_storage_available_bytes = 1;
        let first_attestation = InferenceEngineAcceptanceHostAttestation::from_host_snapshot(
            &first,
            InferenceAccelerator::Cuda,
        )
        .expect("first native attestation");

        let mut same_device_class = first.clone();
        same_device_class.accelerators.reverse();
        same_device_class.model_storage_path = "/different/private/path".to_owned();
        same_device_class.model_storage_available_bytes = u64::MAX;
        let same_attestation = InferenceEngineAcceptanceHostAttestation::from_host_snapshot(
            &same_device_class,
            InferenceAccelerator::Cuda,
        )
        .expect("same device class");
        assert_eq!(first_attestation, same_attestation);

        let serialized = serde_json::to_string(&first_attestation).expect("serialize attestation");
        assert!(!serialized.contains("private"));
        assert!(!serialized.contains(&first.cpu_brand));
        assert!(!serialized.contains(&first.device_model));
        assert_eq!(
            first_attestation.probe_revision.as_deref(),
            Some(NATIVE_HOST_PROBE_REVISION)
        );

        let mut different_device_class = same_device_class;
        different_device_class.device_model = "DifferentAcceptanceHost".to_owned();
        let different_attestation = InferenceEngineAcceptanceHostAttestation::from_host_snapshot(
            &different_device_class,
            InferenceAccelerator::Cuda,
        )
        .expect("different device class");
        assert_ne!(
            first_attestation.device_class_fingerprint,
            different_attestation.device_class_fingerprint
        );
    }

    #[test]
    fn native_host_attestation_fails_closed_on_weak_or_ambiguous_probe_facts() {
        let base = host_snapshot(
            InferencePlatform::Linux,
            InferenceArchitecture::X86_64,
            InferenceAccelerator::Cuda,
        );
        for invalid in [
            HostCapabilitySnapshot {
                probe_revision: "host-capabilities-v2".to_owned(),
                ..base.clone()
            },
            HostCapabilitySnapshot {
                device_model: "Linux host".to_owned(),
                ..base.clone()
            },
            HostCapabilitySnapshot {
                accelerators: vec![InferenceAccelerator::Cpu],
                ..base.clone()
            },
            HostCapabilitySnapshot {
                accelerators: vec![
                    InferenceAccelerator::Cpu,
                    InferenceAccelerator::Cuda,
                    InferenceAccelerator::Cuda,
                ],
                ..base.clone()
            },
            HostCapabilitySnapshot {
                total_memory_bytes: 0,
                ..base.clone()
            },
        ] {
            assert_eq!(
                InferenceEngineAcceptanceHostAttestation::from_host_snapshot(
                    &invalid,
                    InferenceAccelerator::Cuda,
                )
                .err(),
                Some(InferenceEngineAcceptanceEvidenceError::InvalidHostAttestation)
            );
        }
    }

    #[test]
    fn v1_through_v3_ledgers_remain_readable_but_v4_mutations_require_current_evidence() {
        let legacy = InferenceEngineAcceptanceLedger::parse(include_bytes!(
            "../../../contracts/inference-engines/v1-acceptance-evidence.json"
        ))
        .expect("legacy v1 ledger remains readable");
        assert_eq!(
            legacy.schema_version,
            LEGACY_INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION
        );
        assert!(
            legacy
                .records
                .iter()
                .all(|record| record.host_attestation.is_none())
        );

        let typed_host = InferenceEngineAcceptanceLedger::parse(include_bytes!(
            "../../../contracts/inference-engines/v2-acceptance-evidence.json"
        ))
        .expect("v2 typed-host ledger remains readable");
        assert_eq!(
            typed_host.schema_version,
            TYPED_HOST_INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION
        );
        assert!(
            typed_host
                .records
                .iter()
                .all(|record| record.stability.is_none())
        );

        let performance = InferenceEngineAcceptanceLedger::parse(include_bytes!(
            "../../../contracts/inference-engines/v3-acceptance-evidence.json"
        ))
        .expect("v3 performance ledger remains readable");
        assert_eq!(
            performance.schema_version,
            PERFORMANCE_INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION
        );
        assert!(
            performance
                .records
                .iter()
                .all(|record| record.model_evidence.is_none())
        );

        let standard = InferenceEngineAcceptanceLedger::standard().expect("v4 standard ledger");
        assert_eq!(
            standard.schema_version,
            INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION
        );
        assert!(standard.records.iter().all(|record| {
            record.host_attestation.as_ref().is_some_and(|attestation| {
                attestation.kind
                    == InferenceEngineAcceptanceHostAttestationKind::LegacyHostSummaryV1
            })
        }));
        let mut candidate = standard;
        let mut replayed_legacy = candidate.records[0].clone();
        replayed_legacy.id = "replayed-legacy-record".to_owned();
        replayed_legacy.platform = InferencePlatform::Windows;
        replayed_legacy.host_attestation = Some(InferenceEngineAcceptanceHostAttestation {
            kind: InferenceEngineAcceptanceHostAttestationKind::LegacyHostSummaryV1,
            platform: replayed_legacy.platform,
            architecture: replayed_legacy.architecture,
            accelerator: replayed_legacy.accelerator,
            probe_revision: None,
            device_class_fingerprint: None,
        });
        let mut forged_ledger = candidate.clone();
        forged_ledger.records.push(replayed_legacy.clone());
        assert_eq!(
            forged_ledger.validate().err(),
            Some(InferenceEngineAcceptanceEvidenceError::InvalidHostAttestation)
        );
        assert_eq!(
            candidate.append_reviewed_record(replayed_legacy).err(),
            Some(InferenceEngineAcceptanceEvidenceError::InvalidHostAttestation)
        );

        let mut tampered_legacy = candidate;
        tampered_legacy.records[0].verified_at_ms += 1;
        assert_eq!(
            tampered_legacy.validate().err(),
            Some(InferenceEngineAcceptanceEvidenceError::InvalidHostAttestation)
        );
    }

    #[test]
    fn typed_model_evidence_is_deterministic_redacted_and_kind_bound() {
        let raw_value = "private/catalog/model-name";
        let evidence = RuntimeProfileEvidence {
            kind: RuntimeProfileEvidenceKind::CatalogIdentity,
            algorithm: "catalog-id-v1".to_owned(),
            value: raw_value.to_owned(),
        };
        let first = InferenceEngineAcceptanceModelEvidence::from_runtime_evidence(&evidence)
            .expect("bounded model evidence");
        let second = InferenceEngineAcceptanceModelEvidence::from_runtime_evidence(&evidence)
            .expect("same bounded model evidence");
        assert_eq!(first, second);
        assert!(first.matches_runtime_evidence(&evidence));

        let different_kind = RuntimeProfileEvidence {
            kind: RuntimeProfileEvidenceKind::RepositoryRevision,
            ..evidence.clone()
        };
        assert_ne!(
            first,
            InferenceEngineAcceptanceModelEvidence::from_runtime_evidence(&different_kind)
                .expect("different typed evidence")
        );
        let serialized = serde_json::to_string(&first).expect("serialize redacted evidence");
        assert!(!serialized.contains(raw_value));
        assert_eq!(first.value_fingerprint.len(), 64);
    }

    #[test]
    fn run_rejects_host_attestation_replayed_across_support_cells() {
        let mut run = run_artifact();
        run.accelerator = InferenceAccelerator::Cpu;
        assert_eq!(
            run.validate().err(),
            Some(InferenceEngineAcceptanceEvidenceError::InvalidHostAttestation)
        );

        let mut run = run_artifact();
        run.host_summary = Some("Linux GPU host owned by operator".to_owned());
        assert_eq!(
            run.validate().err(),
            Some(InferenceEngineAcceptanceEvidenceError::InvalidHostAttestation)
        );
    }

    #[test]
    fn live_run_binds_a_validated_instance_origin_and_configuration_revision() {
        let mut run = run_artifact();
        run.validate().expect("target-bound run artifact");

        run.origin_fingerprint = "not-a-fingerprint".to_owned();
        assert_eq!(
            run.validate().err(),
            Some(InferenceEngineAcceptanceEvidenceError::InvalidRun)
        );

        let mut run = run_artifact();
        run.instance_id = "acceptance/service".to_owned();
        assert_eq!(
            run.validate().err(),
            Some(InferenceEngineAcceptanceEvidenceError::InvalidRun)
        );

        let mut run = run_artifact();
        run.config_revision = 0;
        assert_eq!(
            run.validate().err(),
            Some(InferenceEngineAcceptanceEvidenceError::InvalidRun)
        );

        let mut run = run_artifact();
        run.config_revision = i64::MAX as u64 + 1;
        assert_eq!(
            run.validate().err(),
            Some(InferenceEngineAcceptanceEvidenceError::InvalidRun)
        );

        let mut run = run_artifact();
        run.protocol_capability_hash = "protocol-capability-text".to_owned();
        assert_eq!(
            run.validate().err(),
            Some(InferenceEngineAcceptanceEvidenceError::InvalidRun)
        );
    }

    #[test]
    fn live_run_artifact_is_partial_and_never_implies_formal_promotion() {
        let run = run_artifact();
        run.validate().expect("bounded live run artifact");
        let json = run.to_json().expect("serialize live run artifact");
        let decoded: InferenceEngineAcceptanceRun =
            serde_json::from_slice(&json).expect("decode live run artifact");
        assert_eq!(decoded, run);
        assert!(
            !run.evidence
                .iter()
                .any(|item| item.kind == InferenceEngineSupportEvidenceKind::Stability)
        );
        assert!(run.resilience.is_none());
        assert_eq!(
            run.into_formal_record(InferenceEngineSupportStatus::VerifiedExternal)
                .err(),
            Some(InferenceEngineAcceptanceEvidenceError::InvalidRecord)
        );
    }

    #[test]
    fn formal_record_requires_all_control_plane_resilience_checks() {
        let mut run = run_artifact();
        run.evidence.push(InferenceEngineAcceptanceEvidence {
            kind: InferenceEngineSupportEvidenceKind::Stability,
            source: "crates/hal100-infra/tests/vllm_live_acceptance.rs".to_owned(),
            assertion: "bounded repeated/concurrent probe passed".to_owned(),
        });
        run.stability = Some(measured_stability(250));
        run.model_revision = Some("vllm/model-revision-20260828".to_owned());

        for resilience in [
            None,
            Some(InferenceEngineAcceptanceResilience {
                cancellation_verified: false,
                failed_switch_rollback_verified: true,
                restart_compensation_verified: true,
            }),
            Some(InferenceEngineAcceptanceResilience {
                cancellation_verified: true,
                failed_switch_rollback_verified: false,
                restart_compensation_verified: true,
            }),
            Some(InferenceEngineAcceptanceResilience {
                cancellation_verified: true,
                failed_switch_rollback_verified: true,
                restart_compensation_verified: false,
            }),
        ] {
            let mut candidate = run.clone();
            candidate.resilience = resilience;
            assert_eq!(
                candidate
                    .into_formal_record(InferenceEngineSupportStatus::VerifiedExternal)
                    .err(),
                Some(InferenceEngineAcceptanceEvidenceError::InvalidRecord)
            );
        }

        run.resilience = Some(InferenceEngineAcceptanceResilience::complete());
        run.into_formal_record(InferenceEngineSupportStatus::VerifiedExternal)
            .expect("complete control-plane resilience evidence");
    }

    #[test]
    fn stability_evidence_requires_structured_bounded_measurements() {
        let mut run = run_artifact();
        run.evidence.push(InferenceEngineAcceptanceEvidence {
            kind: InferenceEngineSupportEvidenceKind::Stability,
            source: "crates/hal100-infra/tests/vllm_live_acceptance.rs".to_owned(),
            assertion: "bounded repeated/concurrent probe passed".to_owned(),
        });
        assert_eq!(
            run.validate().err(),
            Some(InferenceEngineAcceptanceEvidenceError::InvalidRun)
        );
        run.stability = Some(measured_stability(300));
        run.validate().expect("structured stability measurements");
        run.stability = Some(InferenceEngineAcceptanceStability {
            attempts: 101,
            ..measured_stability(300)
        });
        assert_eq!(
            run.validate().err(),
            Some(InferenceEngineAcceptanceEvidenceError::InvalidRun)
        );
    }

    #[test]
    fn reviewed_formal_record_requires_explicit_stability_item() {
        let mut run = run_artifact();
        run.evidence.push(InferenceEngineAcceptanceEvidence {
            kind: InferenceEngineSupportEvidenceKind::Stability,
            source: "crates/hal100-infra/tests/vllm_live_acceptance.rs".to_owned(),
            assertion: "fixed concurrency and recovery stability profile passed".to_owned(),
        });
        run.stability = Some(measured_stability(250));
        run.resilience = Some(InferenceEngineAcceptanceResilience::complete());
        let generated_model_revision = run.model_revision.clone();
        assert_eq!(
            run.clone()
                .into_formal_record(InferenceEngineSupportStatus::VerifiedExternal)
                .err(),
            Some(InferenceEngineAcceptanceEvidenceError::InvalidRecord)
        );
        assert_eq!(
            generated_model_revision.as_deref(),
            Some("model-id-sha256:0123456789abcdef")
        );
        run.model_revision = Some("vllm/model-revision-20260828".to_owned());
        let record = run
            .into_formal_record(InferenceEngineSupportStatus::VerifiedExternal)
            .expect("complete reviewed formal record");
        assert_eq!(
            record.status,
            InferenceEngineSupportStatus::VerifiedExternal
        );
        assert_eq!(record.evidence.len(), 7);
        assert_eq!(record.stability, Some(measured_stability(250)));
    }

    #[test]
    fn new_v4_formal_record_cannot_omit_the_measured_stability_profile() {
        let mut record = formal_record();
        record.id = "new-v4-formal-without-stability".to_owned();
        record.stability = None;
        let ledger = InferenceEngineAcceptanceLedger {
            schema_version: INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION,
            records: vec![record],
        };

        assert_eq!(
            ledger.validate().err(),
            Some(InferenceEngineAcceptanceEvidenceError::InvalidRecord)
        );
    }

    #[test]
    fn new_v4_formal_record_cannot_omit_typed_model_evidence() {
        let mut record = formal_record();
        record.id = "new-v4-formal-without-model-evidence".to_owned();
        record.model_evidence = None;
        let ledger = InferenceEngineAcceptanceLedger {
            schema_version: INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION,
            records: vec![record],
        };

        assert_eq!(
            ledger.validate().err(),
            Some(InferenceEngineAcceptanceEvidenceError::InvalidRecord)
        );
    }

    #[test]
    fn reviewed_import_helper_replaces_the_redacted_model_correlation() {
        let mut run = run_artifact();
        run.evidence.push(InferenceEngineAcceptanceEvidence {
            kind: InferenceEngineSupportEvidenceKind::Stability,
            source: "crates/hal100-infra/tests/vllm_live_acceptance.rs".to_owned(),
            assertion: "fixed concurrency and recovery stability profile passed".to_owned(),
        });
        run.stability = Some(measured_stability(250));
        run.resilience = Some(InferenceEngineAcceptanceResilience::complete());
        let record = run
            .into_formal_record_with_model_revision(
                InferenceEngineSupportStatus::VerifiedExternal,
                "Qwen3-8B@revision-20260828",
            )
            .expect("reviewed model revision");
        assert_eq!(
            record.model_revision.as_deref(),
            Some("Qwen3-8B@revision-20260828")
        );
    }

    #[test]
    fn reviewed_import_helper_rejects_redacted_or_empty_model_revisions() {
        let mut run = run_artifact();
        run.evidence.push(InferenceEngineAcceptanceEvidence {
            kind: InferenceEngineSupportEvidenceKind::Stability,
            source: "crates/hal100-infra/tests/vllm_live_acceptance.rs".to_owned(),
            assertion: "fixed concurrency and recovery stability profile passed".to_owned(),
        });
        run.stability = Some(measured_stability(250));
        for revision in ["", "model-id-sha256:0123456789abcdef"] {
            assert_eq!(
                run.clone()
                    .into_formal_record_with_model_revision(
                        InferenceEngineSupportStatus::VerifiedExternal,
                        revision,
                    )
                    .err(),
                Some(InferenceEngineAcceptanceEvidenceError::InvalidRecord)
            );
        }
    }

    #[test]
    fn formal_record_accepts_deployment_fingerprint_without_package_version() {
        let mut run = run_artifact();
        run.engine_version = None;
        run.deployment_fingerprint =
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned());
        run.evidence.push(InferenceEngineAcceptanceEvidence {
            kind: InferenceEngineSupportEvidenceKind::Stability,
            source: "crates/hal100-infra/tests/mlc_llm_live_acceptance.rs".to_owned(),
            assertion: "bounded repeated/concurrent probe passed".to_owned(),
        });
        run.stability = Some(measured_stability(300));
        run.resilience = Some(InferenceEngineAcceptanceResilience::complete());
        run.model_revision = Some("mlc/deployment-revision-20260828".to_owned());
        let fingerprint = run.deployment_fingerprint.clone();
        let record = run
            .into_formal_record(InferenceEngineSupportStatus::VerifiedExternal)
            .expect("deployment fingerprint can establish engine identity");
        assert_eq!(record.engine_version, None);
        assert_eq!(record.deployment_fingerprint, fingerprint);
        assert_eq!(record.evidence.len(), 7);
    }

    #[test]
    fn formal_run_import_is_atomic_and_rejects_duplicate_support_cells() {
        let mut run = run_artifact();
        run.evidence.push(InferenceEngineAcceptanceEvidence {
            kind: InferenceEngineSupportEvidenceKind::Stability,
            source: "crates/hal100-infra/tests/vllm_live_acceptance.rs".to_owned(),
            assertion: "fixed concurrency and recovery stability profile passed".to_owned(),
        });
        run.stability = Some(measured_stability(250));
        run.resilience = Some(InferenceEngineAcceptanceResilience::complete());
        run.model_revision = Some("vllm/model-revision-20260828".to_owned());
        let mut ledger = InferenceEngineAcceptanceLedger {
            schema_version: INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION,
            records: Vec::new(),
        };
        ledger
            .append_run_as_formal(run.clone(), InferenceEngineSupportStatus::VerifiedExternal)
            .expect("import reviewed formal run");
        assert_eq!(ledger.records.len(), 1);
        let mut duplicate_cell = run;
        duplicate_cell.run_id = "run-20260828-vllm-duplicate".to_owned();
        assert_eq!(
            ledger
                .append_run_as_formal(
                    duplicate_cell,
                    InferenceEngineSupportStatus::VerifiedExternal,
                )
                .err(),
            Some(InferenceEngineAcceptanceEvidenceError::DuplicateSupportCell)
        );
        assert_eq!(ledger.records.len(), 1);
    }

    #[test]
    fn reviewed_requalification_replaces_only_the_named_exact_support_cell() {
        let mut original = formal_record();
        original.id = "old-mlx-metal-run".to_owned();
        let mut ledger = InferenceEngineAcceptanceLedger {
            schema_version: INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION,
            records: vec![original.clone()],
        };
        let mut replacement = original.clone();
        replacement.id = "new-mlx-metal-run".to_owned();
        replacement.verified_at_ms = 2;
        replacement.deployment_fingerprint =
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned());
        ledger
            .replace_reviewed_record(&original.id, replacement.clone())
            .expect("same-cell requalification");
        assert_eq!(ledger.records, vec![replacement]);

        let unchanged = ledger.clone();
        let mut wrong_cell = original;
        wrong_cell.id = "wrong-cell-run".to_owned();
        wrong_cell.accelerator = InferenceAccelerator::Cpu;
        wrong_cell.host_summary = Some(canonical_host_summary(
            wrong_cell.platform,
            wrong_cell.architecture,
            wrong_cell.accelerator,
        ));
        wrong_cell.host_attestation = Some(native_host_attestation(
            wrong_cell.platform,
            wrong_cell.architecture,
            wrong_cell.accelerator,
        ));
        assert_eq!(
            ledger
                .replace_reviewed_record("new-mlx-metal-run", wrong_cell)
                .err(),
            Some(InferenceEngineAcceptanceEvidenceError::RecordMismatch)
        );
        assert_eq!(ledger, unchanged);
    }

    #[test]
    fn incomplete_run_import_does_not_mutate_the_ledger() {
        let mut ledger = InferenceEngineAcceptanceLedger {
            schema_version: INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION,
            records: Vec::new(),
        };
        assert_eq!(
            ledger
                .append_run_as_formal(
                    run_artifact(),
                    InferenceEngineSupportStatus::VerifiedExternal,
                )
                .err(),
            Some(InferenceEngineAcceptanceEvidenceError::InvalidRecord)
        );
        assert!(ledger.records.is_empty());
    }

    #[test]
    fn live_run_artifact_rejects_untrusted_sources_and_empty_passes() {
        let mut run = run_artifact();
        run.evidence[0].source = "https://example.invalid/run".to_owned();
        assert_eq!(
            run.validate().err(),
            Some(InferenceEngineAcceptanceEvidenceError::InvalidRun)
        );

        let mut run = run_artifact();
        run.evidence.clear();
        assert_eq!(
            run.validate().err(),
            Some(InferenceEngineAcceptanceEvidenceError::InvalidRun)
        );
    }

    #[test]
    fn evidence_sources_are_repository_relative_and_path_traversal_safe() {
        for source in [
            "../acceptance.json",
            "crates/hal100-infra/tests/../../secrets.txt",
            "./docs/ITERATION_60_CHECKPOINT.md",
            "docs//ITERATION_60_CHECKPOINT.md",
            "https://example.invalid/evidence",
            "/tmp/evidence.json",
            "notes/evidence.txt",
            "README.md.bak",
        ] {
            let mut run = run_artifact();
            run.evidence[0].source = source.to_owned();
            assert_eq!(
                run.validate().err(),
                Some(InferenceEngineAcceptanceEvidenceError::InvalidRun),
                "source must be rejected: {source}"
            );
        }

        let mut run = run_artifact();
        run.evidence[0].source = "README.md".to_owned();
        run.validate().expect("repository-root source is valid");
    }

    #[test]
    fn live_run_output_is_create_new_and_never_overwrites() {
        let path = std::env::temp_dir().join(format!(
            "hal100-acceptance-run-test-{}.json",
            uuid::Uuid::new_v4().simple()
        ));
        let run = run_artifact();
        write_acceptance_run_exclusive(&path, &run).expect("create explicit output");
        assert!(path.is_file());
        assert_eq!(
            write_acceptance_run_exclusive(&path, &run).err(),
            Some(InferenceEngineAcceptanceEvidenceError::RunOutputUnavailable)
        );
        std::fs::remove_file(path).expect("remove isolated test output");
    }

    #[test]
    fn formal_record_requires_all_seven_bounded_evidence_items() {
        let mut ledger = InferenceEngineAcceptanceLedger {
            schema_version: INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION,
            records: vec![formal_record()],
        };
        ledger.validate().expect("complete formal record");
        ledger.records[0].evidence.pop();
        assert_eq!(
            ledger.validate().err(),
            Some(InferenceEngineAcceptanceEvidenceError::InvalidRecord)
        );
    }

    #[test]
    fn formal_manifest_without_real_record_cannot_pass_opt_in_promotion_gate() {
        let manifest = InferenceEngineManifest {
            adapter_id: formal_record().adapter_id.clone(),
            descriptor: InferenceEngineDescriptor {
                kind: InferenceEngineKind::MlxLm,
                display_name: "MLX-LM".to_owned(),
                ownership: InferenceEngineOwnership::External,
                deployment: InferenceDeployment::Local,
                protocols: vec![InferenceProtocol::OpenAi],
                platforms: vec![InferencePlatform::MacOs],
                architectures: vec![InferenceArchitecture::Aarch64],
                accelerators: vec![InferenceAccelerator::Metal],
                model_formats: vec![InferenceModelFormat::Mlx],
                managed_lifecycle: false,
            },
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
        let empty = InferenceEngineAcceptanceLedger {
            schema_version: INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION,
            records: Vec::new(),
        };
        assert_eq!(
            empty.validate_manifest(&manifest).err(),
            Some(InferenceEngineAcceptanceEvidenceError::MissingFormalRecord)
        );
        let ledger = InferenceEngineAcceptanceLedger {
            schema_version: INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION,
            records: vec![formal_record()],
        };
        ledger
            .validate_manifest(&manifest)
            .expect("matching record");
    }

    #[test]
    fn evidence_sources_cannot_smuggle_urls_or_absolute_paths() {
        let mut record = formal_record();
        record.evidence[0].source = "https://example.invalid/evidence".to_owned();
        let ledger = InferenceEngineAcceptanceLedger {
            schema_version: INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION,
            records: vec![record],
        };
        assert_eq!(
            ledger.validate().err(),
            Some(InferenceEngineAcceptanceEvidenceError::InvalidRecord)
        );
    }

    #[test]
    fn duplicate_support_cells_are_rejected_even_when_record_ids_differ() {
        let first = formal_record();
        let mut second = first.clone();
        second.id = "mlx-lm-macos-aarch64-metal-other".to_owned();
        let ledger = InferenceEngineAcceptanceLedger {
            schema_version: INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION,
            records: vec![first, second],
        };
        assert_eq!(
            ledger.validate().err(),
            Some(InferenceEngineAcceptanceEvidenceError::DuplicateSupportCell)
        );
    }

    #[test]
    fn stale_record_for_the_same_adapter_is_rejected_by_manifest_gate() {
        let mut record = formal_record();
        record.platform = InferencePlatform::Windows;
        record.host_summary = Some(canonical_host_summary(
            record.platform,
            record.architecture,
            record.accelerator,
        ));
        record.host_attestation = Some(native_host_attestation(
            record.platform,
            record.architecture,
            record.accelerator,
        ));
        let ledger = InferenceEngineAcceptanceLedger {
            schema_version: INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION,
            records: vec![record],
        };
        let manifest = InferenceEngineManifest {
            adapter_id: formal_record().adapter_id.clone(),
            descriptor: InferenceEngineDescriptor {
                kind: InferenceEngineKind::MlxLm,
                display_name: "MLX-LM".to_owned(),
                ownership: InferenceEngineOwnership::External,
                deployment: InferenceDeployment::Local,
                protocols: vec![InferenceProtocol::OpenAi],
                platforms: vec![InferencePlatform::MacOs],
                architectures: vec![InferenceArchitecture::Aarch64],
                accelerators: vec![InferenceAccelerator::Metal],
                model_formats: vec![InferenceModelFormat::Mlx],
                managed_lifecycle: false,
            },
            support_units: vec![InferenceEngineSupportUnit {
                platform: InferencePlatform::MacOs,
                architecture: InferenceArchitecture::Aarch64,
                accelerator: InferenceAccelerator::Metal,
                deployment: InferenceDeployment::Local,
                status: InferenceEngineSupportStatus::Connected,
                evidence: None,
            }],
        };
        assert_eq!(
            ledger.validate_manifest(&manifest).err(),
            Some(InferenceEngineAcceptanceEvidenceError::RecordMismatch)
        );
    }
}
