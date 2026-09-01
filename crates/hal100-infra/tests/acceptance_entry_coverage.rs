mod common;

use std::collections::{HashMap, HashSet};

use hal100_infra::{
    ExternalInferenceEngineRegistry, InferenceEngineAcceptanceEvidence,
    InferenceEngineAcceptanceLedger, InferenceEngineAcceptanceResilience,
    InferenceEngineAcceptanceRun, InferenceEngineAcceptanceRunOutcome,
    InferenceEngineAcceptanceStability, InferenceEngineManifestRegistry,
    build_support_coverage_report_with_protocol_capability_hashes, llama_cpp_manifest,
};
use hal100_protocol::{
    InferenceDeployment, InferenceEngineSupportEvidenceSummary, InferenceEngineSupportStatus,
    RuntimeProfileSupportCell,
};

/// Every external manifest support cell must be selectable by the same bounded parser used by
/// the real-service tests. This prevents a manifest expansion from silently creating a cell that
/// can never emit platform evidence. It does not prove that a service or accelerator is present.
#[test]
fn every_external_manifest_support_cell_has_a_live_acceptance_entry() {
    let registry = ExternalInferenceEngineRegistry::standard().expect("external registry");
    let adapters = registry.adapters();
    assert_eq!(
        adapters.len(),
        13,
        "all planned external adapters must be covered"
    );

    let mut adapter_ids = HashSet::new();
    let mut support_cell_count = 0_usize;
    for adapter in adapters {
        let manifest = adapter.manifest();
        assert!(adapter_ids.insert(manifest.adapter_id.clone()));
        assert!(
            !manifest.support_units.is_empty(),
            "{} must declare at least one acceptance cell",
            manifest.adapter_id.engine.storage_key()
        );
        for unit in &manifest.support_units {
            assert_eq!(
                unit.deployment,
                InferenceDeployment::Local,
                "remote services require a separate target/evidence contract"
            );
            let expected = RuntimeProfileSupportCell {
                platform: unit.platform,
                architecture: unit.architecture,
                accelerator: unit.accelerator,
                deployment: unit.deployment,
            };
            assert_eq!(
                common::select_declared_acceptance_cell(
                    &manifest,
                    unit.platform.storage_key(),
                    unit.architecture.storage_key(),
                    unit.accelerator.storage_key(),
                ),
                Some(expected),
                "{} has an unreachable manifest support cell",
                manifest.adapter_id.engine.storage_key()
            );
            support_cell_count += 1;
        }
    }

    assert_eq!(support_cell_count, 28, "external support matrix drifted");
}

#[test]
fn acceptance_selector_rejects_unknown_or_undeclared_coordinates() {
    let registry = ExternalInferenceEngineRegistry::standard().expect("external registry");
    let ollama = registry
        .adapters()
        .into_iter()
        .find(|adapter| adapter.manifest().adapter_id.engine.storage_key() == "ollama")
        .expect("Ollama adapter");
    let manifest = ollama.manifest();

    assert!(common::select_declared_acceptance_cell(&manifest, "plan9", "x86_64", "cpu").is_none());
    assert!(
        common::select_declared_acceptance_cell(&manifest, "windows", "sparc", "cpu").is_none()
    );
    assert!(
        common::select_declared_acceptance_cell(&manifest, "windows", "x86_64", "cuda").is_none()
    );
}

/// Every formal external support cell must remain backed by a reviewed ledger record.
/// Host-attestation migration debt is represented inside those records and cannot be confused
/// with a missing-record exception.
#[test]
fn every_formal_external_cell_has_a_reviewed_ledger_record() {
    let external = ExternalInferenceEngineRegistry::standard().expect("external registry");
    let adapters = external.adapters();
    let mut manifests = adapters
        .iter()
        .map(|adapter| adapter.manifest())
        .collect::<Vec<_>>();
    manifests.push(llama_cpp_manifest());
    let registry = InferenceEngineManifestRegistry::new(manifests).expect("combined registry");
    let hashes = adapters
        .iter()
        .map(|adapter| {
            (
                adapter.manifest().adapter_id,
                adapter
                    .protocol_capability_hash()
                    .expect("external adapter protocol hash"),
            )
        })
        .collect::<HashMap<_, _>>();
    let ledger = InferenceEngineAcceptanceLedger::standard().expect("checked-in ledger");
    let report =
        build_support_coverage_report_with_protocol_capability_hashes(&registry, &ledger, &hashes)
            .expect("support report");

    assert_eq!(report.formal_cells_missing_ledger, 0);
    assert_eq!(report.reviewed_performance_profiles, 0);
    assert_eq!(report.formal_external_cells_missing_performance_profile, 3);
    assert!(report.all_formal_cells_ledger_backed);
    assert!(report.adapters.iter().all(|adapter| {
        adapter.ownership != hal100_protocol::InferenceEngineOwnership::External
            || adapter.cells.iter().all(|cell| {
                !matches!(
                    cell.manifest_status,
                    hal100_protocol::InferenceEngineSupportStatus::VerifiedExternal
                ) || cell.ledger_record_present
            })
    }));
}

