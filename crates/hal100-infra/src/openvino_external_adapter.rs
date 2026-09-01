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

const OPENVINO_API_ROOT: &str = "http://127.0.0.1:8000/v1/";
const MAX_METADATA_BODY_BYTES: usize = 64 * 1024;
const MAX_HEALTH_BODY_BYTES: usize = 64 * 1024;
const MAX_MODELS_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_MODELS: usize = 4096;
const MAX_MODEL_ID_BYTES: usize = 1024;
const OVMS_NAME: &str = "OpenVINO Model Server";

pub fn openvino_agent_protocol_capabilities() -> EngineProtocolCapabilitySet {
    protocol_capability_set([
        EngineProtocolCapability::ModelsList,
        EngineProtocolCapability::ChatCompletionsUnary,
        EngineProtocolCapability::ChatCompletionsStream,
        EngineProtocolCapability::UsagePromptCompletion,
        EngineProtocolCapability::ToolCallsSingle,
    ])
}

pub fn openvino_agent_protocol_capability_hash() -> String {
    protocol_capability_hash(&openvino_agent_protocol_capabilities())
}

/// Read-only adapter for one explicit OpenVINO Model Server (OVMS) target-device contract.
///
/// OVMS exposes a KServe metadata/health surface alongside its OpenAI-compatible GenAI
/// endpoints. The adapter keeps the two observations separate: `/v2` proves the server identity
/// and version, while `/v1/models` and the shared qualification probe prove the served model and
/// the bounded Agent protocol. OVMS does not expose `target_device` through its HTTP metadata,
/// models, config-status or metrics surfaces, so CPU, Intel GPU and Intel NPU are separate adapter
/// variants. Each variant has one accelerator per host coordinate and must be selected explicitly;
/// the native host probe and reviewed live run then provide the independent hardware evidence.
#[derive(Clone)]
pub struct OpenVinoExternalEngineAdapter {
    http: BoundedEngineHttpClient,
    qualification_http: BoundedEngineHttpClient,
    accelerator: InferenceAccelerator,
}

impl OpenVinoExternalEngineAdapter {
    pub fn new() -> Result<Self, ExternalEngineAdapterError> {
        Self::for_accelerator(InferenceAccelerator::Cpu)
    }

    pub fn for_accelerator(
        accelerator: InferenceAccelerator,
    ) -> Result<Self, ExternalEngineAdapterError> {
        if !matches!(
            accelerator,
            InferenceAccelerator::Cpu
                | InferenceAccelerator::IntelGpu
                | InferenceAccelerator::IntelNpu
        ) {
            return Err(ExternalEngineAdapterError::InvalidAdapterRegistry);
        }
        Ok(Self {
            http: BoundedEngineHttpClient::new("openvino-read-only-adapter")
                .map_err(map_http_error)?,
            qualification_http: BoundedEngineHttpClient::with_timeouts(
                "openvino-qualification-adapter",
                Duration::from_secs(2),
                Duration::from_secs(120),
            )
            .map_err(map_http_error)?,
            accelerator,
        })
    }

