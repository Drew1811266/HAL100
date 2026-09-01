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

const TENSORRT_LLM_API_ROOT: &str = "http://127.0.0.1:8000/v1/";
const MAX_VERSION_BODY_BYTES: usize = 64 * 1024;
const MAX_HEALTH_BODY_BYTES: usize = 64 * 1024;
const MAX_MODELS_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_MODELS: usize = 4096;
const MAX_MODEL_ID_BYTES: usize = 1024;

pub fn tensorrt_llm_agent_protocol_capabilities() -> EngineProtocolCapabilitySet {
    protocol_capability_set([
        EngineProtocolCapability::ModelsList,
        EngineProtocolCapability::ChatCompletionsUnary,
        EngineProtocolCapability::ChatCompletionsStream,
        EngineProtocolCapability::UsagePromptCompletion,
        EngineProtocolCapability::ToolCallsSingle,
    ])
}

pub fn tensorrt_llm_agent_protocol_capability_hash() -> String {
    protocol_capability_hash(&tensorrt_llm_agent_protocol_capabilities())
}

/// Read-only adapter for NVIDIA's official `trtllm-serve` OpenAI-compatible server.
///
/// The official server publishes an exact TensorRT-LLM package version, a readiness endpoint and
/// a single served-model catalog. The first HAL100 variant represents text-generation servers
/// started from a Hugging Face checkpoint; prebuilt TensorRT engine directories remain a separate
/// evidence variant because the OpenAI catalog alone cannot prove their build configuration.
#[derive(Clone)]
pub struct TensorRtLlmExternalEngineAdapter {
    http: BoundedEngineHttpClient,
    qualification_http: BoundedEngineHttpClient,
}

impl TensorRtLlmExternalEngineAdapter {
    pub fn new() -> Result<Self, ExternalEngineAdapterError> {
        Ok(Self {
            http: BoundedEngineHttpClient::new("tensorrt-llm-read-only-adapter")
                .map_err(map_http_error)?,
            qualification_http: BoundedEngineHttpClient::with_timeouts(
                "tensorrt-llm-qualification-adapter",
                Duration::from_secs(2),
                Duration::from_secs(120),
            )
            .map_err(map_http_error)?,
        })
    }

    async fn read_version(
        &self,
        target: &VerifiedEngineTarget,
    ) -> Result<String, ExternalEngineAdapterError> {
        parse_version(
            &self
                .http
                .get_bounded(target, "/version", MAX_VERSION_BODY_BYTES)
                .await
                .map_err(map_http_error)?,
        )
    }

