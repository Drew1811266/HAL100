use std::{collections::HashSet, time::Duration};

use hal100_protocol::{
    ENGINE_ADAPTER_CONTRACT_REVISION, EngineAdapterId, EngineProtocolCapability,
    EngineProtocolCapabilitySet, EngineQualificationReport, EngineRuntimeDeviceEvidence,
    ExternalEngineModelSummary, ExternalEngineSnapshot, InferenceAccelerator,
    InferenceArchitecture, InferenceDeployment, InferenceEngineDescriptor, InferenceEngineKind,
    InferenceEngineManifest, InferenceEngineOwnership, InferenceEngineSupportStatus,
    InferenceEngineSupportUnit, InferenceModelFormat, InferencePlatform, InferenceProtocol,
    RuntimeProfileEvidence, RuntimeProfileEvidenceKind,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::openai_protocol_qualification::{
    OpenAiQualificationOptions, map_http_error, qualify_openai_agent_protocol, validate_text,
};
use crate::{
    BoundedEngineHttpClient, EngineInspector, ExternalEngineAdapterError,
    ExternalEngineInspectionFuture, ExternalEngineQualificationFuture,
    ExternalInferenceEngineAdapter, VerifiedEngineTarget, protocol_capability_hash,
    protocol_capability_set,
};

const MLX_LM_API_ROOT: &str = "http://127.0.0.1:8080/v1/";
const MAX_HEALTH_BODY_BYTES: usize = 64 * 1024;
const MAX_MODELS_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_MODELS: usize = 4096;
const MAX_MODEL_ID_BYTES: usize = 1024;

pub fn mlx_lm_agent_protocol_capabilities() -> EngineProtocolCapabilitySet {
    protocol_capability_set([
        EngineProtocolCapability::ModelsList,
        EngineProtocolCapability::ChatCompletionsUnary,
        EngineProtocolCapability::ChatCompletionsStream,
        EngineProtocolCapability::UsagePromptCompletion,
        EngineProtocolCapability::ToolCallsSingle,
    ])
}

pub fn mlx_lm_agent_protocol_capability_hash() -> String {
    protocol_capability_hash(&mlx_lm_agent_protocol_capabilities())
}

/// Adapter for the official `mlx_lm.server` HTTP service.
///
/// The official read-only endpoints do not expose the package version. Discovery therefore marks
/// the version as incomplete; the active Agent qualification extracts the exact MLX-LM version
/// from the official `system_fingerprint` response field before a profile can be authorized.
#[derive(Clone)]
pub struct MlxLmExternalEngineAdapter {
    http: BoundedEngineHttpClient,
    qualification_http: BoundedEngineHttpClient,
}

impl MlxLmExternalEngineAdapter {
    pub fn new() -> Result<Self, ExternalEngineAdapterError> {
        Ok(Self {
            http: BoundedEngineHttpClient::new("mlx-lm-read-only-adapter")
                .map_err(map_http_error)?,
            qualification_http: BoundedEngineHttpClient::with_timeouts(
                "mlx-lm-qualification-adapter",
                Duration::from_secs(2),
                Duration::from_secs(120),
            )
            .map_err(map_http_error)?,
        })
    }

    async fn inspect_inner(
        &self,
        target: &VerifiedEngineTarget,
    ) -> Result<ExternalEngineSnapshot, ExternalEngineAdapterError> {
        if target.adapter_id() != &self.manifest().adapter_id {
            return Err(ExternalEngineAdapterError::AdapterUnavailable);
        }
        let health = self
            .http
            .get_bounded(target, "/health", MAX_HEALTH_BODY_BYTES)
            .await
            .map_err(map_http_error)?;
        validate_health(&health)?;
        let models = parse_models(
            &self
                .http
                .get_bounded(target, "/v1/models", MAX_MODELS_BODY_BYTES)
                .await
                .map_err(map_http_error)?,
        )?;
        Ok(ExternalEngineSnapshot {
            engine: InferenceEngineKind::MlxLm,
            display_name: "用户所有的本机 MLX-LM".to_owned(),
            api_root: target.origin().api_root().as_str().to_owned(),
            version: "qualification-required".to_owned(),
            engine_version_exact: false,
            models,
            model_catalog_complete: true,
        })
    }

    async fn qualify_inner(
        &self,
        target: &VerifiedEngineTarget,
        model_id: &str,
    ) -> Result<EngineQualificationReport, ExternalEngineAdapterError> {
        if target.adapter_id() != &self.manifest().adapter_id {
            return Err(ExternalEngineAdapterError::AdapterUnavailable);
        }
        let observation = qualify_openai_agent_protocol(
            &self.qualification_http,
            target,
            model_id,
            &OpenAiQualificationOptions {
                chat_template_kwargs: Some(serde_json::json!({"enable_thinking": false})),
                ..OpenAiQualificationOptions::default()
            },
        )
        .await?;
        let fingerprint = observation
            .system_fingerprint
            .ok_or(ExternalEngineAdapterError::QualificationFailed)?;
        let observed_engine_version = parse_mlx_lm_version(&fingerprint)?;
        let protocol_capabilities = mlx_lm_agent_protocol_capabilities();
        let protocol_capability_hash = protocol_capability_hash(&protocol_capabilities);
        let deployment_fingerprint = mlx_lm_deployment_fingerprint(&fingerprint, model_id);
        Ok(EngineQualificationReport {
            adapter_id: self.manifest().adapter_id,
            model_id: model_id.to_owned(),
            protocol_capabilities,
            protocol_capability_hash,
            observed_engine_version: Some(observed_engine_version),
            runtime_device_evidence: EngineRuntimeDeviceEvidence::AdapterVariantContract {
                accelerator: InferenceAccelerator::Metal,
            },
            deployment_fingerprint: Some(deployment_fingerprint),
        })
    }
}

impl EngineInspector for MlxLmExternalEngineAdapter {
    fn manifest(&self) -> InferenceEngineManifest {
        InferenceEngineManifest {
            adapter_id: EngineAdapterId {
                engine: InferenceEngineKind::MlxLm,
                variant: "official-http-server".to_owned(),
                contract_revision: ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
            },
            descriptor: InferenceEngineDescriptor {
                kind: InferenceEngineKind::MlxLm,
                display_name: "用户所有的本机 MLX-LM".to_owned(),
                ownership: InferenceEngineOwnership::External,
                deployment: InferenceDeployment::Local,
                protocols: vec![InferenceProtocol::OpenAi],
                platforms: vec![InferencePlatform::MacOs],
                architectures: vec![InferenceArchitecture::Aarch64],
                accelerators: vec![InferenceAccelerator::Metal],
                model_formats: vec![InferenceModelFormat::Mlx],
                managed_lifecycle: false,
            },
            support_units: vec![InferenceEngineSupportUnit {
                platform: InferencePlatform::MacOs,
                architecture: InferenceArchitecture::Aarch64,
                accelerator: InferenceAccelerator::Metal,
                deployment: InferenceDeployment::Local,
                status: InferenceEngineSupportStatus::VerifiedExternal,
                evidence: Some(crate::support_evidence_for(
                    InferenceEngineKind::MlxLm,
                    Some(InferenceEngineSupportStatus::VerifiedExternal),
                )),
            }],
        }
    }

    fn protocol_capability_hash(&self) -> Option<String> {
        Some(mlx_lm_agent_protocol_capability_hash())
    }

    fn default_target(&self) -> Option<VerifiedEngineTarget> {
        VerifiedEngineTarget::external_local(
            "discovery:mlx-lm",
            &self.manifest(),
            MLX_LM_API_ROOT,
            0,
        )
        .ok()
    }

    fn inspect<'a>(
        &'a self,
        target: &'a VerifiedEngineTarget,
    ) -> ExternalEngineInspectionFuture<'a> {
        Box::pin(self.inspect_inner(target))
    }

    fn qualify<'a>(
        &'a self,
        target: &'a VerifiedEngineTarget,
        model_id: &'a str,
    ) -> ExternalEngineQualificationFuture<'a> {
        Box::pin(self.qualify_inner(target, model_id))
    }
}

