use std::{
    collections::{BTreeMap, HashSet},
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
    time::Duration,
};

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
    OpenAiQualificationOptions, OpenAiToolArgumentsMode, map_http_error,
    qualify_openai_agent_protocol, validate_text,
};
use crate::{
    BoundedEngineHttpClient, EngineInspector, ExternalEngineAdapterError,
    ExternalEngineInspectionFuture, ExternalEngineQualificationFuture,
    ExternalInferenceEngineAdapter, VerifiedEngineTarget, protocol_capability_hash,
    protocol_capability_set,
};

const MLC_LLM_API_ROOT: &str = "http://127.0.0.1:8000/v1/";
const MAX_MODELS_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_MODELS: usize = 4096;
const MAX_MODEL_ID_BYTES: usize = 1024;
const MAX_DEPLOYMENT_METADATA_BYTES: u64 = 4 * 1024 * 1024;
const MAX_DEPLOYMENT_FILES: usize = 4096;
const MAX_DEPLOYMENT_BYTES: u64 = 128 * 1024 * 1024 * 1024;
const FILE_HASH_BUFFER_BYTES: usize = 1024 * 1024;
const MLC_OWNER: &str = "MLC-LLM";

pub fn mlc_llm_agent_protocol_capabilities() -> EngineProtocolCapabilitySet {
    protocol_capability_set([
        EngineProtocolCapability::ModelsList,
        EngineProtocolCapability::ChatCompletionsUnary,
        EngineProtocolCapability::ChatCompletionsStream,
        EngineProtocolCapability::UsagePromptCompletion,
        EngineProtocolCapability::ToolCallsSingle,
    ])
}

pub fn mlc_llm_agent_protocol_capability_hash() -> String {
    protocol_capability_hash(&mlc_llm_agent_protocol_capabilities())
}

/// Read-only adapter for the official `mlc_llm serve` OpenAI-compatible REST service.
///
/// MLC LLM exposes a model catalog and OpenAI chat endpoints, but the official server does not
/// expose a stable package-version endpoint and currently emits an empty `system_fingerprint`.
/// Formal qualification therefore requires the served model ID to be an absolute local MLC
/// deployment directory. Rust hashes the bounded config, weight manifest, every declared weight
/// shard and tokenizer file into a repeatable deployment fingerprint. Arbitrary catalog IDs and
/// remote `HF://` aliases remain discoverable but cannot become executable runtime profiles.
#[derive(Clone)]
pub struct MlcLlmExternalEngineAdapter {
    accelerator: InferenceAccelerator,
    http: BoundedEngineHttpClient,
    qualification_http: BoundedEngineHttpClient,
}

impl MlcLlmExternalEngineAdapter {
    pub fn for_accelerator(
        accelerator: InferenceAccelerator,
    ) -> Result<Self, ExternalEngineAdapterError> {
        if !matches!(
            accelerator,
            InferenceAccelerator::Metal
                | InferenceAccelerator::Vulkan
                | InferenceAccelerator::Cuda
                | InferenceAccelerator::Rocm
        ) {
            return Err(ExternalEngineAdapterError::InvalidAdapterRegistry);
        }
        Ok(Self {
            accelerator,
            http: BoundedEngineHttpClient::new("mlc-llm-read-only-adapter")
                .map_err(map_http_error)?,
            qualification_http: BoundedEngineHttpClient::with_timeouts(
                "mlc-llm-qualification-adapter",
                Duration::from_secs(2),
                Duration::from_secs(120),
            )
            .map_err(map_http_error)?,
        })
    }

