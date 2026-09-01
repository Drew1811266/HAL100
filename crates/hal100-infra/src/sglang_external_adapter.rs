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

const SGLANG_API_ROOT: &str = "http://127.0.0.1:30000/v1/";
const MAX_SERVER_INFO_BODY_BYTES: usize = 256 * 1024;
const MAX_HEALTH_BODY_BYTES: usize = 64 * 1024;
const MAX_MODELS_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_MODELS: usize = 4096;
const MAX_MODEL_ID_BYTES: usize = 1024;

pub fn sglang_agent_protocol_capabilities() -> EngineProtocolCapabilitySet {
    protocol_capability_set([
        EngineProtocolCapability::ModelsList,
        EngineProtocolCapability::ChatCompletionsUnary,
        EngineProtocolCapability::ChatCompletionsStream,
        EngineProtocolCapability::UsagePromptCompletion,
        EngineProtocolCapability::ToolCallsSingle,
    ])
}

pub fn sglang_agent_protocol_capability_hash() -> String {
    protocol_capability_hash(&sglang_agent_protocol_capabilities())
}

/// Read-only adapter for the official SGLang OpenAI-compatible server.
///
/// SGLang publishes `/server_info` with the package version, `/health` for readiness and
/// `/v1/models` for the served model identity. The server's OpenAI surface is qualified with the
/// shared bounded Agent probe. Support cells stay `Connected` until a Linux/CUDA deployment is
/// exercised and its model/weight revision is recorded.
#[derive(Clone)]
pub struct SglangExternalEngineAdapter {
    http: BoundedEngineHttpClient,
    qualification_http: BoundedEngineHttpClient,
}

impl SglangExternalEngineAdapter {
    pub fn new() -> Result<Self, ExternalEngineAdapterError> {
        Ok(Self {
            http: BoundedEngineHttpClient::new("sglang-read-only-adapter")
                .map_err(map_http_error)?,
            qualification_http: BoundedEngineHttpClient::with_timeouts(
                "sglang-qualification-adapter",
                Duration::from_secs(2),
                Duration::from_secs(120),
            )
            .map_err(map_http_error)?,
        })
    }

    async fn read_server_info(
        &self,
        target: &VerifiedEngineTarget,
    ) -> Result<SglangServerInfo, ExternalEngineAdapterError> {
        parse_server_info(
            &self
                .http
                .get_bounded(target, "/server_info", MAX_SERVER_INFO_BODY_BYTES)
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
        let server_info = self.read_server_info(target).await?;
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
            engine: InferenceEngineKind::Sglang,
            display_name: "用户所有的本机 SGLang".to_owned(),
            api_root: target.origin().api_root().as_str().to_owned(),
            version: server_info.version,
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
        let server_info = self.read_server_info(target).await?;
        qualify_openai_agent_protocol(
            &self.qualification_http,
            target,
            model_id,
            &OpenAiQualificationOptions::default(),
        )
        .await?;
        let protocol_capabilities = sglang_agent_protocol_capabilities();
        let protocol_capability_hash = protocol_capability_hash(&protocol_capabilities);
        Ok(EngineQualificationReport {
            adapter_id: self.manifest().adapter_id,
            model_id: model_id.to_owned(),
            protocol_capabilities,
            protocol_capability_hash,
            observed_engine_version: Some(server_info.version),
            runtime_device_evidence: EngineRuntimeDeviceEvidence::AdapterVariantContract {
                accelerator: InferenceAccelerator::Cuda,
            },
            deployment_fingerprint: None,
        })
    }
}

impl EngineInspector for SglangExternalEngineAdapter {
    fn manifest(&self) -> InferenceEngineManifest {
        InferenceEngineManifest {
            adapter_id: EngineAdapterId {
                engine: InferenceEngineKind::Sglang,
                variant: "official-openai-server".to_owned(),
                contract_revision: ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
            },
            descriptor: InferenceEngineDescriptor {
                kind: InferenceEngineKind::Sglang,
                display_name: "用户所有的本机 SGLang".to_owned(),
                ownership: InferenceEngineOwnership::External,
                deployment: InferenceDeployment::Local,
                protocols: vec![InferenceProtocol::OpenAi],
                platforms: vec![InferencePlatform::Linux],
                architectures: vec![InferenceArchitecture::X86_64],
                accelerators: vec![InferenceAccelerator::Cuda],
                model_formats: vec![InferenceModelFormat::Safetensors],
                managed_lifecycle: false,
            },
            support_units: vec![InferenceEngineSupportUnit {
                platform: InferencePlatform::Linux,
                architecture: InferenceArchitecture::X86_64,
                accelerator: InferenceAccelerator::Cuda,
                deployment: InferenceDeployment::Local,
                status: InferenceEngineSupportStatus::Connected,
                evidence: None,
            }],
        }
    }

