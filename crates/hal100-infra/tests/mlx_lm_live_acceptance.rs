mod common;

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use hal100_core::{SecretStore, SecretStoreError, SecretStoreOperation};
use hal100_infra::{
    BackendManager, CredentialRegistry, Database, EngineInspector, ExternalInferenceEngineRegistry,
    GatewayState, LlamaCppManager, MlxLmExternalEngineAdapter, OpenAiQualificationOptions,
    RuntimeProfileManager, UsageWriter, VerifiedEngineTarget,
};
use hal100_protocol::{
    BackendAuthMethod, BackendDraft, BackendKind, EngineProtocolCapability,
    ExternalRuntimeProfileDraft, InferenceEngineKind, InferenceEngineSupportStatus,
};
use uuid::Uuid;

#[derive(Default)]
struct AcceptanceSecretStore {
    values: Mutex<HashMap<String, Vec<u8>>>,
}

impl SecretStore for AcceptanceSecretStore {
    fn read(&self, credential_id: &str) -> Result<Option<Vec<u8>>, SecretStoreError> {
        Ok(self
            .values
            .lock()
            .map_err(|_| SecretStoreError::new(SecretStoreOperation::Read))?
            .get(credential_id)
            .cloned())
    }

    fn write(&self, credential_id: &str, secret: &[u8]) -> Result<(), SecretStoreError> {
        self.values
            .lock()
            .map_err(|_| SecretStoreError::new(SecretStoreOperation::Write))?
            .insert(credential_id.to_owned(), secret.to_vec());
        Ok(())
    }

    fn delete(&self, credential_id: &str) -> Result<(), SecretStoreError> {
        self.values
            .lock()
            .map_err(|_| SecretStoreError::new(SecretStoreOperation::Delete))?
            .remove(credential_id);
        Ok(())
    }
}

/// Explicit real-service qualification for the Apple Silicon MLX-LM support cell.
///
/// The caller owns the fixed local server and model. This test never downloads a model or starts
/// a process implicitly, and it refuses non-loopback targets through `VerifiedEngineTarget`.
#[tokio::test]
#[ignore = "requires an explicitly prepared local mlx_lm.server on Apple Silicon"]
async fn fixed_local_mlx_lm_service_passes_the_agent_protocol_vertical() {
    assert_eq!(
        std::env::var("HAL100_MLX_LM_ACCEPTANCE").as_deref(),
        Ok("1"),
        "set HAL100_MLX_LM_ACCEPTANCE=1 to acknowledge the real inference requests"
    );
    let adapter = MlxLmExternalEngineAdapter::new().expect("MLX-LM adapter");
    let manifest = adapter.manifest();
    let (support_cell, host) = common::prepare_real_acceptance_host(&manifest, "metal", "MLX-LM");
    let api_root = std::env::var("HAL100_MLX_LM_API_ROOT")
        .unwrap_or_else(|_| "http://127.0.0.1:8080/v1/".to_owned());
    let model_id = std::env::var("HAL100_MLX_LM_MODEL_ID")
        .expect("HAL100_MLX_LM_MODEL_ID must name the fixed served model");
    let expected_version = std::env::var("HAL100_MLX_LM_EXPECTED_VERSION")
        .expect("HAL100_MLX_LM_EXPECTED_VERSION must pin the qualified package version");

    let target = VerifiedEngineTarget::external_local("acceptance:mlx-lm", &manifest, &api_root, 1)
        .expect("fixed loopback target");
    let snapshot = adapter.inspect(&target).await.expect("MLX-LM inspection");
    assert!(!snapshot.engine_version_exact);
    assert!(snapshot.model_catalog_complete);
    assert!(snapshot.models.iter().any(|model| model.name == model_id));

    let report = adapter
        .qualify(&target, &model_id)
        .await
        .expect("MLX-LM protocol qualification");
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
    let stability = common::verify_external_stability_with_options(
        &target,
        &model_id,
        "MLX-LM",
        OpenAiQualificationOptions {
            chat_template_kwargs: Some(serde_json::json!({"enable_thinking": false})),
            ..OpenAiQualificationOptions::default()
        },
    )
    .await;
    assert!(
        manifest
            .support_units
            .iter()
            .all(|unit| unit.status == InferenceEngineSupportStatus::VerifiedExternal)
    );

    let database = Arc::new(Database::open_in_memory().expect("acceptance database"));
    let gateway = GatewayState::new(
        None,
        CredentialRegistry::new(Vec::new()),
        UsageWriter::start(database.clone()),
    )
    .expect("acceptance gateway");
    let backend_manager = Arc::new(BackendManager::new(
        database.clone(),
        gateway.clone(),
        Arc::new(AcceptanceSecretStore::default()),
    ));
    let backend_catalog = backend_manager
        .save_backend(BackendDraft {
            id: None,
            display_name: "MLX-LM real acceptance".to_owned(),
            kind: BackendKind::ExternalOpenAi,
            engine: Some(InferenceEngineKind::MlxLm),
            adapter_variant: Some("official-http-server".to_owned()),
            api_root: api_root.clone(),
            auth_method: BackendAuthMethod::None,
            api_key: None,
        })
        .await
        .expect("persist exact MLX-LM backend");
    let backend_id = backend_catalog.backends[0].id.clone();
    let engine_root: PathBuf = std::env::temp_dir().join(format!(
        "hal100-mlx-lm-acceptance-{}",
        Uuid::new_v4().simple()
    ));
    let managed_engine = Arc::new(
        LlamaCppManager::new(database.clone(), gateway.clone(), engine_root.clone())
            .expect("managed engine boundary"),
    );
    let registry = Arc::new(ExternalInferenceEngineRegistry::standard().expect("registry"));
    let manager = RuntimeProfileManager::with_external_context(
        database,
        managed_engine,
        host.clone(),
        backend_manager,
        gateway,
        registry,
    );
    let catalog = manager
        .save_external(ExternalRuntimeProfileDraft {
            name: "MLX-LM real acceptance".to_owned(),
            description: "Apple Silicon active protocol qualification".to_owned(),
            backend_id,
            model_id: model_id.clone(),
            expected_evidence: snapshot
                .models
                .iter()
                .find(|model| model.name == model_id)
                .expect("qualified catalog model")
                .evidence
                .clone(),
            support_cell: Some(hal100_protocol::RuntimeProfileSupportCell {
                platform: host.platform,
                architecture: host.architecture,
                accelerator: support_cell.accelerator,
                deployment: hal100_protocol::InferenceDeployment::Local,
            }),
        })
        .await
        .expect("save qualified runtime profile");
    let profile = catalog.profiles.first().expect("saved profile");
    assert_eq!(profile.engine_version, expected_version);
    let plan = manager
        .plan_activation_verified(&profile.id)
        .await
        .expect("plan verified activation");
    let result = manager
        .apply_activation(&plan.plan_id)
        .await
        .expect("activate and post-verify MLX-LM profile");
    assert_eq!(result.active_model_id, model_id);
    assert!(
        manager
            .verify_active_profile(&profile.id)
            .await
            .expect("verify active profile")
    );
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
        test_source: "crates/hal100-infra/tests/mlx_lm_live_acceptance.rs",
        lifecycle_verified: true,
        stability: Some(stability),
        resilience: Some(resilience),
    })
    .expect("emit explicit MLX-LM acceptance run artifact");
    let _ = std::fs::remove_dir_all(engine_root);
    println!(
        "qualified MLX-LM {} model {} capability {}",
        expected_version, model_id, report.protocol_capability_hash
    );
}