    fn adapter_variant(&self) -> &'static str {
        match self.accelerator {
            InferenceAccelerator::Metal => "official-openai-metal",
            InferenceAccelerator::Vulkan => "official-openai-vulkan",
            InferenceAccelerator::Cuda => "official-openai-cuda",
            InferenceAccelerator::Rocm => "official-openai-rocm",
            _ => unreachable!("constructor restricts MLC LLM accelerator variants"),
        }
    }

    fn accelerator_label(&self) -> &'static str {
        match self.accelerator {
            InferenceAccelerator::Metal => "Metal",
            InferenceAccelerator::Vulkan => "Vulkan",
            InferenceAccelerator::Cuda => "CUDA",
            InferenceAccelerator::Rocm => "ROCm",
            _ => unreachable!("constructor restricts MLC LLM accelerator variants"),
        }
    }

    async fn inspect_inner(
        &self,
        target: &VerifiedEngineTarget,
    ) -> Result<ExternalEngineSnapshot, ExternalEngineAdapterError> {
        if target.adapter_id() != &self.manifest().adapter_id {
            return Err(ExternalEngineAdapterError::AdapterUnavailable);
        }
        // `/v1/models` is the official MLC LLM readiness and served-model endpoint. There is no
        // separate health or package-version endpoint in the official REST surface.
        let models = parse_models(
            &self
                .http
                .get_bounded(target, "/v1/models", MAX_MODELS_BODY_BYTES)
                .await
                .map_err(map_http_error)?,
        )?;
        Ok(ExternalEngineSnapshot {
            engine: InferenceEngineKind::MlcLlm,
            display_name: format!("用户所有的本机 MLC LLM（{}）", self.accelerator_label()),
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
        validate_text(model_id, MAX_MODEL_ID_BYTES)?;
        // MLC's official ChatCompletionRequest supports the standard `tools` and `tool_choice`
        // fields. The shared helper verifies unary + streaming tool calls and Usage with bounded
        // requests. The service fingerprint is intentionally ignored: the official implementation
        // currently emits an empty string and it cannot identify a package or deployment.
        let observation = qualify_openai_agent_protocol(
            &self.qualification_http,
            target,
            model_id,
            &OpenAiQualificationOptions {
                tool_arguments: OpenAiToolArgumentsMode::JsonStringOrObject,
                ..OpenAiQualificationOptions::default()
            },
        )
        .await?;
        let _service_fingerprint = observation.system_fingerprint;
        let deployment_path = model_id.to_owned();
        let deployment_fingerprint =
            tokio::task::spawn_blocking(move || fingerprint_local_mlc_deployment(&deployment_path))
                .await
                .map_err(|_| ExternalEngineAdapterError::QualificationFailed)??;
        let protocol_capabilities = mlc_llm_agent_protocol_capabilities();
        let protocol_capability_hash = protocol_capability_hash(&protocol_capabilities);
        Ok(EngineQualificationReport {
            adapter_id: self.manifest().adapter_id,
            model_id: model_id.to_owned(),
            protocol_capabilities,
            protocol_capability_hash,
            observed_engine_version: None,
            runtime_device_evidence: EngineRuntimeDeviceEvidence::AdapterVariantContract {
                accelerator: self.accelerator,
            },
            deployment_fingerprint: Some(deployment_fingerprint),
        })
    }
}

impl EngineInspector for MlcLlmExternalEngineAdapter {
    fn manifest(&self) -> InferenceEngineManifest {
        InferenceEngineManifest {
            adapter_id: EngineAdapterId {
                engine: InferenceEngineKind::MlcLlm,
                variant: self.adapter_variant().to_owned(),
                contract_revision: ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
            },
            descriptor: InferenceEngineDescriptor {
                kind: InferenceEngineKind::MlcLlm,
                display_name: format!("用户所有的本机 MLC LLM（{}）", self.accelerator_label()),
                ownership: InferenceEngineOwnership::External,
                deployment: InferenceDeployment::Local,
                protocols: vec![InferenceProtocol::OpenAi],
                platforms: mlc_platforms(self.accelerator),
                architectures: mlc_architectures(self.accelerator),
                accelerators: vec![self.accelerator],
                model_formats: vec![InferenceModelFormat::Mlc],
                managed_lifecycle: false,
            },
            support_units: mlc_support_units(self.accelerator),
        }
    }

    fn protocol_capability_hash(&self) -> Option<String> {
        Some(mlc_llm_agent_protocol_capability_hash())
    }

    fn default_target(&self) -> Option<VerifiedEngineTarget> {
        // Automatic discovery can safely bind the Apple-only Metal variant. Windows and Linux
        // expose multiple MLC device contracts at the same conventional port, so those targets
        // require an explicit saved adapter selection instead of guessing from reachability.
        if self.accelerator != InferenceAccelerator::Metal || !cfg!(target_os = "macos") {
            return None;
        }
        VerifiedEngineTarget::external_local(
            "discovery:mlc-llm",
            &self.manifest(),
            MLC_LLM_API_ROOT,
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

impl ExternalInferenceEngineAdapter for MlcLlmExternalEngineAdapter {}

fn support_unit(
    platform: InferencePlatform,
    architecture: InferenceArchitecture,
    accelerator: InferenceAccelerator,
) -> InferenceEngineSupportUnit {
    InferenceEngineSupportUnit {
        platform,
        architecture,
        accelerator,
        deployment: InferenceDeployment::Local,
        status: InferenceEngineSupportStatus::Connected,
        evidence: None,
    }
}

fn mlc_support_units(accelerator: InferenceAccelerator) -> Vec<InferenceEngineSupportUnit> {
    match accelerator {
        InferenceAccelerator::Metal => vec![support_unit(
            InferencePlatform::MacOs,
            InferenceArchitecture::Aarch64,
            accelerator,
        )],
        InferenceAccelerator::Vulkan | InferenceAccelerator::Cuda | InferenceAccelerator::Rocm => {
            vec![
                support_unit(
                    InferencePlatform::Windows,
                    InferenceArchitecture::X86_64,
                    accelerator,
                ),
                support_unit(
                    InferencePlatform::Linux,
                    InferenceArchitecture::Aarch64,
                    accelerator,
                ),
                support_unit(
                    InferencePlatform::Linux,
                    InferenceArchitecture::X86_64,
                    accelerator,
                ),
            ]
        }
        _ => Vec::new(),
    }
}

fn mlc_platforms(accelerator: InferenceAccelerator) -> Vec<InferencePlatform> {
    match accelerator {
        InferenceAccelerator::Metal => vec![InferencePlatform::MacOs],
        InferenceAccelerator::Vulkan | InferenceAccelerator::Cuda | InferenceAccelerator::Rocm => {
            vec![InferencePlatform::Windows, InferencePlatform::Linux]
        }
        _ => Vec::new(),
    }
}

fn mlc_architectures(accelerator: InferenceAccelerator) -> Vec<InferenceArchitecture> {
    match accelerator {
        InferenceAccelerator::Metal => vec![InferenceArchitecture::Aarch64],
        InferenceAccelerator::Vulkan | InferenceAccelerator::Cuda | InferenceAccelerator::Rocm => {
            vec![
                InferenceArchitecture::Aarch64,
                InferenceArchitecture::X86_64,
            ]
        }
        _ => Vec::new(),
    }
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

#[derive(Deserialize)]
struct MlcChatConfigIdentity {
    tokenizer_files: Vec<String>,
    conv_template: MlcConversationIdentity,
}

#[derive(Deserialize)]
struct MlcConversationIdentity {
    system_template: String,
}

#[derive(Deserialize)]
struct MlcTensorCacheIdentity {
    records: Vec<MlcTensorCacheRecord>,
}

#[derive(Deserialize)]
struct MlcTensorCacheRecord {
    #[serde(rename = "dataPath")]
    data_path: String,
    nbytes: u64,
}

fn fingerprint_local_mlc_deployment(model_id: &str) -> Result<String, ExternalEngineAdapterError> {
    validate_text(model_id, MAX_MODEL_ID_BYTES)
        .map_err(|_| ExternalEngineAdapterError::QualificationFailed)?;
    let supplied_root = Path::new(model_id);
    if !supplied_root.is_absolute() {
        return Err(ExternalEngineAdapterError::QualificationFailed);
    }
    let root = supplied_root
        .canonicalize()
        .map_err(|_| ExternalEngineAdapterError::QualificationFailed)?;
    if !root.is_dir() {
        return Err(ExternalEngineAdapterError::QualificationFailed);
    }

    let config_path = resolve_deployment_file(&root, "mlc-chat-config.json")?;
    let config = read_bounded_deployment_metadata(&config_path)?;
    let config_identity = serde_json::from_slice::<MlcChatConfigIdentity>(&config)
        .map_err(|_| ExternalEngineAdapterError::QualificationFailed)?;
    if config_identity.tokenizer_files.is_empty()
        || config_identity.tokenizer_files.len() > MAX_DEPLOYMENT_FILES
        || !valid_mlc_system_template(&config_identity.conv_template.system_template)
        || config_identity
            .conv_template
            .system_template
            .matches("{function_string}")
            .count()
            != 1
    {
        return Err(ExternalEngineAdapterError::QualificationFailed);
    }

    let manifest_name = if root.join("tensor-cache.json").is_file() {
        "tensor-cache.json"
    } else if root.join("ndarray-cache.json").is_file() {
        "ndarray-cache.json"
    } else {
        return Err(ExternalEngineAdapterError::QualificationFailed);
    };
    let manifest_path = resolve_deployment_file(&root, manifest_name)?;
    let manifest = read_bounded_deployment_metadata(&manifest_path)?;
    let tensor_cache = serde_json::from_slice::<MlcTensorCacheIdentity>(&manifest)
        .map_err(|_| ExternalEngineAdapterError::QualificationFailed)?;
    if tensor_cache.records.is_empty() || tensor_cache.records.len() > MAX_DEPLOYMENT_FILES {
        return Err(ExternalEngineAdapterError::QualificationFailed);
    }

    let mut files = BTreeMap::<String, Option<u64>>::new();
    files.insert("mlc-chat-config.json".to_owned(), Some(config.len() as u64));
    files.insert(manifest_name.to_owned(), Some(manifest.len() as u64));
    for record in tensor_cache.records {
        if record.nbytes == 0
            || files
                .insert(record.data_path, Some(record.nbytes))
                .is_some()
        {
            return Err(ExternalEngineAdapterError::QualificationFailed);
        }
    }
    let mut tokenizer_names = HashSet::new();
    for tokenizer in config_identity.tokenizer_files {
        if !tokenizer_names.insert(tokenizer.clone()) || files.insert(tokenizer, None).is_some() {
            return Err(ExternalEngineAdapterError::QualificationFailed);
        }
    }
    if files.len() > MAX_DEPLOYMENT_FILES {
        return Err(ExternalEngineAdapterError::QualificationFailed);
    }

    let mut hasher = Sha256::new();
    hasher.update(b"mlc-llm-local-deployment-v2\0");
    let mut total_bytes = 0_u64;
    let mut buffer = vec![0_u8; FILE_HASH_BUFFER_BYTES];
    for (relative_name, expected_size) in files {
        let path = resolve_deployment_file(&root, &relative_name)?;
        let mut file =
            File::open(path).map_err(|_| ExternalEngineAdapterError::QualificationFailed)?;
        let metadata = file
            .metadata()
            .map_err(|_| ExternalEngineAdapterError::QualificationFailed)?;
        if !metadata.is_file() || expected_size.is_some_and(|expected| expected != metadata.len()) {
            return Err(ExternalEngineAdapterError::QualificationFailed);
        }
        let initial_modified = metadata.modified().ok();
        total_bytes = total_bytes
            .checked_add(metadata.len())
            .filter(|total| *total <= MAX_DEPLOYMENT_BYTES)
            .ok_or(ExternalEngineAdapterError::QualificationFailed)?;
        hasher.update((relative_name.len() as u64).to_le_bytes());
        hasher.update(relative_name.as_bytes());
        hasher.update(metadata.len().to_le_bytes());
        let mut hashed_bytes = 0_u64;
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|_| ExternalEngineAdapterError::QualificationFailed)?;
            if read == 0 {
                break;
            }
            hashed_bytes = hashed_bytes
                .checked_add(read as u64)
                .ok_or(ExternalEngineAdapterError::QualificationFailed)?;
            hasher.update(&buffer[..read]);
        }
        if hashed_bytes != metadata.len() {
            return Err(ExternalEngineAdapterError::QualificationFailed);
        }
        let final_metadata = file
            .metadata()
            .map_err(|_| ExternalEngineAdapterError::QualificationFailed)?;
        if final_metadata.len() != metadata.len()
            || final_metadata.modified().ok() != initial_modified
        {
            return Err(ExternalEngineAdapterError::QualificationFailed);
        }
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn valid_mlc_system_template(template: &str) -> bool {
    !template.trim().is_empty()
        && template.len() <= 64 * 1024
        && !template.chars().any(|character| {
            character == '\0'
                || (character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
        })
}

fn read_bounded_deployment_metadata(path: &Path) -> Result<Vec<u8>, ExternalEngineAdapterError> {
    let mut file = File::open(path).map_err(|_| ExternalEngineAdapterError::QualificationFailed)?;
    let metadata = file
        .metadata()
        .map_err(|_| ExternalEngineAdapterError::QualificationFailed)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_DEPLOYMENT_METADATA_BYTES
    {
        return Err(ExternalEngineAdapterError::QualificationFailed);
    }
    let initial_modified = metadata.modified().ok();
    let mut body = Vec::with_capacity(metadata.len() as usize);
    file.by_ref()
        .take(MAX_DEPLOYMENT_METADATA_BYTES + 1)
        .read_to_end(&mut body)
        .map_err(|_| ExternalEngineAdapterError::QualificationFailed)?;
    let final_metadata = file
        .metadata()
        .map_err(|_| ExternalEngineAdapterError::QualificationFailed)?;
    if body.len() as u64 != metadata.len()
        || final_metadata.len() != metadata.len()
        || final_metadata.modified().ok() != initial_modified
    {
        return Err(ExternalEngineAdapterError::QualificationFailed);
    }
    Ok(body)
}

fn resolve_deployment_file(
    root: &Path,
    relative_name: &str,
) -> Result<PathBuf, ExternalEngineAdapterError> {
    validate_text(relative_name, MAX_MODEL_ID_BYTES)
        .map_err(|_| ExternalEngineAdapterError::QualificationFailed)?;
    let relative = Path::new(relative_name);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ExternalEngineAdapterError::QualificationFailed);
    }
    let resolved = root
        .join(relative)
        .canonicalize()
        .map_err(|_| ExternalEngineAdapterError::QualificationFailed)?;
    if !resolved.starts_with(root) {
        return Err(ExternalEngineAdapterError::QualificationFailed);
    }
    Ok(resolved)
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
                .is_some_and(|owner| owner != MLC_OWNER)
            || !ids.insert(model.id.clone())
        {
            return Err(ExternalEngineAdapterError::InvalidResponse);
        }
        let evidence = RuntimeProfileEvidence {
            kind: RuntimeProfileEvidenceKind::CatalogIdentity,
            algorithm: "mlc-llm-catalog-id".to_owned(),
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
            format: "mlc".to_owned(),
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
    use std::fs;

    use axum::{
        Json, Router,
        response::IntoResponse,
        routing::{get, post},
    };
    use serde_json::{Value, json};
    use uuid::Uuid;

    use super::*;

    struct TestMlcDeployment {
        path: PathBuf,
    }

    impl TestMlcDeployment {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("hal100-mlc-deployment-{}", Uuid::new_v4().simple()));
            fs::create_dir_all(&path).expect("create MLC test deployment");
            fs::write(
                path.join("mlc-chat-config.json"),
                br#"{"tokenizer_files":["tokenizer.json"],"conv_template":{"system_template":"{system_message}\n{function_string}"}}"#,
            )
            .expect("write MLC config");
            fs::write(
                path.join("tensor-cache.json"),
                br#"{"records":[{"dataPath":"params_shard_0.bin","nbytes":4}]}"#,
            )
            .expect("write MLC tensor cache");
            fs::write(path.join("params_shard_0.bin"), [1, 2, 3, 4]).expect("write MLC shard");
            fs::write(path.join("tokenizer.json"), br#"{"version":"1"}"#)
                .expect("write MLC tokenizer");
            Self { path }
        }

        fn model_id(&self) -> String {
            self.path.to_string_lossy().into_owned()
        }
    }

    impl Drop for TestMlcDeployment {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[tokio::test]
    async fn mlc_llm_adapter_binds_protocol_to_a_local_deployment_without_faking_version() {
        let deployment = TestMlcDeployment::new();
        let model_id = deployment.model_id();
        let catalog_model_id = model_id.clone();
        let app = Router::new()
            .route(
                "/v1/models",
                get(move || {
                    let model_id = catalog_model_id.clone();
                    async move {
                    Json(json!({
                        "object":"list",
                        "data":[{"id":model_id,"object":"model","owned_by":"MLC-LLM"}]
                    }))
                    }
                }),
            )
            .route(
                "/v1/chat/completions",
                post(|Json(body): Json<Value>| async move {
                    if body.get("stream").and_then(Value::as_bool) == Some(true) {
                        let stream = concat!(
                            "data: {\"choices\":[{\"delta\":{\"content\":\"OK\"},\"finish_reason\":null}],\"system_fingerprint\":\"\"}\n\n",
                            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"system_fingerprint\":\"\"}\n\n",
                            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":1},\"system_fingerprint\":\"\"}\n\n",
                            "data: [DONE]\n\n"
                        );
                        (
                            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                            stream,
                        )
                            .into_response()
                    } else {
                        Json(json!({
                            "system_fingerprint":"",
                            "choices": [{
                                "message": {
                                    "tool_calls": [{
                                        "function": {
                                            "name": "hal100_protocol_probe",
                                            "arguments": {"value":"ok"}
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
        let adapter = MlcLlmExternalEngineAdapter::for_accelerator(InferenceAccelerator::Metal)
            .expect("Metal adapter");
        let manifest = adapter.manifest();
        assert!(
            manifest
                .support_units
                .iter()
                .all(|unit| unit.status == InferenceEngineSupportStatus::Connected)
        );
        let target = VerifiedEngineTarget::external_local(
            "backend-mlc-llm-test",
            &manifest,
            &format!("http://127.0.0.1:{}/v1/", address.port()),
            1,
        )
        .expect("target");
        let snapshot = adapter.inspect(&target).await.expect("inspection");
        assert_eq!(snapshot.engine, InferenceEngineKind::MlcLlm);
        assert!(!snapshot.engine_version_exact);
        assert_eq!(snapshot.models[0].format, "mlc");
        assert_eq!(
            snapshot.models[0].evidence.kind,
            RuntimeProfileEvidenceKind::CatalogIdentity
        );
        let qualification = adapter
            .qualify(&target, &model_id)
            .await
            .expect("qualification");
        assert!(qualification.observed_engine_version.is_none());
        assert_eq!(
            qualification.runtime_device_evidence,
            EngineRuntimeDeviceEvidence::AdapterVariantContract {
                accelerator: InferenceAccelerator::Metal,
            }
        );
        assert_eq!(
            qualification
                .deployment_fingerprint
                .as_deref()
                .map(str::len),
            Some(64)
        );
        assert_eq!(
            qualification.protocol_capability_hash,
            mlc_llm_agent_protocol_capability_hash()
        );
        server.abort();
    }

    #[test]
    fn mlc_device_variants_partition_the_support_matrix_without_overlap() {
        let variants = [
            (InferenceAccelerator::Metal, "official-openai-metal", 1),
            (InferenceAccelerator::Vulkan, "official-openai-vulkan", 3),
            (InferenceAccelerator::Cuda, "official-openai-cuda", 3),
            (InferenceAccelerator::Rocm, "official-openai-rocm", 3),
        ];
        let mut cells = HashSet::new();
        for (accelerator, expected_variant, expected_cells) in variants {
            let manifest = MlcLlmExternalEngineAdapter::for_accelerator(accelerator)
                .expect("supported MLC accelerator")
                .manifest();
            assert_eq!(manifest.adapter_id.variant, expected_variant);
            assert_eq!(manifest.descriptor.accelerators, vec![accelerator]);
            assert_eq!(manifest.support_units.len(), expected_cells);
            assert!(manifest.support_units.iter().all(|unit| {
                unit.accelerator == accelerator
                    && cells.insert((
                        unit.platform,
                        unit.architecture,
                        unit.accelerator,
                        unit.deployment,
                    ))
            }));
        }
        assert_eq!(cells.len(), 10);
        assert!(matches!(
            MlcLlmExternalEngineAdapter::for_accelerator(InferenceAccelerator::Cpu),
            Err(ExternalEngineAdapterError::InvalidAdapterRegistry)
        ));
    }

    #[test]
    fn local_deployment_fingerprint_changes_with_weight_content() {
        let deployment = TestMlcDeployment::new();
        let model_id = deployment.model_id();
        let before = fingerprint_local_mlc_deployment(&model_id).expect("initial fingerprint");
        fs::write(deployment.path.join("params_shard_0.bin"), [4, 3, 2, 1])
            .expect("mutate test shard");
        let after = fingerprint_local_mlc_deployment(&model_id).expect("changed fingerprint");
        assert_ne!(before, after);
    }

    #[test]
    fn local_deployment_fingerprint_rejects_relative_and_traversing_files() {
        assert!(matches!(
            fingerprint_local_mlc_deployment("HF://mlc-ai/example"),
            Err(ExternalEngineAdapterError::QualificationFailed)
        ));
        let deployment = TestMlcDeployment::new();
        fs::write(
            deployment.path.join("tensor-cache.json"),
            br#"{"records":[{"dataPath":"../outside.bin","nbytes":4}]}"#,
        )
        .expect("write traversal manifest");
        assert!(matches!(
            fingerprint_local_mlc_deployment(&deployment.model_id()),
            Err(ExternalEngineAdapterError::QualificationFailed)
        ));
    }

    #[test]
    fn local_deployment_fingerprint_rejects_duplicate_or_mismatched_weight_records() {
        let duplicate = TestMlcDeployment::new();
        fs::write(
            duplicate.path.join("tensor-cache.json"),
            br#"{"records":[{"dataPath":"params_shard_0.bin","nbytes":4},{"dataPath":"params_shard_0.bin","nbytes":4}]}"#,
        )
        .expect("write duplicate manifest");
        assert!(matches!(
            fingerprint_local_mlc_deployment(&duplicate.model_id()),
            Err(ExternalEngineAdapterError::QualificationFailed)
        ));

        let mismatched = TestMlcDeployment::new();
        fs::write(
            mismatched.path.join("tensor-cache.json"),
            br#"{"records":[{"dataPath":"params_shard_0.bin","nbytes":5}]}"#,
        )
        .expect("write mismatched manifest");
        assert!(matches!(
            fingerprint_local_mlc_deployment(&mismatched.model_id()),
            Err(ExternalEngineAdapterError::QualificationFailed)
        ));
    }

    #[test]
    fn local_deployment_fingerprint_requires_one_function_template_placeholder() {
        let deployment = TestMlcDeployment::new();
        fs::write(
            deployment.path.join("mlc-chat-config.json"),
            br#"{"tokenizer_files":["tokenizer.json"],"conv_template":{"system_template":"{system_message}"}}"#,
        )
        .expect("write config without function placeholder");
        assert!(matches!(
            fingerprint_local_mlc_deployment(&deployment.model_id()),
            Err(ExternalEngineAdapterError::QualificationFailed)
        ));
    }

    #[test]
    fn rejects_non_mlc_catalog_owners() {
        let body =
            br#"{"object":"list","data":[{"id":"model","object":"model","owned_by":"other"}]}"#;
        assert!(matches!(
            parse_models(body),
            Err(ExternalEngineAdapterError::InvalidResponse)
        ));
    }
}