    fn adapter_variant(&self) -> &'static str {
        match self.accelerator {
            InferenceAccelerator::Cpu => "ovms-openai-cpu",
            InferenceAccelerator::IntelGpu => "ovms-openai-intel-gpu",
            InferenceAccelerator::IntelNpu => "ovms-openai-intel-npu",
            _ => unreachable!("constructor restricts OVMS accelerators"),
        }
    }

    fn target_device_label(&self) -> &'static str {
        match self.accelerator {
            InferenceAccelerator::Cpu => "CPU",
            InferenceAccelerator::IntelGpu => "Intel GPU",
            InferenceAccelerator::IntelNpu => "Intel NPU",
            _ => unreachable!("constructor restricts OVMS accelerators"),
        }
    }

    async fn read_metadata(
        &self,
        target: &VerifiedEngineTarget,
    ) -> Result<OpenVinoMetadata, ExternalEngineAdapterError> {
        parse_metadata(
            &self
                .http
                .get_bounded(target, "/v2", MAX_METADATA_BODY_BYTES)
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
        let metadata = self.read_metadata(target).await?;
        // KServe liveness/readiness endpoints intentionally carry their signal in the status
        // code. The bounded body read still enforces the same redirect and size boundary.
        self.http
            .get_bounded(target, "/v2/health/live", MAX_HEALTH_BODY_BYTES)
            .await
            .map_err(map_http_error)?;
        self.http
            .get_bounded(target, "/v2/health/ready", MAX_HEALTH_BODY_BYTES)
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
            engine: InferenceEngineKind::OpenVino,
            display_name: format!(
                "用户所有的本机 OpenVINO Model Server（{}）",
                self.target_device_label()
            ),
            api_root: target.origin().api_root().as_str().to_owned(),
            version: metadata.version,
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
        let metadata = self.read_metadata(target).await?;
        qualify_openai_agent_protocol(
            &self.qualification_http,
            target,
            model_id,
            &OpenAiQualificationOptions::default(),
        )
        .await?;
        let protocol_capabilities = openvino_agent_protocol_capabilities();
        let protocol_capability_hash = protocol_capability_hash(&protocol_capabilities);
        Ok(EngineQualificationReport {
            adapter_id: self.manifest().adapter_id,
            model_id: model_id.to_owned(),
            protocol_capabilities,
            protocol_capability_hash,
            observed_engine_version: Some(metadata.version),
            runtime_device_evidence: EngineRuntimeDeviceEvidence::AdapterVariantContract {
                accelerator: self.accelerator,
            },
            deployment_fingerprint: None,
        })
    }
}

impl EngineInspector for OpenVinoExternalEngineAdapter {
    fn manifest(&self) -> InferenceEngineManifest {
        InferenceEngineManifest {
            adapter_id: EngineAdapterId {
                engine: InferenceEngineKind::OpenVino,
                variant: self.adapter_variant().to_owned(),
                contract_revision: ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
            },
            descriptor: InferenceEngineDescriptor {
                kind: InferenceEngineKind::OpenVino,
                display_name: format!(
                    "用户所有的本机 OpenVINO Model Server（{}）",
                    self.target_device_label()
                ),
                ownership: InferenceEngineOwnership::External,
                deployment: InferenceDeployment::Local,
                protocols: vec![InferenceProtocol::OpenAi],
                platforms: vec![InferencePlatform::Windows, InferencePlatform::Linux],
                architectures: vec![InferenceArchitecture::X86_64],
                accelerators: vec![self.accelerator],
                model_formats: vec![InferenceModelFormat::OpenVino],
                managed_lifecycle: false,
            },
            support_units: vec![
                support_unit(InferencePlatform::Windows, self.accelerator),
                support_unit(InferencePlatform::Linux, self.accelerator),
            ],
        }
    }

    fn protocol_capability_hash(&self) -> Option<String> {
        Some(openvino_agent_protocol_capability_hash())
    }