impl ExternalInferenceEngineAdapter for MlxLmExternalEngineAdapter {}

#[derive(Deserialize)]
struct HealthResponse {
    status: String,
}

#[derive(Deserialize)]
struct ModelsResponse {
    object: String,
    data: Vec<ModelResponse>,
}

#[derive(Deserialize)]
struct ModelResponse {
    id: String,
    object: String,
}

fn validate_health(body: &[u8]) -> Result<(), ExternalEngineAdapterError> {
    let health = serde_json::from_slice::<HealthResponse>(body)
        .map_err(|_| ExternalEngineAdapterError::InvalidResponse)?;
    if health.status != "ok" {
        return Err(ExternalEngineAdapterError::InvalidResponse);
    }
    Ok(())
}

fn parse_models(
    body: &[u8],
) -> Result<Vec<ExternalEngineModelSummary>, ExternalEngineAdapterError> {
    let response = serde_json::from_slice::<ModelsResponse>(body)
        .map_err(|_| ExternalEngineAdapterError::InvalidResponse)?;
    if response.object != "list" || response.data.is_empty() || response.data.len() > MAX_MODELS {
        return Err(ExternalEngineAdapterError::InvalidResponse);
    }
    let mut ids = HashSet::with_capacity(response.data.len());
    let mut models = Vec::with_capacity(response.data.len());
    for model in response.data {
        validate_text(&model.id, MAX_MODEL_ID_BYTES)?;
        if model.object != "model" || !ids.insert(model.id.clone()) {
            return Err(ExternalEngineAdapterError::InvalidResponse);
        }
        let evidence = RuntimeProfileEvidence {
            kind: RuntimeProfileEvidenceKind::CatalogIdentity,
            algorithm: "mlx-lm-catalog-id".to_owned(),
            value: model.id.clone(),
        };
        let digest = Sha256::digest(
            format!(
                "{}\0{}\0{}",
                evidence.algorithm, evidence.value, "contract-v1"
            )
            .as_bytes(),
        )
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
        models.push(ExternalEngineModelSummary {
            name: model.id,
            digest,
            size_bytes: 0,
            format: "mlx".to_owned(),
            family: None,
            parameter_size: None,
            quantization: None,
            evidence,
        });
    }
    models.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(models)
}

