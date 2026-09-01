mod common;

use hal100_infra::{EngineInspector, OllamaExternalEngineAdapter, VerifiedEngineTarget};
use hal100_protocol::{EngineProtocolCapability, EngineRuntimeDeviceEvidence};

/// Explicit real-service qualification for one declared native Ollama support cell.
///
/// The caller owns the fixed local Ollama service and model. HAL100 never starts Ollama, pulls a
/// model, reads process arguments or accepts a non-loopback endpoint from this test.
#[tokio::test]
#[ignore = "requires an explicitly prepared local Ollama service on a declared native support cell"]
async fn fixed_local_ollama_service_passes_the_agent_protocol_vertical() {
    assert_eq!(
        std::env::var("HAL100_OLLAMA_ACCEPTANCE").as_deref(),
        Ok("1"),
        "set HAL100_OLLAMA_ACCEPTANCE=1 to acknowledge the real inference requests"
    );
    let adapter = OllamaExternalEngineAdapter::new().expect("Ollama adapter");
    let manifest = adapter.manifest();
    let accelerator_key = std::env::var("HAL100_OLLAMA_ACCELERATOR")
        .expect("HAL100_OLLAMA_ACCELERATOR must declare the prepared support cell");
    let (support_cell, host) =
        common::prepare_real_acceptance_host(&manifest, &accelerator_key, "Ollama");
    let accelerator = support_cell.accelerator;
    let api_root = std::env::var("HAL100_OLLAMA_API_ROOT")
        .unwrap_or_else(|_| "http://127.0.0.1:11434/v1/".to_owned());
    let model_id = std::env::var("HAL100_OLLAMA_MODEL_ID")
        .expect("HAL100_OLLAMA_MODEL_ID must name the fixed served model");
    let expected_version = std::env::var("HAL100_OLLAMA_EXPECTED_VERSION")
        .expect("HAL100_OLLAMA_EXPECTED_VERSION must pin the qualified Ollama version");

    let target = VerifiedEngineTarget::external_local("acceptance:ollama", &manifest, &api_root, 1)
        .expect("fixed loopback target");
    let snapshot = adapter.inspect(&target).await.expect("Ollama inspection");
    assert_eq!(snapshot.version, expected_version);
    assert!(snapshot.engine_version_exact);
    assert!(snapshot.model_catalog_complete);
    let model = snapshot
        .models
        .iter()
        .find(|model| model.name == model_id)
        .expect("qualified catalog model");

    let report = adapter
        .qualify(&target, &model_id)
        .await
        .expect("Ollama protocol qualification");
    assert_eq!(
        report.observed_engine_version.as_deref(),
        Some(expected_version.as_str())
    );
    assert_eq!(
        report.runtime_device_evidence,
        EngineRuntimeDeviceEvidence::ModelResidencyObservation { accelerator },
        "Ollama runtime placement must prove the declared acceptance accelerator"
    );
    for required in [
        EngineProtocolCapability::ModelsList,
        EngineProtocolCapability::ChatCompletionsUnary,
        EngineProtocolCapability::ChatCompletionsStream,
        EngineProtocolCapability::UsagePromptCompletion,
        EngineProtocolCapability::ToolCallsSingle,
    ] {
        assert!(
            report
                .protocol_capabilities
                .capabilities
                .contains(&required)
        );
    }
    assert_eq!(report.protocol_capability_hash.len(), 64);
    let stability = common::verify_external_stability_with_options(
        &target,
        &model_id,
        "Ollama",
        OllamaExternalEngineAdapter::openai_qualification_options(),
    )
    .await;
    common::verify_external_profile_lifecycle(common::ExternalProfileLifecycleInput {
        adapter_id: &manifest.adapter_id,
        api_root: &api_root,
        model_id: &model_id,
        expected_evidence: model.evidence.clone(),
        host: host.clone(),
        accelerator,
        api_key: None,
        label: "Ollama",
    })
    .await;
    let resilience = common::verify_control_plane_resilience().await;
    common::emit_acceptance_run_if_opted_in(common::AcceptanceRunInput {
        adapter_id: &manifest.adapter_id,
        target: &target,
        host: &host,
        accelerator,
        engine_version: Some(expected_version.as_str()),
        deployment_fingerprint: report.deployment_fingerprint.as_deref(),
        model_id: &model_id,
        model_evidence: &model.evidence,
        protocol_capability_hash: &report.protocol_capability_hash,
        test_source: "crates/hal100-infra/tests/ollama_live_acceptance.rs",
        lifecycle_verified: true,
        stability: Some(stability),
        resilience: Some(resilience),
    })
    .expect("emit explicit Ollama acceptance run artifact");
    println!(
        "qualified Ollama {} model {} capability {}",
        expected_version, model_id, report.protocol_capability_hash
    );
}
