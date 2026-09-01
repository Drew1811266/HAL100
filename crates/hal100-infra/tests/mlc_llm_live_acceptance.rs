mod common;

use hal100_infra::{EngineInspector, MlcLlmExternalEngineAdapter, VerifiedEngineTarget};
use hal100_protocol::{EngineProtocolCapability, InferenceAccelerator};

/// Explicit real-service qualification for one user-selected MLC LLM support cell.
///
/// MLC's official REST service has no stable package-version endpoint and emits an empty
/// `system_fingerprint`. The adapter therefore requires an absolute local MLC deployment directory
/// and hashes its bounded config, weight manifest, declared shards and tokenizer files. The
/// operator separately records the exact reviewed package version used to start the service. This
/// test never provisions a compiled model library or starts a process.
#[tokio::test]
#[ignore = "requires an explicitly prepared local MLC LLM service and compiled model library"]
async fn fixed_local_mlc_llm_service_passes_the_agent_protocol_vertical() {
    assert_eq!(
        std::env::var("HAL100_MLC_LLM_ACCEPTANCE").as_deref(),
        Ok("1"),
        "set HAL100_MLC_LLM_ACCEPTANCE=1 to acknowledge the real inference requests"
    );
    let accelerator_key = std::env::var("HAL100_MLC_LLM_ACCELERATOR")
        .expect("HAL100_MLC_LLM_ACCELERATOR must declare the prepared cell");
    let declared_accelerator = InferenceAccelerator::from_storage_key(&accelerator_key)
        .expect("HAL100_MLC_LLM_ACCELERATOR must use a typed accelerator key");
    let adapter = MlcLlmExternalEngineAdapter::for_accelerator(declared_accelerator)
        .expect("MLC LLM device-specific adapter");
    let manifest = adapter.manifest();
    let (support_cell, host) =
        common::prepare_real_acceptance_host(&manifest, &accelerator_key, "MLC LLM");
    let accelerator = support_cell.accelerator;
    let api_root = std::env::var("HAL100_MLC_LLM_API_ROOT")
        .unwrap_or_else(|_| "http://127.0.0.1:8000/v1/".to_owned());
    let model_id = std::env::var("HAL100_MLC_LLM_MODEL_ID")
        .expect("HAL100_MLC_LLM_MODEL_ID must name the absolute local MLC deployment directory");
    let engine_version = std::env::var("HAL100_MLC_LLM_ENGINE_VERSION")
        .expect("HAL100_MLC_LLM_ENGINE_VERSION must name the reviewed package version");
    assert!(
        !engine_version.trim().is_empty() && engine_version.len() <= 128,
        "HAL100_MLC_LLM_ENGINE_VERSION must be bounded non-empty text"
    );

    let target =
        VerifiedEngineTarget::external_local("acceptance:mlc-llm", &manifest, &api_root, 1)
            .expect("fixed loopback target");
    let snapshot = adapter.inspect(&target).await.expect("MLC LLM inspection");
    assert!(!snapshot.engine_version_exact);
    assert!(snapshot.model_catalog_complete);
    assert!(snapshot.models.iter().any(|model| model.name == model_id));

    let report = adapter
        .qualify(&target, &model_id)
        .await
        .expect("MLC LLM protocol qualification");
    assert!(report.observed_engine_version.is_none());
    if let Some(fingerprint) = report.deployment_fingerprint.as_deref() {
        assert_eq!(fingerprint.len(), 64);
        assert!(fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
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
    let stability = common::verify_external_stability(&target, &model_id, "MLC LLM").await;

    let deployment_fingerprint = report
        .deployment_fingerprint
        .as_deref()
        .expect("MLC LLM local deployment must produce a content fingerprint");
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
        label: "MLC LLM",
    })
    .await;
    let resilience = common::verify_control_plane_resilience().await;
    common::emit_acceptance_run_if_opted_in(common::AcceptanceRunInput {
        adapter_id: &manifest.adapter_id,
        target: &target,
        host: &host,
        accelerator,
        engine_version: Some(&engine_version),
        deployment_fingerprint: Some(deployment_fingerprint),
        model_id: &model_id,
        model_evidence: &snapshot
            .models
            .iter()
            .find(|model| model.name == model_id)
            .expect("qualified catalog model")
            .evidence,
        protocol_capability_hash: &report.protocol_capability_hash,
        test_source: "crates/hal100-infra/tests/mlc_llm_live_acceptance.rs",
        lifecycle_verified: true,
        stability: Some(stability),
        resilience: Some(resilience),
    })
    .expect("emit explicit MLC LLM acceptance run artifact");
    println!(
        "qualified MLC LLM {} model {} capability {} with local deployment identity",
        engine_version, model_id, report.protocol_capability_hash
    );
}
