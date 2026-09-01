mod common;

use hal100_infra::{EngineInspector, TensorRtLlmExternalEngineAdapter, VerifiedEngineTarget};
use hal100_protocol::EngineProtocolCapability;

/// Explicit real-service qualification entry point for TensorRT-LLM Linux/CUDA support cells.
///
/// The caller owns the fixed service and model. HAL100 never starts TensorRT-LLM, downloads
/// checkpoints, reads command lines, or accepts a non-loopback URL from this test. Passing this
/// probe still does not promote a cell by itself: GPU identity, backend/configuration evidence and
/// the save/activate/recovery vertical must be recorded separately.
#[tokio::test]
#[ignore = "requires an explicitly prepared local trtllm-serve service on Linux NVIDIA CUDA"]
async fn fixed_local_tensorrt_llm_service_passes_the_agent_protocol_vertical() {
    assert_eq!(
        std::env::var("HAL100_TENSORRT_LLM_ACCEPTANCE").as_deref(),
        Ok("1"),
        "set HAL100_TENSORRT_LLM_ACCEPTANCE=1 to acknowledge the real inference requests"
    );
    let adapter = TensorRtLlmExternalEngineAdapter::new().expect("TensorRT-LLM adapter");
    let manifest = adapter.manifest();
    let (support_cell, host) =
        common::prepare_real_acceptance_host(&manifest, "cuda", "TensorRT-LLM");
    let api_root = std::env::var("HAL100_TENSORRT_LLM_API_ROOT")
        .unwrap_or_else(|_| "http://127.0.0.1:8000/v1/".to_owned());
    let model_id = std::env::var("HAL100_TENSORRT_LLM_MODEL_ID")
        .expect("HAL100_TENSORRT_LLM_MODEL_ID must name the fixed served model");
    let expected_version = std::env::var("HAL100_TENSORRT_LLM_EXPECTED_VERSION")
        .expect("HAL100_TENSORRT_LLM_EXPECTED_VERSION must pin the qualified service version");

    let target =
        VerifiedEngineTarget::external_local("acceptance:tensorrt-llm", &manifest, &api_root, 1)
            .expect("fixed loopback target");
    let snapshot = adapter
        .inspect(&target)
        .await
        .expect("TensorRT-LLM inspection");
    assert_eq!(snapshot.version, expected_version);
    assert!(snapshot.engine_version_exact);
    assert!(snapshot.model_catalog_complete);
    assert!(snapshot.models.iter().any(|model| model.name == model_id));

    let report = adapter
        .qualify(&target, &model_id)
        .await
        .expect("TensorRT-LLM protocol qualification");
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
    let stability = common::verify_external_stability(&target, &model_id, "TensorRT-LLM").await;
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
        api_key: None,
        label: "TensorRT-LLM",
    })
    .await;
    let resilience = common::verify_control_plane_resilience().await;
    common::emit_acceptance_run_if_opted_in(common::AcceptanceRunInput {
        adapter_id: &manifest.adapter_id,
        target: &target,
        host: &host,
        accelerator: support_cell.accelerator,
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
        test_source: "crates/hal100-infra/tests/tensorrt_llm_live_acceptance.rs",
        lifecycle_verified: true,
        stability: Some(stability),
        resilience: Some(resilience),
    })
    .expect("emit explicit TensorRT-LLM acceptance run artifact");
    println!(
        "qualified TensorRT-LLM {} model {} capability {}",
        expected_version, model_id, report.protocol_capability_hash
    );
}
