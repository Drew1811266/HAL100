mod common;

use hal100_infra::{EngineInspector, LmDeployExternalEngineAdapter, VerifiedEngineTarget};
use hal100_protocol::EngineProtocolCapability;

/// Explicit real-service qualification entry point for the LMDeploy Linux/Windows CUDA cells.
///
/// The caller owns the fixed service and model. HAL100 never starts LMDeploy, downloads weights,
/// reads command lines, or accepts a non-loopback URL from this test. Because the official API
/// does not expose a stable package-version endpoint, the test requires a stable non-empty
/// `system_fingerprint` (bound to the model as a deployment identity) before recording the full
/// lifecycle evidence. The checked-in support cell remains `connected` until review.
#[tokio::test]
#[ignore = "requires an explicitly prepared local LMDeploy service on Linux/Windows x86_64 CUDA"]
async fn fixed_local_lmdeploy_service_passes_the_agent_protocol_vertical() {
    assert_eq!(
        std::env::var("HAL100_LMDEPLOY_ACCEPTANCE").as_deref(),
        Ok("1"),
        "set HAL100_LMDEPLOY_ACCEPTANCE=1 to acknowledge the real inference requests"
    );
    let adapter = LmDeployExternalEngineAdapter::new().expect("LMDeploy adapter");
    let manifest = adapter.manifest();
    let accelerator_key = std::env::var("HAL100_LMDEPLOY_ACCELERATOR")
        .expect("HAL100_LMDEPLOY_ACCELERATOR must declare the prepared CUDA cell");
    let (support_cell, host) =
        common::prepare_real_acceptance_host(&manifest, &accelerator_key, "LMDeploy");
    let api_root = std::env::var("HAL100_LMDEPLOY_API_ROOT")
        .unwrap_or_else(|_| "http://127.0.0.1:23333/v1/".to_owned());
    let model_id = std::env::var("HAL100_LMDEPLOY_MODEL_ID")
        .expect("HAL100_LMDEPLOY_MODEL_ID must name the fixed served model");

    let target =
        VerifiedEngineTarget::external_local("acceptance:lmdeploy", &manifest, &api_root, 1)
            .expect("fixed loopback target");
    let snapshot = adapter.inspect(&target).await.expect("LMDeploy inspection");
    assert_eq!(snapshot.version, "qualification-required");
    assert!(!snapshot.engine_version_exact);
    assert!(snapshot.model_catalog_complete);
    assert!(snapshot.models.iter().any(|model| model.name == model_id));

    let report = adapter
        .qualify(&target, &model_id)
        .await
        .expect("LMDeploy protocol qualification");
    assert!(report.observed_engine_version.is_none());
    let deployment_fingerprint = report
        .deployment_fingerprint
        .as_deref()
        .expect("LMDeploy must expose a non-empty system_fingerprint for formal acceptance");
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
    let stability = common::verify_external_stability(&target, &model_id, "LMDeploy").await;
    let accelerator = support_cell.accelerator;
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
        accelerator,
        api_key: None,
        label: "LMDeploy",
    })
    .await;
    let resilience = common::verify_control_plane_resilience().await;
    common::emit_acceptance_run_if_opted_in(common::AcceptanceRunInput {
        adapter_id: &manifest.adapter_id,
        target: &target,
        host: &host,
        accelerator,
        engine_version: None,
        deployment_fingerprint: Some(deployment_fingerprint),
        model_id: &model_id,
        model_evidence: &snapshot
            .models
            .iter()
            .find(|model| model.name == model_id)
            .expect("qualified catalog model")
            .evidence,
        protocol_capability_hash: &report.protocol_capability_hash,
        test_source: "crates/hal100-infra/tests/lmdeploy_live_acceptance.rs",
        lifecycle_verified: true,
        stability: Some(stability),
        resilience: Some(resilience),
    })
    .expect("emit explicit LMDeploy acceptance run artifact");
    println!(
        "qualified LMDeploy model {} capability {} (deployment fingerprint used as identity)",
        model_id, report.protocol_capability_hash
    );
}
