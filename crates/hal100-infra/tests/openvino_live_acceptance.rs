mod common;

use hal100_infra::{EngineInspector, OpenVinoExternalEngineAdapter, VerifiedEngineTarget};
use hal100_protocol::{EngineProtocolCapability, InferenceAccelerator};

/// Explicit real-service qualification entry point for the first OVMS support cells.
///
/// Run this test on a fixed Windows or Linux x86_64 host with a user-owned OVMS deployment. It
/// never provisions OVMS, downloads a model, reads a process command line, or accepts a remote URL.
#[tokio::test]
#[ignore = "requires an explicitly prepared local OpenVINO Model Server on Windows/Linux x86_64"]
async fn fixed_local_openvino_model_server_passes_the_agent_protocol_vertical() {
    assert_eq!(
        std::env::var("HAL100_OPENVINO_ACCEPTANCE").as_deref(),
        Ok("1"),
        "set HAL100_OPENVINO_ACCEPTANCE=1 to acknowledge the real inference requests"
    );
    let accelerator_key = std::env::var("HAL100_OPENVINO_ACCELERATOR")
        .expect("HAL100_OPENVINO_ACCELERATOR must select cpu, intel_gpu or intel_npu");
    let accelerator = InferenceAccelerator::from_storage_key(&accelerator_key)
        .filter(|accelerator| {
            matches!(
                accelerator,
                InferenceAccelerator::Cpu
                    | InferenceAccelerator::IntelGpu
                    | InferenceAccelerator::IntelNpu
            )
        })
        .expect("HAL100_OPENVINO_ACCELERATOR must select cpu, intel_gpu or intel_npu");
    let adapter = OpenVinoExternalEngineAdapter::for_accelerator(accelerator)
        .expect("OVMS target-device adapter");
    let manifest = adapter.manifest();
    let api_root = std::env::var("HAL100_OPENVINO_API_ROOT")
        .unwrap_or_else(|_| "http://127.0.0.1:8000/v1/".to_owned());
    let model_id = std::env::var("HAL100_OPENVINO_MODEL_ID")
        .expect("HAL100_OPENVINO_MODEL_ID must name the fixed served model");
    let expected_version = std::env::var("HAL100_OPENVINO_EXPECTED_VERSION")
        .expect("HAL100_OPENVINO_EXPECTED_VERSION must pin the qualified OVMS version");
    let (support_cell, host) =
        common::prepare_real_acceptance_host(&manifest, &accelerator_key, "OpenVINO Model Server");
    assert_eq!(support_cell.accelerator, accelerator);
    let target =
        VerifiedEngineTarget::external_local("acceptance:openvino-ovms", &manifest, &api_root, 1)
            .expect("fixed loopback target");
    let snapshot = adapter.inspect(&target).await.expect("OVMS inspection");
    assert_eq!(snapshot.version, expected_version);
    assert!(snapshot.engine_version_exact);
    assert!(snapshot.model_catalog_complete);
    assert!(snapshot.models.iter().any(|model| model.name == model_id));

    let report = adapter
        .qualify(&target, &model_id)
        .await
        .expect("OVMS protocol qualification");
    assert_eq!(
        report.observed_engine_version.as_deref(),
        Some(expected_version.as_str())
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
    let stability =
        common::verify_external_stability(&target, &model_id, "OpenVINO Model Server").await;
    common::verify_external_profile_lifecycle(common::ExternalProfileLifecycleInput {
        adapter_id: &adapter.manifest().adapter_id,
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
        label: "OpenVINO Model Server",
    })
    .await;
    let resilience = common::verify_control_plane_resilience().await;
    common::emit_acceptance_run_if_opted_in(common::AcceptanceRunInput {
        adapter_id: &adapter.manifest().adapter_id,
        target: &target,
        host: &host,
        accelerator,
        engine_version: Some(expected_version.as_str()),
        deployment_fingerprint: report.deployment_fingerprint.as_deref(),
        model_id: &model_id,
        model_evidence: &snapshot
            .models
            .iter()
            .find(|model| model.name == model_id)
            .expect("qualified catalog model")
            .evidence,
        protocol_capability_hash: &report.protocol_capability_hash,
        test_source: "crates/hal100-infra/tests/openvino_live_acceptance.rs",
        lifecycle_verified: true,
        stability: Some(stability),
        resilience: Some(resilience),
    })
    .expect("emit explicit OpenVINO acceptance run artifact");
    println!(
        "qualified OpenVINO Model Server {} model {} capability {}",
        expected_version, model_id, report.protocol_capability_hash
    );
}