    async fn inspect_inner(
        &self,
        target: &VerifiedEngineTarget,
    ) -> Result<ExternalEngineSnapshot, ExternalEngineAdapterError> {
        if target.adapter_id() != &self.manifest().adapter_id {
            return Err(ExternalEngineAdapterError::AdapterUnavailable);
        }
        let version = self.read_version(target).await?;
        // The official endpoint intentionally returns an empty success response. Only HTTP
        // success is evidence; response content is not interpreted or upgraded to device proof.
        self.http
            .get_bounded(target, "/health", MAX_HEALTH_BODY_BYTES)
            .await
            .map_err(map_http_error)?;
        let models = parse_models(
            &self
                .http
                .get_bounded(target, "/v1/models", MAX_MODELS_BODY_BYTES)
                .await
                .map_err(map_http_error)?,
        )?;
        Ok(ExternalEngineSnapshot {
            engine: InferenceEngineKind::TensorRtLlm,
            display_name: "用户所有的本机 TensorRT-LLM".to_owned(),
            api_root: target.origin().api_root().as_str().to_owned(),
            version,
            engine_version_exact: true,
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
        validate_text(model_id, MAX_MODEL_ID_BYTES)?;
        let version = self.read_version(target).await?;
        qualify_openai_agent_protocol(
            &self.qualification_http,
            target,
            model_id,
            &OpenAiQualificationOptions::default(),
        )
        .await?;
        let protocol_capabilities = tensorrt_llm_agent_protocol_capabilities();
        let protocol_capability_hash = protocol_capability_hash(&protocol_capabilities);
        Ok(EngineQualificationReport {
            adapter_id: self.manifest().adapter_id,
            model_id: model_id.to_owned(),
            protocol_capabilities,
            protocol_capability_hash,
            observed_engine_version: Some(version),
            runtime_device_evidence: EngineRuntimeDeviceEvidence::AdapterVariantContract {
                accelerator: InferenceAccelerator::Cuda,
            },
            deployment_fingerprint: None,
        })
    }
}

impl EngineInspector for TensorRtLlmExternalEngineAdapter {
    fn manifest(&self) -> InferenceEngineManifest {
        InferenceEngineManifest {
            adapter_id: EngineAdapterId {
                engine: InferenceEngineKind::TensorRtLlm,
                variant: "trtllm-serve-openai-server".to_owned(),
                contract_revision: ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
            },
            descriptor: InferenceEngineDescriptor {
                kind: InferenceEngineKind::TensorRtLlm,
                display_name: "用户所有的本机 TensorRT-LLM".to_owned(),
                ownership: InferenceEngineOwnership::External,
                deployment: InferenceDeployment::Local,
                protocols: vec![InferenceProtocol::OpenAi],
                platforms: vec![InferencePlatform::Linux],
                architectures: vec![
                    InferenceArchitecture::X86_64,
                    InferenceArchitecture::Aarch64,
                ],
                accelerators: vec![InferenceAccelerator::Cuda],
                model_formats: vec![InferenceModelFormat::Safetensors],
                managed_lifecycle: false,
            },
            support_units: vec![
                support_unit(InferenceArchitecture::X86_64),
                support_unit(InferenceArchitecture::Aarch64),
            ],
        }
    }

    fn protocol_capability_hash(&self) -> Option<String> {
        Some(tensorrt_llm_agent_protocol_capability_hash())
    }

    fn default_target(&self) -> Option<VerifiedEngineTarget> {
        VerifiedEngineTarget::external_local(
            "discovery:tensorrt-llm",
            &self.manifest(),
            TENSORRT_LLM_API_ROOT,
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

impl ExternalInferenceEngineAdapter for TensorRtLlmExternalEngineAdapter {}

fn support_unit(architecture: InferenceArchitecture) -> InferenceEngineSupportUnit {
    InferenceEngineSupportUnit {
        platform: InferencePlatform::Linux,
        architecture,
        accelerator: InferenceAccelerator::Cuda,
        deployment: InferenceDeployment::Local,
        status: InferenceEngineSupportStatus::Connected,
        evidence: None,
    }
}

#[derive(Deserialize)]
struct VersionResponse {
    version: String,
}

fn parse_version(body: &[u8]) -> Result<String, ExternalEngineAdapterError> {
    let version = serde_json::from_slice::<VersionResponse>(body)
        .map_err(|_| ExternalEngineAdapterError::InvalidResponse)?
        .version;
    validate_text(&version, 128)?;
    Ok(version)
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
            algorithm: "tensorrt-llm-model-id".to_owned(),
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
            format: "safetensors".to_owned(),
            family: None,
            parameter_size: None,
            quantization: None,
            evidence,
        });
    }
    models.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(models)
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
    async fn tensorrt_llm_adapter_proves_version_health_catalog_and_openai_protocol() {
        let app = Router::new()
            .route(
                "/version",
                get(|| async { Json(json!({"version":"1.2.0"})) }),
            )
            .route("/health", get(|| async { axum::http::StatusCode::OK }))
            .route(
                "/v1/models",
                get(|| async {
                    Json(json!({
                        "object":"list",
                        "data":[{"id":"TinyLlama/TinyLlama-1.1B-Chat-v1.0","object":"model"}]
                    }))
                }),
            )
            .route(
                "/v1/chat/completions",
                post(|Json(body): Json<Value>| async move {
                    if body.get("stream").and_then(Value::as_bool) == Some(true) {
                        let stream = concat!(
                            "data: {\"choices\":[{\"delta\":{\"content\":\"OK\"},\"finish_reason\":null}]}\n\n",
                            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":1}}\n\n",
                            "data: [DONE]\n\n"
                        );
                        (
                            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                            stream,
                        )
                            .into_response()
                    } else {
                        Json(json!({
                            "choices": [{"message": {"tool_calls": [{"function": {
                                "name": "hal100_protocol_probe",
                                "arguments": "{\"value\":\"ok\"}"
                            }}]}}],
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
        let adapter = TensorRtLlmExternalEngineAdapter::new().expect("adapter");
        let manifest = adapter.manifest();
        assert!(
            manifest
                .support_units
                .iter()
                .all(|unit| unit.status == InferenceEngineSupportStatus::Connected)
        );
        let target = VerifiedEngineTarget::external_local(
            "backend-tensorrt-llm-test",
            &manifest,
            &format!("http://127.0.0.1:{}/v1/", address.port()),
            1,
        )
        .expect("target");
        let snapshot = adapter.inspect(&target).await.expect("inspection");
        assert_eq!(snapshot.engine, InferenceEngineKind::TensorRtLlm);
        assert_eq!(snapshot.version, "1.2.0");
        assert!(snapshot.engine_version_exact);
        assert_eq!(snapshot.models[0].format, "safetensors");
        let qualification = adapter
            .qualify(&target, "TinyLlama/TinyLlama-1.1B-Chat-v1.0")
            .await
            .expect("qualification");
        assert_eq!(
            qualification.observed_engine_version.as_deref(),
            Some("1.2.0")
        );
        assert_eq!(
            qualification.runtime_device_evidence,
            EngineRuntimeDeviceEvidence::AdapterVariantContract {
                accelerator: InferenceAccelerator::Cuda,
            }
        );
        assert_eq!(
            qualification.protocol_capability_hash,
            tensorrt_llm_agent_protocol_capability_hash()
        );
        server.abort();
    }

    #[test]
    fn rejects_invalid_version_or_model_shapes() {
        assert!(matches!(
            parse_version(br#"{"version":""}"#),
            Err(ExternalEngineAdapterError::InvalidResponse)
        ));
        assert!(matches!(
            parse_models(br#"{"object":"list","data":[{"id":"model","object":"engine"}]}"#),
            Err(ExternalEngineAdapterError::InvalidResponse)
        ));
    }
}