    fn protocol_capability_hash(&self) -> Option<String> {
        Some(sglang_agent_protocol_capability_hash())
    }

    fn default_target(&self) -> Option<VerifiedEngineTarget> {
        VerifiedEngineTarget::external_local(
            "discovery:sglang",
            &self.manifest(),
            SGLANG_API_ROOT,
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

impl ExternalInferenceEngineAdapter for SglangExternalEngineAdapter {}

#[derive(Deserialize)]
struct SglangServerInfo {
    version: String,
}

fn parse_server_info(body: &[u8]) -> Result<SglangServerInfo, ExternalEngineAdapterError> {
    let info = serde_json::from_slice::<SglangServerInfo>(body)
        .map_err(|_| ExternalEngineAdapterError::InvalidResponse)?;
    validate_text(&info.version, 128)?;
    Ok(info)
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
    #[serde(default)]
    owned_by: Option<String>,
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
        if model.object != "model"
            || model
                .owned_by
                .as_deref()
                .is_some_and(|owner| !matches!(owner, "sglang" | "SGLang"))
            || !ids.insert(model.id.clone())
        {
            return Err(ExternalEngineAdapterError::InvalidResponse);
        }
        let evidence = RuntimeProfileEvidence {
            kind: RuntimeProfileEvidenceKind::CatalogIdentity,
            algorithm: "sglang-model-id".to_owned(),
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
    async fn sglang_adapter_proves_server_info_catalog_and_openai_protocol() {
        let app = Router::new()
            .route(
                "/server_info",
                get(|| async { Json(json!({"version":"0.5.6"})) }),
            )
            .route("/health", get(|| async { "" }))
            .route(
                "/v1/models",
                get(|| async {
                    Json(json!({
                        "object":"list",
                        "data":[{"id":"Qwen/Qwen2.5-7B-Instruct","object":"model","owned_by":"sglang"}]
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
        let adapter = SglangExternalEngineAdapter::new().expect("adapter");
        let manifest = adapter.manifest();
        assert_eq!(
            manifest.support_units[0].status,
            InferenceEngineSupportStatus::Connected
        );
        let target = VerifiedEngineTarget::external_local(
            "backend-sglang-test",
            &manifest,
            &format!("http://127.0.0.1:{}/v1/", address.port()),
            1,
        )
        .expect("target");
        let snapshot = adapter.inspect(&target).await.expect("inspection");
        assert_eq!(snapshot.engine, InferenceEngineKind::Sglang);
        assert_eq!(snapshot.version, "0.5.6");
        assert!(snapshot.engine_version_exact);
        assert_eq!(snapshot.models[0].format, "safetensors");
        let qualification = adapter
            .qualify(&target, "Qwen/Qwen2.5-7B-Instruct")
            .await
            .expect("qualification");
        assert_eq!(
            qualification.observed_engine_version.as_deref(),
            Some("0.5.6")
        );
        assert_eq!(
            qualification.runtime_device_evidence,
            EngineRuntimeDeviceEvidence::AdapterVariantContract {
                accelerator: InferenceAccelerator::Cuda,
            }
        );
        assert_eq!(
            qualification.protocol_capability_hash,
            sglang_agent_protocol_capability_hash()
        );
        server.abort();
    }

    #[test]
    fn rejects_invalid_server_info_or_model_owner() {
        assert!(matches!(
            parse_server_info(br#"{"version":""}"#),
            Err(ExternalEngineAdapterError::InvalidResponse)
        ));
        assert!(matches!(
            parse_models(
                br#"{"object":"list","data":[{"id":"model","object":"model","owned_by":"other"}]}"#
            ),
            Err(ExternalEngineAdapterError::InvalidResponse)
        ));
    }
}