fn parse_mlx_lm_version(fingerprint: &str) -> Result<String, ExternalEngineAdapterError> {
    let version = fingerprint
        .split_once('-')
        .map(|(version, _)| version)
        .ok_or(ExternalEngineAdapterError::QualificationFailed)?;
    let segments = version.split('.').collect::<Vec<_>>();
    if !(2..=4).contains(&segments.len())
        || segments
            .iter()
            .any(|segment| segment.is_empty() || !segment.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(ExternalEngineAdapterError::QualificationFailed);
    }
    validate_text(version, 64).map_err(|_| ExternalEngineAdapterError::QualificationFailed)?;
    Ok(version.to_owned())
}

fn mlx_lm_deployment_fingerprint(fingerprint: &str, model_id: &str) -> String {
    Sha256::digest(format!("mlx-lm-deployment-v1\0{fingerprint}\0{model_id}").as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use axum::{
        Json, Router,
        response::IntoResponse,
        routing::{get, post},
    };
    use serde_json::{Value, json};

    use super::*;

    #[tokio::test]
    async fn mlx_lm_adapter_requires_active_fingerprint_identity_and_agent_protocol() {
        let fingerprint = "0.31.3-0.31.2-macOS-15.6-arm64-arm64";
        let app = Router::new()
            .route("/health", get(|| async { Json(json!({"status":"ok"})) }))
            .route(
                "/v1/models",
                get(|| async {
                    Json(json!({
                        "object":"list",
                        "data":[{"id":"mlx-community/Qwen3-0.6B-4bit","object":"model"}]
                    }))
                }),
            )
            .route(
                "/v1/chat/completions",
                post(move |Json(body): Json<Value>| async move {
                    if body.get("stream").and_then(Value::as_bool) == Some(true) {
                        let stream = format!(
                            concat!(
                                "data: {{\"system_fingerprint\":\"{}\",\"choices\":[{{\"delta\":{{\"content\":\"OK\"}},\"finish_reason\":null}}]}}\n\n",
                                "data: {{\"system_fingerprint\":\"{}\",\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\n",
                                "data: {{\"system_fingerprint\":\"{}\",\"choices\":[],\"usage\":{{\"prompt_tokens\":5,\"completion_tokens\":1}}}}\n\n",
                                "data: [DONE]\n\n"
                            ),
                            fingerprint, fingerprint, fingerprint
                        );
                        (
                            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                            stream,
                        )
                            .into_response()
                    } else {
                        Json(json!({
                            "system_fingerprint": fingerprint,
                            "choices": [{
                                "message": {
                                    "tool_calls": [{
                                        "function": {
                                            "name": "hal100_protocol_probe",
                                            "arguments": "{\"value\":\"ok\"}"
                                        }
                                    }]
                                }
                            }],
                            "usage": {"prompt_tokens": 12, "completion_tokens": 4}
                        }))
                        .into_response()
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let address = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("test server");
        });
        let adapter = MlxLmExternalEngineAdapter::new().expect("adapter");
        let manifest = adapter.manifest();
        assert_eq!(
            manifest.support_units[0].status,
            InferenceEngineSupportStatus::VerifiedExternal
        );
        let target = VerifiedEngineTarget::external_local(
            "backend-mlx-lm-test",
            &manifest,
            &format!("http://127.0.0.1:{}/v1/", address.port()),
            1,
        )
        .expect("target");
        let snapshot = adapter.inspect(&target).await.expect("inspection");
        assert!(!snapshot.engine_version_exact);
        assert_eq!(snapshot.models.len(), 1);
        let qualification = adapter
            .qualify(&target, "mlx-community/Qwen3-0.6B-4bit")
            .await
            .expect("qualification");
        assert_eq!(
            qualification.observed_engine_version.as_deref(),
            Some("0.31.3")
        );
        assert_eq!(
            qualification.runtime_device_evidence,
            EngineRuntimeDeviceEvidence::AdapterVariantContract {
                accelerator: InferenceAccelerator::Metal,
            }
        );
        assert_eq!(
            qualification.protocol_capability_hash,
            mlx_lm_agent_protocol_capability_hash()
        );
        assert_eq!(
            qualification
                .deployment_fingerprint
                .as_deref()
                .map(str::len),
            Some(64)
        );
        server.abort();
    }
}