    fn default_target(&self) -> Option<VerifiedEngineTarget> {
        if self.accelerator != InferenceAccelerator::Cpu {
            return None;
        }
        VerifiedEngineTarget::external_local(
            "discovery:openvino-ovms",
            &self.manifest(),
            OPENVINO_API_ROOT,
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

impl ExternalInferenceEngineAdapter for OpenVinoExternalEngineAdapter {}

fn support_unit(
    platform: InferencePlatform,
    accelerator: InferenceAccelerator,
) -> InferenceEngineSupportUnit {
    InferenceEngineSupportUnit {
        platform,
        architecture: InferenceArchitecture::X86_64,
        accelerator,
        deployment: InferenceDeployment::Local,
        status: InferenceEngineSupportStatus::Connected,
        evidence: None,
    }
}

#[derive(Debug, Deserialize)]
struct OpenVinoMetadata {
    name: String,
    version: String,
}

fn parse_metadata(body: &[u8]) -> Result<OpenVinoMetadata, ExternalEngineAdapterError> {
    let metadata = serde_json::from_slice::<OpenVinoMetadata>(body)
        .map_err(|_| ExternalEngineAdapterError::InvalidResponse)?;
    if metadata.name != OVMS_NAME {
        return Err(ExternalEngineAdapterError::InvalidResponse);
    }
    validate_text(&metadata.version, 128)?;
    Ok(metadata)
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
                .is_some_and(|owner| !matches!(owner, "OVMS" | OVMS_NAME))
            || !ids.insert(model.id.clone())
        {
            return Err(ExternalEngineAdapterError::InvalidResponse);
        }
        let evidence = RuntimeProfileEvidence {
            kind: RuntimeProfileEvidenceKind::CatalogIdentity,
            algorithm: "openvino-model-id".to_owned(),
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
            format: "openVino".to_owned(),
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
    async fn ovms_adapter_proves_metadata_catalog_and_openai_protocol() {
        let app = Router::new()
            .route(
                "/v2",
                get(|| async {
                    Json(json!({"name":"OpenVINO Model Server","version":"2026.0.0"}))
                }),
            )
            .route("/v2/health/live", get(|| async { "" }))
            .route("/v2/health/ready", get(|| async { "" }))
            .route(
                "/v1/models",
                get(|| async {
                    Json(json!({
                        "object":"list",
                        "data":[{"id":"OpenVINO/Qwen3-8B-int4-ov","object":"model","owned_by":"OVMS"}]
                    }))
                }),
            )
            .route(
                "/v1/chat/completions",
                post(|Json(body): Json<Value>| async move {
                    if body.get("stream").and_then(Value::as_bool) == Some(true) {
                        let stream = concat!(
                            "data: {\"choices\":[{\"delta\":{\"content\":\"OK\"},\"finish_reason\":null}] }\n\n",
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
        let adapter = OpenVinoExternalEngineAdapter::new().expect("adapter");
        let manifest = adapter.manifest();
        assert_eq!(manifest.adapter_id.variant, "ovms-openai-cpu");
        assert_eq!(
            manifest.descriptor.accelerators,
            vec![InferenceAccelerator::Cpu]
        );
        assert_eq!(manifest.support_units.len(), 2);
        assert!(
            manifest
                .support_units
                .iter()
                .all(|unit| { unit.status == InferenceEngineSupportStatus::Connected })
        );
        let target = VerifiedEngineTarget::external_local(
            "backend-openvino-test",
            &manifest,
            &format!("http://127.0.0.1:{}/v1/", address.port()),
            1,
        )
        .expect("target");
        let snapshot = adapter.inspect(&target).await.expect("inspection");
        assert_eq!(snapshot.engine, InferenceEngineKind::OpenVino);
        assert_eq!(snapshot.version, "2026.0.0");
        assert!(snapshot.engine_version_exact);
        assert_eq!(snapshot.models[0].format, "openVino");
        assert_eq!(
            snapshot.models[0].evidence.kind,
            RuntimeProfileEvidenceKind::CatalogIdentity
        );
        let qualification = adapter
            .qualify(&target, "OpenVINO/Qwen3-8B-int4-ov")
            .await
            .expect("qualification");
        assert_eq!(
            qualification.observed_engine_version.as_deref(),
            Some("2026.0.0")
        );
        assert_eq!(
            qualification.runtime_device_evidence,
            EngineRuntimeDeviceEvidence::AdapterVariantContract {
                accelerator: InferenceAccelerator::Cpu,
            }
        );
        assert_eq!(
            qualification.protocol_capability_hash,
            openvino_agent_protocol_capability_hash()
        );
        server.abort();
    }

    #[test]
    fn ovms_target_device_variants_are_distinct_and_single_accelerator() {
        for (accelerator, variant) in [
            (InferenceAccelerator::Cpu, "ovms-openai-cpu"),
            (InferenceAccelerator::IntelGpu, "ovms-openai-intel-gpu"),
            (InferenceAccelerator::IntelNpu, "ovms-openai-intel-npu"),
        ] {
            let adapter = OpenVinoExternalEngineAdapter::for_accelerator(accelerator)
                .expect("supported OVMS target device");
            let manifest = adapter.manifest();
            assert_eq!(manifest.adapter_id.variant, variant);
            assert_eq!(manifest.descriptor.accelerators, vec![accelerator]);
            assert_eq!(manifest.support_units.len(), 2);
            assert!(
                manifest
                    .support_units
                    .iter()
                    .all(|unit| unit.accelerator == accelerator)
            );
        }
        assert!(
            OpenVinoExternalEngineAdapter::for_accelerator(InferenceAccelerator::Cuda).is_err()
        );
    }

    #[test]
    fn rejects_non_ovms_metadata_or_model_owner() {
        assert!(matches!(
            parse_metadata(br#"{"name":"other","version":"1"}"#),
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
