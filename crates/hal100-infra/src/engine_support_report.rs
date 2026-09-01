use std::collections::HashMap;

use hal100_protocol::{
    EngineAdapterId, InferenceAccelerator, InferenceArchitecture, InferenceDeployment,
    InferenceEngineManifest, InferenceEngineOwnership, InferenceEngineSupportEvidenceSummary,
    InferenceEngineSupportStatus, InferencePlatform,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    InferenceEngineAcceptanceHostAttestation, InferenceEngineAcceptanceHostAttestationKind,
    InferenceEngineAcceptanceLedger, InferenceEngineAcceptanceModelEvidence,
    InferenceEngineAcceptanceRecord, InferenceEngineAcceptanceStability,
    InferenceEngineManifestRegistry,
};

/// Versioned, read-only coverage report for the reviewed inference-engine support matrix.
///
/// The report intentionally contains only typed support-cell coordinates, statuses, aggregate
/// evidence/ledger counts and reviewed workload-bound stability measurements. It never includes
/// endpoints, model identifiers, paths, commands, credentials or raw runtime observations. It is
/// suitable for operator preflight and CI gates before a reviewed ledger is copied into the
/// repository.
pub const INFERENCE_ENGINE_SUPPORT_REPORT_SCHEMA_VERSION: u16 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InferenceEngineSupportCoverageReport {
    pub schema_version: u16,
    pub adapters: Vec<InferenceEngineAdapterSupportCoverage>,
    pub total_support_cells: u32,
    pub formal_support_cells: u32,
    pub pending_support_cells: u32,
    pub ledger_records: u32,
    pub formal_cells_missing_ledger: u32,
    /// Reviewed records with one atomic instance/model/native-device/performance scope.
    pub reviewed_performance_profiles: u32,
    /// Formal external support cells that still lack a complete scoped performance profile.
    /// Historical formal records remain valid, but are counted here instead of receiving inferred
    /// or fabricated latency/throughput values or a model/device association.
    pub formal_external_cells_missing_performance_profile: u32,
    /// True when every declared support cell is formal enough for activation.
    pub all_cells_formal: bool,
    /// True when every base-manifest formal external cell has a matching reviewed ledger record.
    /// Managed cells are proven by the owned manifest/lifecycle contract instead.
    pub all_formal_cells_ledger_backed: bool,
    /// Mirrors the strict promotion gate: all cells must be formal and every formal external base
    /// cell must have a reviewed record. This is an operator signal, not activation authority.
    pub ready_for_strict_promotion: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InferenceEngineAdapterSupportCoverage {
    pub adapter_id: EngineAdapterId,
    pub display_name: String,
    pub ownership: InferenceEngineOwnership,
    pub cells: Vec<InferenceEngineSupportCellCoverage>,
    pub total_support_cells: u32,
    pub formal_support_cells: u32,
    pub pending_support_cells: u32,
    pub ledger_backed_cells: u32,
    pub ready_for_strict_promotion: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InferenceEngineSupportCellCoverage {
    pub platform: InferencePlatform,
    pub architecture: InferenceArchitecture,
    pub accelerator: InferenceAccelerator,
    pub deployment: InferenceDeployment,
    pub manifest_status: InferenceEngineSupportStatus,
    /// Effective status after considering a matching reviewed formal ledger record. A weak
    /// `reserved`/`connected` ledger record never promotes a cell.
    pub effective_status: InferenceEngineSupportStatus,
    pub ledger_record_present: bool,
    pub ledger_record_status: Option<InferenceEngineSupportStatus>,
    /// Atomic scope copied from one exact reviewed ledger record. Measurements are never exposed
    /// without their instance/config, engine, typed model and native-device bindings.
    /// `None` means unknown and must never be interpreted as zero latency or zero throughput.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_performance_profile: Option<InferenceEngineReviewedPerformanceProfile>,
    pub evidence: InferenceEngineSupportEvidenceSummary,
    pub promotion_ready: bool,
}

/// Complete, redacted scope for one reviewed performance measurement.
///
/// Adapter and support-cell coordinates are inherited from the containing report cell. The
/// remaining fields make it impossible for an operator consumer to detach numeric measurements
/// from the exact service configuration, engine identity, model evidence or native device class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InferenceEngineReviewedPerformanceProfile {
    pub origin_fingerprint: String,
    pub config_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment_fingerprint: Option<String>,
    pub model_evidence: InferenceEngineAcceptanceModelEvidence,
    pub host_attestation: InferenceEngineAcceptanceHostAttestation,
    pub stability: InferenceEngineAcceptanceStability,
    pub reviewed_at_ms: i64,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum InferenceEngineSupportReportError {
    #[error("推理引擎验收账本无效")]
    InvalidLedger,
    #[error("推理引擎验收账本包含未声明的适配器或支持格")]
    UnmappedLedgerRecord,
    #[error("推理引擎验收账本记录状态与适配器所有权不一致")]
    RecordStatusMismatch,
    #[error("外部推理引擎验收账本缺少协议能力哈希基线")]
    MissingProtocolCapabilityHash,
    #[error("推理引擎验收账本协议能力哈希与适配器合同不一致")]
    ProtocolCapabilityHashMismatch,
}

/// Build a deterministic coverage report from a manifest registry and reviewed ledger.
///
/// The registry is treated as the checked-in/base declaration. A matching formal ledger record
/// projects the cell's effective status for reporting only; this function does not mutate the
/// registry and does not grant execution authority.
pub fn build_support_coverage_report(
    registry: &InferenceEngineManifestRegistry,
    ledger: &InferenceEngineAcceptanceLedger,
) -> Result<InferenceEngineSupportCoverageReport, InferenceEngineSupportReportError> {
    // A manifest-only caller has no executable adapter hashes. Keep empty-ledger and managed-only
    // reports useful, but fail closed as soon as an external record would otherwise be projected
    // without its canonical protocol contract check.
    let no_external_hashes = HashMap::new();
    build_support_coverage_report_inner(registry, ledger, Some(&no_external_hashes))
}

/// Build a coverage report while also checking the canonical protocol-capability hash for each
/// external adapter represented in the ledger.
///
/// A manifest deliberately contains no runtime adapter implementation, so callers that have the
/// executable adapter registry should pass its exact hash map here. The plain report builder is
/// retained for manifest-only tooling with no external records; strict CLI/report paths must use
/// this variant so a stale protocol contract cannot look promotion-ready.
pub fn build_support_coverage_report_with_protocol_capability_hashes(
    registry: &InferenceEngineManifestRegistry,
    ledger: &InferenceEngineAcceptanceLedger,
    expected_protocol_capability_hashes: &HashMap<EngineAdapterId, String>,
) -> Result<InferenceEngineSupportCoverageReport, InferenceEngineSupportReportError> {
    build_support_coverage_report_inner(registry, ledger, Some(expected_protocol_capability_hashes))
}

fn build_support_coverage_report_inner(
    registry: &InferenceEngineManifestRegistry,
    ledger: &InferenceEngineAcceptanceLedger,
    expected_protocol_capability_hashes: Option<&HashMap<EngineAdapterId, String>>,
) -> Result<InferenceEngineSupportCoverageReport, InferenceEngineSupportReportError> {
    ledger
        .validate()
        .map_err(|_| InferenceEngineSupportReportError::InvalidLedger)?;

    // Reject stale records before producing a report. Silently hiding one would make an operator
    // believe the matrix is complete while the production promotion constructor would fail.
    for record in &ledger.records {
        let Some(manifest) = registry.manifest(&record.adapter_id) else {
            return Err(InferenceEngineSupportReportError::UnmappedLedgerRecord);
        };
        let mapped = manifest.support_units.iter().find(|unit| {
            unit.platform == record.platform
                && unit.architecture == record.architecture
                && unit.accelerator == record.accelerator
                && unit.deployment == record.deployment
        });
        let Some(_unit) = mapped else {
            return Err(InferenceEngineSupportReportError::UnmappedLedgerRecord);
        };
        if is_formal(record.status)
            && !matches!(
                (manifest.descriptor.ownership, record.status),
                (
                    InferenceEngineOwnership::External,
                    InferenceEngineSupportStatus::VerifiedExternal
                ) | (
                    InferenceEngineOwnership::Managed,
                    InferenceEngineSupportStatus::Managed
                )
            )
        {
            return Err(InferenceEngineSupportReportError::RecordStatusMismatch);
        }
        if manifest.descriptor.ownership == InferenceEngineOwnership::External
            && let Some(expected_hashes) = expected_protocol_capability_hashes
        {
            let Some(expected_hash) = expected_hashes.get(&record.adapter_id) else {
                return Err(InferenceEngineSupportReportError::MissingProtocolCapabilityHash);
            };
            if record.protocol_capability_hash != *expected_hash {
                return Err(InferenceEngineSupportReportError::ProtocolCapabilityHashMismatch);
            }
        }
    }

    let mut adapters = registry
        .manifests()
        .into_iter()
        .map(|manifest| adapter_coverage(&manifest, ledger))
        .collect::<Vec<_>>();
    adapters.sort_by(|left, right| {
        left.adapter_id
            .engine
            .storage_key()
            .cmp(right.adapter_id.engine.storage_key())
            .then_with(|| left.adapter_id.variant.cmp(&right.adapter_id.variant))
            .then_with(|| {
                left.adapter_id
                    .contract_revision
                    .cmp(&right.adapter_id.contract_revision)
            })
    });

    let total_support_cells: u32 = adapters
        .iter()
        .map(|adapter| adapter.total_support_cells)
        .sum();
    let formal_support_cells: u32 = adapters
        .iter()
        .map(|adapter| adapter.formal_support_cells)
        .sum();
    let pending_support_cells = total_support_cells.saturating_sub(formal_support_cells);
    let formal_cells_missing_ledger: u32 = adapters
        .iter()
        .filter(|adapter| adapter.ownership == InferenceEngineOwnership::External)
        .flat_map(|adapter| adapter.cells.iter())
        .filter(|cell| is_formal(cell.manifest_status) && !cell.ledger_record_present)
        .count()
        .try_into()
        .unwrap_or(u32::MAX);
    let reviewed_performance_profiles = adapters
        .iter()
        .flat_map(|adapter| adapter.cells.iter())
        .filter(|cell| cell.reviewed_performance_profile.is_some())
        .count()
        .try_into()
        .unwrap_or(u32::MAX);
    let formal_external_cells_missing_performance_profile = adapters
        .iter()
        .filter(|adapter| adapter.ownership == InferenceEngineOwnership::External)
        .flat_map(|adapter| adapter.cells.iter())
        .filter(|cell| {
            is_formal(cell.effective_status) && cell.reviewed_performance_profile.is_none()
        })
        .count()
        .try_into()
        .unwrap_or(u32::MAX);
    let all_cells_formal = pending_support_cells == 0;
    let all_formal_cells_ledger_backed = formal_cells_missing_ledger == 0;

    Ok(InferenceEngineSupportCoverageReport {
        schema_version: INFERENCE_ENGINE_SUPPORT_REPORT_SCHEMA_VERSION,
        adapters,
        total_support_cells,
        formal_support_cells,
        pending_support_cells,
        ledger_records: ledger.records.len().try_into().unwrap_or(u32::MAX),
        formal_cells_missing_ledger,
        reviewed_performance_profiles,
        formal_external_cells_missing_performance_profile,
        all_cells_formal,
        all_formal_cells_ledger_backed,
        ready_for_strict_promotion: all_cells_formal && all_formal_cells_ledger_backed,
    })
}

fn adapter_coverage(
    manifest: &InferenceEngineManifest,
    ledger: &InferenceEngineAcceptanceLedger,
) -> InferenceEngineAdapterSupportCoverage {
    let mut cells = manifest
        .support_units
        .iter()
        .map(|unit| {
            let record = ledger.record_for(
                &manifest.adapter_id,
                unit.platform,
                unit.architecture,
                unit.accelerator,
                unit.deployment,
            );
            let ledger_record_status = record.map(|record| record.status);
            let effective_status = match ledger_record_status {
                Some(status) if is_formal(status) => status,
                _ => unit.status,
            };
            let evidence = if is_formal(effective_status) {
                InferenceEngineSupportEvidenceSummary::for_status(effective_status)
            } else {
                unit.evidence.clone().unwrap_or_else(|| {
                    InferenceEngineSupportEvidenceSummary::for_status(unit.status)
                })
            };
            InferenceEngineSupportCellCoverage {
                platform: unit.platform,
                architecture: unit.architecture,
                accelerator: unit.accelerator,
                deployment: unit.deployment,
                manifest_status: unit.status,
                effective_status,
                ledger_record_present: record.is_some(),
                ledger_record_status,
                reviewed_performance_profile: record.and_then(reviewed_performance_profile),
                evidence,
                promotion_ready: !is_formal(unit.status)
                    && ledger_record_status.is_some_and(is_formal),
            }
        })
        .collect::<Vec<_>>();
    cells.sort_by_key(|cell| {
        (
            cell.platform.storage_key(),
            cell.architecture.storage_key(),
            cell.accelerator.storage_key(),
            cell.deployment.storage_key(),
        )
    });
    let total_support_cells = cells.len().try_into().unwrap_or(u32::MAX);
    let formal_support_cells = cells
        .iter()
        .filter(|cell| is_formal(cell.effective_status))
        .count()
        .try_into()
        .unwrap_or(u32::MAX);
    let pending_support_cells = total_support_cells.saturating_sub(formal_support_cells);
    let ledger_backed_cells = cells
        .iter()
        .filter(|cell| cell.ledger_record_present)
        .count()
        .try_into()
        .unwrap_or(u32::MAX);
    let ready_for_strict_promotion = pending_support_cells == 0
        && (manifest.descriptor.ownership == InferenceEngineOwnership::Managed
            || cells
                .iter()
                .filter(|cell| is_formal(cell.manifest_status))
                .all(|cell| cell.ledger_record_present));
    InferenceEngineAdapterSupportCoverage {
        adapter_id: manifest.adapter_id.clone(),
        display_name: manifest.descriptor.display_name.clone(),
        ownership: manifest.descriptor.ownership,
        cells,
        total_support_cells,
        formal_support_cells,
        pending_support_cells,
        ledger_backed_cells,
        ready_for_strict_promotion,
    }
}

fn reviewed_performance_profile(
    record: &InferenceEngineAcceptanceRecord,
) -> Option<InferenceEngineReviewedPerformanceProfile> {
    if !is_formal(record.status) {
        return None;
    }
    let model_evidence = record.model_evidence.clone()?;
    let host_attestation = record.host_attestation.clone()?;
    if host_attestation.kind != InferenceEngineAcceptanceHostAttestationKind::NativeHostProbeV1 {
        return None;
    }
    Some(InferenceEngineReviewedPerformanceProfile {
        origin_fingerprint: record.origin_fingerprint.clone(),
        config_revision: record.config_revision,
        engine_version: record.engine_version.clone(),
        deployment_fingerprint: record.deployment_fingerprint.clone(),
        model_evidence,
        host_attestation,
        stability: record.stability.clone()?,
        reviewed_at_ms: record.verified_at_ms,
    })
}

const fn is_formal(status: InferenceEngineSupportStatus) -> bool {
    matches!(
        status,
        InferenceEngineSupportStatus::Managed | InferenceEngineSupportStatus::VerifiedExternal
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use hal100_protocol::{
        EngineAdapterId, InferenceEngineDescriptor, InferenceEngineKind,
        InferenceEngineSupportUnit, InferenceModelFormat, InferenceProtocol,
    };

    use super::*;
    use crate::{InferenceEngineAcceptanceRecord, llama_cpp_manifest};

    fn manifest() -> InferenceEngineManifest {
        InferenceEngineManifest {
            adapter_id: EngineAdapterId {
                engine: InferenceEngineKind::Vllm,
                variant: "official-openai-server".to_owned(),
                contract_revision: hal100_protocol::ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
            },
            descriptor: InferenceEngineDescriptor {
                kind: InferenceEngineKind::Vllm,
                display_name: "vLLM fixture".to_owned(),
                ownership: InferenceEngineOwnership::External,
                deployment: InferenceDeployment::Local,
                protocols: vec![InferenceProtocol::OpenAi],
                platforms: vec![InferencePlatform::Linux],
                architectures: vec![InferenceArchitecture::X86_64],
                accelerators: vec![InferenceAccelerator::Cuda],
                model_formats: vec![InferenceModelFormat::Safetensors],
                managed_lifecycle: false,
            },
            support_units: vec![InferenceEngineSupportUnit {
                platform: InferencePlatform::Linux,
                architecture: InferenceArchitecture::X86_64,
                accelerator: InferenceAccelerator::Cuda,
                deployment: InferenceDeployment::Local,
                status: InferenceEngineSupportStatus::Connected,
                evidence: Some(InferenceEngineSupportEvidenceSummary::for_status(
                    InferenceEngineSupportStatus::Connected,
                )),
            }],
        }
    }

    fn ledger() -> InferenceEngineAcceptanceLedger {
        InferenceEngineAcceptanceLedger {
            schema_version: crate::INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION,
            records: Vec::new(),
        }
    }

    fn reviewed_record() -> InferenceEngineAcceptanceRecord {
        let status = InferenceEngineSupportStatus::VerifiedExternal;
        InferenceEngineAcceptanceRecord {
            id: "reviewed-vllm".to_owned(),
            adapter_id: manifest().adapter_id,
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
            status,
            verified_at_ms: 1,
            engine_version: Some("0.10.0".to_owned()),
            deployment_fingerprint: None,
            model_revision: Some("model@immutable-revision".to_owned()),
            host_summary: Some("linux/x86_64/cuda".to_owned()),
            host_attestation: Some(
                crate::engine_acceptance_evidence::test_native_host_attestation(
                    InferencePlatform::Linux,
                    InferenceArchitecture::X86_64,
                    InferenceAccelerator::Cuda,
                ),
            ),
            model_evidence: Some(crate::engine_acceptance_evidence::test_model_evidence()),
            stability: Some(crate::engine_acceptance_evidence::test_stability_profile()),
            resilience: Some(crate::InferenceEngineAcceptanceResilience::complete()),
            evidence: InferenceEngineSupportEvidenceSummary::for_status(status)
                .verified
                .into_iter()
                .map(|kind| crate::InferenceEngineAcceptanceEvidence {
                    kind,
                    source: "docs/INFERENCE_ENGINE_SUPPORT_PLAN.md".to_owned(),
                    assertion: "reviewed acceptance evidence".to_owned(),
                })
                .collect(),
        }
    }

    #[test]
    fn empty_ledger_reports_pending_cell_without_claiming_promotion() {
        let registry = InferenceEngineManifestRegistry::new(vec![manifest()]).expect("manifest");
        let report = build_support_coverage_report(&registry, &ledger()).expect("report");
        assert_eq!(report.total_support_cells, 1);
        assert_eq!(report.formal_support_cells, 0);
        assert_eq!(report.pending_support_cells, 1);
        assert!(!report.all_cells_formal);
        assert!(!report.ready_for_strict_promotion);
        assert!(!report.adapters[0].cells[0].ledger_record_present);
        assert_eq!(report.reviewed_performance_profiles, 0);
        assert_eq!(report.formal_external_cells_missing_performance_profile, 0);
    }

    #[test]
    fn reviewed_formal_record_projects_exact_cell_and_makes_strict_gate_ready() {
        let registry = InferenceEngineManifestRegistry::new(vec![manifest()]).expect("manifest");
        let mut ledger = ledger();
        let record = reviewed_record();
        let expected_hashes = HashMap::from([(
            record.adapter_id.clone(),
            record.protocol_capability_hash.clone(),
        )]);
        ledger.records.push(record);
        let report = build_support_coverage_report_with_protocol_capability_hashes(
            &registry,
            &ledger,
            &expected_hashes,
        )
        .expect("report");
        let cell = &report.adapters[0].cells[0];
        assert_eq!(
            cell.manifest_status,
            InferenceEngineSupportStatus::Connected
        );
        assert_eq!(
            cell.effective_status,
            InferenceEngineSupportStatus::VerifiedExternal
        );
        assert!(cell.ledger_record_present);
        let performance = cell
            .reviewed_performance_profile
            .as_ref()
            .expect("atomic reviewed performance scope");
        assert_eq!(
            performance.stability,
            crate::engine_acceptance_evidence::test_stability_profile()
        );
        assert_eq!(performance.config_revision, 1);
        assert_eq!(performance.engine_version.as_deref(), Some("0.10.0"));
        assert_eq!(
            performance.model_evidence,
            crate::engine_acceptance_evidence::test_model_evidence()
        );
        assert_eq!(
            performance.host_attestation.kind,
            InferenceEngineAcceptanceHostAttestationKind::NativeHostProbeV1
        );
        assert!(cell.promotion_ready);
        assert_eq!(report.reviewed_performance_profiles, 1);
        assert_eq!(report.formal_external_cells_missing_performance_profile, 0);
        assert!(report.all_cells_formal);
        assert!(report.all_formal_cells_ledger_backed);
        assert!(report.ready_for_strict_promotion);
        let rendered = serde_json::to_string(&report).expect("serialize scoped report");
        assert!(rendered.contains("reviewedPerformanceProfile"));
        assert!(!rendered.contains("acceptance:vllm"));
        assert!(!rendered.contains("model@immutable-revision"));
    }

    #[test]
    fn stale_ledger_record_is_rejected_instead_of_hidden() {
        let registry = InferenceEngineManifestRegistry::new(vec![manifest()]).expect("manifest");
        let mut ledger = ledger();
        ledger.records.push(InferenceEngineAcceptanceRecord {
            id: "stale".to_owned(),
            adapter_id: EngineAdapterId {
                engine: InferenceEngineKind::Ollama,
                variant: "official-loopback-api".to_owned(),
                contract_revision: hal100_protocol::ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
            },
            instance_id: "acceptance:stale".to_owned(),
            origin_fingerprint: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .to_owned(),
            config_revision: 1,
            protocol_capability_hash:
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned(),
            platform: InferencePlatform::Linux,
            architecture: InferenceArchitecture::X86_64,
            accelerator: InferenceAccelerator::Cuda,
            deployment: InferenceDeployment::Local,
            status: InferenceEngineSupportStatus::Connected,
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
        });
        assert_eq!(
            build_support_coverage_report(&registry, &ledger).err(),
            Some(InferenceEngineSupportReportError::UnmappedLedgerRecord)
        );
    }

    #[test]
    fn formal_ledger_record_must_match_manifest_ownership() {
        let registry = InferenceEngineManifestRegistry::new(vec![manifest()]).expect("manifest");
        let mut ledger = ledger();
        let mut record = reviewed_record();
        record.status = InferenceEngineSupportStatus::Managed;
        ledger.records.push(record);

        assert_eq!(
            build_support_coverage_report(&registry, &ledger).err(),
            Some(InferenceEngineSupportReportError::RecordStatusMismatch)
        );
    }

    #[test]
    fn strict_report_rejects_a_stale_protocol_capability_hash() {
        let registry = InferenceEngineManifestRegistry::new(vec![manifest()]).expect("manifest");
        let mut ledger = ledger();
        ledger.records.push(reviewed_record());
        let expected_hashes = HashMap::from([(
            manifest().adapter_id,
            "0000000000000000000000000000000000000000000000000000000000000000".to_owned(),
        )]);

        assert_eq!(
            build_support_coverage_report_with_protocol_capability_hashes(
                &registry,
                &ledger,
                &expected_hashes,
            )
            .err(),
            Some(InferenceEngineSupportReportError::ProtocolCapabilityHashMismatch)
        );
    }

    #[test]
    fn manifest_only_report_fails_closed_for_external_records_without_hashes() {
        let registry = InferenceEngineManifestRegistry::new(vec![manifest()]).expect("manifest");
        let mut ledger = ledger();
        ledger.records.push(reviewed_record());

        assert_eq!(
            build_support_coverage_report(&registry, &ledger).err(),
            Some(InferenceEngineSupportReportError::MissingProtocolCapabilityHash)
        );
    }

    #[test]
    fn managed_formal_cell_uses_owned_lifecycle_evidence_instead_of_external_ledger() {
        let registry = InferenceEngineManifestRegistry::new(vec![llama_cpp_manifest()])
            .expect("managed manifest");
        let report = build_support_coverage_report(&registry, &ledger()).expect("managed report");

        assert_eq!(report.formal_support_cells, 1);
        assert_eq!(report.formal_cells_missing_ledger, 0);
        assert!(report.all_formal_cells_ledger_backed);
        assert!(report.ready_for_strict_promotion);
        assert!(report.adapters[0].ready_for_strict_promotion);
        assert_eq!(report.adapters[0].ledger_backed_cells, 0);
        assert_eq!(report.reviewed_performance_profiles, 0);
        assert_eq!(report.formal_external_cells_missing_performance_profile, 0);
    }
}
