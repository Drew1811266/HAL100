mod common;

use hal100_infra::{
    EngineInspector, EngineRequestAuth, VerifiedEngineTarget, VllmExternalEngineAdapter,
};
use hal100_protocol::EngineProtocolCapability;

/// Explicit real-service qualification for the first vLLM support cell.
///
/// Run this test on the target Linux CUDA host with a fixed vLLM deployment. It intentionally
/// refuses remote URLs and does not provision vLLM, download a model, or infer credentials.
#[tokio::test]
#[ignore = "requires an explicitly prepared local vLLM service on a Linux CUDA host"]
async fn fixed_local_vllm_service_passes_the_agent_protocol_vertical() {
    assert_eq!(
        std::env::var("HAL100_VLLM_ACCEPTANCE").as_deref(),
        Ok("1"),
        "set HAL100_VLLM_ACCEPTANCE=1 to acknowledge the real inference request"
    );
    let adapter = VllmExternalEngineAdapter::new().expect("vLLM adapter");
    let manifest = adapter.manifest();
    let (support_cell, host) = common::prepare_real_acceptance_host(&manifest, "cuda", "vLLM");
    let api_root = std::env::var("HAL100_VLLM_API_ROOT")
        .unwrap_or_else(|_| "http://127.0.0.1:8000/v1/".to_owned());
    let model_id = std::env::var("HAL100_VLLM_MODEL_ID")
        .expect("HAL100_VLLM_MODEL_ID must name the fixed served model");
    let expected_version = std::env::var("HAL100_VLLM_EXPECTED_VERSION")
        .expect("HAL100_VLLM_EXPECTED_VERSION must pin the qualified service version");
    let api_key = std::env::var("HAL100_VLLM_API_KEY")
        .ok()
        .filter(|value| !value.is_empty());
    let request_auth = api_key
        .as_deref()
        .map(|value| EngineRequestAuth::bearer(value).expect("bounded key"))
        .unwrap_or(EngineRequestAuth::None);

    let target = VerifiedEngineTarget::external_local_with_auth(
        "acceptance:vllm",
        &manifest,
        &api_root,
        1,
        request_auth,
    )
    .expect("fixed loopback target");
    let snapshot = adapter.inspect(&target).await.expect("vLLM inspection");
    assert_eq!(snapshot.version, expected_version);
    assert!(snapshot.model_catalog_complete);
    assert!(snapshot.models.iter().any(|model| model.name == model_id));

    let report = adapter
        .qualify(&target, &model_id)
        .await
        .expect("vLLM protocol qualification");
    assert_eq!(
        report.observed_engine_version.as_deref(),
        Some(expected_version.as_str()),
        "vLLM qualification must bind the same exact version as inspection"
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
    let stability = common::verify_external_stability(&target, &model_id, "vLLM").await;
    common::verify_external_profile_lifecycle(common::ExternalProfileLifecycleInput {
        adapter_id: &manifest.adapter_id,
        api_root: &api_root,
        model_id: &model_id,
        expected_evidence: snapshot
            .models
            .iter()
            .find(|model| model.name == model_id)
            .expect("qualified catalog model")
            .evidence
            .clone(),
        host: host.clone(),
        accelerator: support_cell.accelerator,
        api_key: api_key.as_deref(),
        label: "vLLM",
    })
    .await;
    let resilience = common::verify_control_plane_resilience().await;
    common::emit_acceptance_run_if_opted_in(common::AcceptanceRunInput {
        adapter_id: &manifest.adapter_id,
        target: &target,
        host: &host,
        accelerator: support_cell.accelerator,
        engine_version: Some(snapshot.version.as_str()),
        deployment_fingerprint: report.deployment_fingerprint.as_deref(),
        model_id: &model_id,
        model_evidence: &snapshot
            .models
            .iter()
            .find(|model| model.name == model_id)
            .expect("qualified catalog model")
            .evidence,
        protocol_capability_hash: &report.protocol_capability_hash,
        test_source: "crates/hal100-infra/tests/vllm_live_acceptance.rs",
        lifecycle_verified: true,
        stability: Some(stability),
        resilience: Some(resilience),
    })
    .expect("emit explicit vLLM acceptance run artifact");
    println!(
        "qualified vLLM {} model {} capability {}",
        snapshot.version, model_id, report.protocol_capability_hash
    );
}