/// Prove the complete structural promotion path for every declared external support cell.
///
/// These deterministic fixtures are deliberately not real platform evidence and never touch the
/// checked-in ledger. Their purpose is narrower: when a native live run eventually arrives, no
/// engine/platform cell may fail because the artifact, reviewer-supplied model revision, ledger
/// append, protocol-hash gate or reviewed registry projection was only wired for a subset of the
/// matrix.
#[test]
fn every_external_support_cell_can_traverse_the_formal_review_pipeline() {
    let base = ExternalInferenceEngineRegistry::standard().expect("external registry");
    let adapters = base.adapters();
    let formal_status = InferenceEngineSupportStatus::VerifiedExternal;
    let evidence_kinds = InferenceEngineSupportEvidenceSummary::for_status(formal_status).verified;
    let mut ledger = InferenceEngineAcceptanceLedger {
        schema_version: hal100_infra::INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION,
        records: Vec::new(),
    };
    let mut record_index = 0_usize;

    for adapter in &adapters {
        let manifest = adapter.manifest();
        let protocol_capability_hash = adapter
            .protocol_capability_hash()
            .expect("external adapter protocol hash");
        for unit in &manifest.support_units {
            record_index += 1;
            let run = InferenceEngineAcceptanceRun {
                schema_version: hal100_infra::INFERENCE_ENGINE_ACCEPTANCE_RUN_SCHEMA_VERSION,
                run_id: format!("pipeline-coverage-{record_index}"),
                adapter_id: manifest.adapter_id.clone(),
                instance_id: format!("acceptance:pipeline:{record_index}"),
                origin_fingerprint:
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned(),
                config_revision: 1,
                protocol_capability_hash: protocol_capability_hash.clone(),
                platform: unit.platform,
                architecture: unit.architecture,
                accelerator: unit.accelerator,
                deployment: unit.deployment,
                outcome: InferenceEngineAcceptanceRunOutcome::Passed,
                observed_at_ms: 1,
                engine_version: Some("pipeline-coverage-version".to_owned()),
                deployment_fingerprint: None,
                model_revision: Some(format!("model-id-sha256:{record_index:064x}")),
                host_summary: Some(format!(
                    "{}/{}/{}",
                    unit.platform.storage_key(),
                    unit.architecture.storage_key(),
                    unit.accelerator.storage_key()
                )),
                host_attestation: common::fixture_host_attestation(
                    unit.platform,
                    unit.architecture,
                    unit.accelerator,
                ),
                model_evidence:
                    hal100_infra::InferenceEngineAcceptanceModelEvidence::from_runtime_evidence(
                        &hal100_protocol::RuntimeProfileEvidence {
                            kind: hal100_protocol::RuntimeProfileEvidenceKind::CatalogIdentity,
                            algorithm: "acceptance-structure-model-id".to_owned(),
                            value: format!("fixture-model-{record_index}"),
                        },
                    )
                    .expect("bounded model evidence"),
                stability: Some(InferenceEngineAcceptanceStability {
                    workload_revision: hal100_infra::OPENAI_STABILITY_WORKLOAD_REVISION.to_owned(),
                    attempts: 20,
                    concurrency: 4,
                    p95_latency_ms: 1,
                    max_latency_ms: 1,
                    total_prompt_tokens: 40,
                    total_completion_tokens: 20,
                    wall_time_ms: 5,
                }),
                resilience: Some(InferenceEngineAcceptanceResilience::complete()),
                evidence: evidence_kinds
                    .iter()
                    .copied()
                    .map(|kind| InferenceEngineAcceptanceEvidence {
                        kind,
                        source: "crates/hal100-infra/tests/acceptance_entry_coverage.rs".to_owned(),
                        assertion: "structural acceptance pipeline coverage fixture".to_owned(),
                    })
                    .collect(),
            };
            run.validate().expect("acceptance run contract");
            let record = run
                .into_formal_record_with_model_revision(
                    formal_status,
                    &format!("reviewed-model-revision-{record_index}"),
                )
                .expect("human-reviewed formal record");
            ledger
                .append_reviewed_record(record)
                .expect("atomic ledger append");
        }
    }

    assert_eq!(record_index, 28, "external support matrix drifted");
    ledger.validate().expect("complete synthetic ledger");

    let promoted = ExternalInferenceEngineRegistry::new_with_reviewed_acceptance_evidence(
        adapters.clone(),
        &ledger,
    )
    .expect("strict reviewed registry projection");
    assert!(promoted.adapters().iter().all(|adapter| {
        adapter.manifest().support_units.iter().all(|unit| {
            unit.status == InferenceEngineSupportStatus::VerifiedExternal
                && unit.evidence.as_ref().is_some_and(|summary| {
                    summary.missing.is_empty() && summary.verified.len() == evidence_kinds.len()
                })
        })
    }));

    let mut manifests = adapters
        .iter()
        .map(|adapter| adapter.manifest())
        .collect::<Vec<_>>();
    manifests.push(llama_cpp_manifest());
    let manifest_registry =
        InferenceEngineManifestRegistry::new(manifests).expect("combined manifest registry");
    let hashes = adapters
        .iter()
        .map(|adapter| {
            (
                adapter.manifest().adapter_id,
                adapter
                    .protocol_capability_hash()
                    .expect("external adapter protocol hash"),
            )
        })
        .collect::<HashMap<_, _>>();
    let report = build_support_coverage_report_with_protocol_capability_hashes(
        &manifest_registry,
        &ledger,
        &hashes,
    )
    .expect("strict synthetic support report");
    assert_eq!(report.total_support_cells, 29);
    assert_eq!(report.formal_support_cells, 29);
    assert_eq!(report.pending_support_cells, 0);
    assert_eq!(report.formal_cells_missing_ledger, 0);
    assert_eq!(report.reviewed_performance_profiles, 28);
    assert_eq!(report.formal_external_cells_missing_performance_profile, 0);
    assert!(report.ready_for_strict_promotion);
}
