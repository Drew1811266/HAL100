mod common;

use hal100_infra::{
    ExternalInferenceEngineRegistry, InferenceEngineAcceptanceEvidence,
    InferenceEngineAcceptanceLedger, InferenceEngineAcceptanceRecord,
    InferenceEngineAcceptanceResilience, InferenceEngineAcceptanceStability,
    OPENAI_STABILITY_WORKLOAD_REVISION,
};
use hal100_protocol::{
    InferenceEngineKind, InferenceEngineSupportEvidenceSummary, InferenceEngineSupportStatus,
};

fn reviewed_record(
    adapter: &dyn hal100_infra::ExternalInferenceEngineAdapter,
    unit: &hal100_protocol::InferenceEngineSupportUnit,
    index: usize,
) -> InferenceEngineAcceptanceRecord {
    let status = InferenceEngineSupportStatus::VerifiedExternal;
    InferenceEngineAcceptanceRecord {
        id: format!("projection-record-{index}"),
        adapter_id: adapter.manifest().adapter_id,
        instance_id: format!(
            "projection:{}",
            adapter.manifest().adapter_id.engine.storage_key()
        ),
        origin_fingerprint: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
            .to_owned(),
        config_revision: 1,
        protocol_capability_hash: adapter
            .protocol_capability_hash()
            .expect("every standard adapter has a fixed protocol contract"),
        platform: unit.platform,
        architecture: unit.architecture,
        accelerator: unit.accelerator,
        deployment: unit.deployment,
        status,
        verified_at_ms: 1,
        engine_version: Some("projection-fixture-1".to_owned()),
        deployment_fingerprint: None,
        model_revision: Some("projection-model@immutable".to_owned()),
        host_summary: Some(format!(
            "{}/{}/{}",
            unit.platform.storage_key(),
            unit.architecture.storage_key(),
            unit.accelerator.storage_key()
        )),
        host_attestation: Some(common::fixture_host_attestation(
            unit.platform,
            unit.architecture,
            unit.accelerator,
        )),
        model_evidence: Some(
            hal100_infra::InferenceEngineAcceptanceModelEvidence::from_runtime_evidence(
                &hal100_protocol::RuntimeProfileEvidence {
                    kind: hal100_protocol::RuntimeProfileEvidenceKind::CatalogIdentity,
                    algorithm: "projection-model-id".to_owned(),
                    value: format!("projection-model-{index}"),
                },
            )
            .expect("bounded model evidence"),
        ),
        stability: Some(InferenceEngineAcceptanceStability {
            workload_revision: OPENAI_STABILITY_WORKLOAD_REVISION.to_owned(),
            attempts: 20,
            concurrency: 4,
            p95_latency_ms: 90,
            max_latency_ms: 100,
            total_prompt_tokens: 40,
            total_completion_tokens: 20,
            wall_time_ms: 500,
        }),
        resilience: Some(InferenceEngineAcceptanceResilience::complete()),
        evidence: InferenceEngineSupportEvidenceSummary::for_status(status)
            .verified
            .into_iter()
            .map(|kind| InferenceEngineAcceptanceEvidence {
                kind,
                source: "crates/hal100-infra/tests/reviewed_registry_projection.rs".to_owned(),
                assertion: "typed reviewed projection fixture".to_owned(),
            })
            .collect(),
    }
}

#[test]
fn every_standard_external_adapter_accepts_only_its_reviewed_support_cell() {
    let standard = ExternalInferenceEngineRegistry::standard().expect("standard registry");
    let mut ledger = InferenceEngineAcceptanceLedger {
        schema_version: hal100_infra::INFERENCE_ENGINE_ACCEPTANCE_EVIDENCE_SCHEMA_VERSION,
        records: Vec::new(),
    };
    let mut selected = Vec::new();

    for (index, adapter) in standard.adapters().into_iter().enumerate() {
        let manifest = adapter.manifest();
        let unit = manifest
            .support_units
            .first()
            .expect("standard adapter declares a support cell")
            .clone();
        ledger
            .append_reviewed_record(reviewed_record(adapter.as_ref(), &unit, index))
            .expect("reviewed record matches bounded ledger contract");
        selected.push((manifest.adapter_id, unit));
    }
    assert_eq!(
        selected.len(),
        13,
        "all standard external adapters are covered"
    );

    let promoted =
        ExternalInferenceEngineRegistry::standard_with_reviewed_acceptance_ledger(&ledger)
            .expect("reviewed ledger projects through every standard adapter");

    for (adapter_id, selected_unit) in selected {
        let manifest = promoted
            .manifest_registry()
            .manifest(&adapter_id)
            .expect("projected adapter manifest");
        let exact = manifest
            .support_units
            .iter()
            .find(|unit| {
                unit.platform == selected_unit.platform
                    && unit.architecture == selected_unit.architecture
                    && unit.accelerator == selected_unit.accelerator
                    && unit.deployment == selected_unit.deployment
            })
            .expect("selected support cell remains declared");
        assert_eq!(
            exact.status,
            InferenceEngineSupportStatus::VerifiedExternal,
            "only the reviewed cell is promoted for {}",
            adapter_id.engine.storage_key()
        );
        assert_eq!(
            exact.evidence,
            Some(InferenceEngineSupportEvidenceSummary::for_status(
                InferenceEngineSupportStatus::VerifiedExternal
            ))
        );

        let unselected = manifest.support_units.iter().filter(|unit| {
            unit.platform != selected_unit.platform
                || unit.architecture != selected_unit.architecture
                || unit.accelerator != selected_unit.accelerator
                || unit.deployment != selected_unit.deployment
        });
        let base = standard
            .manifest_registry()
            .manifest(&adapter_id)
            .expect("base adapter manifest");
        for unit in unselected {
            let base_unit = base
                .support_units
                .iter()
                .find(|candidate| {
                    candidate.platform == unit.platform
                        && candidate.architecture == unit.architecture
                        && candidate.accelerator == unit.accelerator
                        && candidate.deployment == unit.deployment
                })
                .expect("base support cell");
            assert_eq!(unit.status, base_unit.status);
        }
    }

    // Keep the explicit engine list visible in this test so a newly added adapter cannot be
    // silently omitted from the reviewed projection coverage.
    for engine in [
        InferenceEngineKind::Ollama,
        InferenceEngineKind::Vllm,
        InferenceEngineKind::MlxLm,
        InferenceEngineKind::MlcLlm,
        InferenceEngineKind::OpenVino,
        InferenceEngineKind::Sglang,
        InferenceEngineKind::LmDeploy,
        InferenceEngineKind::TensorRtLlm,
    ] {
        assert!(
            promoted
                .manifest_registry()
                .manifests_for_engine(engine)
                .iter()
                .any(|manifest| manifest
                    .support_units
                    .iter()
                    .any(|unit| { unit.status == InferenceEngineSupportStatus::VerifiedExternal })),
            "reviewed projection missing {engine:?}"
        );
    }
}
