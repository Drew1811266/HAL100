use std::{sync::Arc, time::Duration};

use futures_util::{StreamExt, future::join_all};
use hal100_protocol::{
    BackendKind, EngineAdapterId, ExternalEngineSnapshot, HostCapabilitySnapshot,
    InferenceEngineCapability, InferenceEngineKind, LocalBackendCandidate, LocalBackendDiscovery,
};
use reqwest::{Client, Url};
use serde_json::Value;
use thiserror::Error;

use crate::ExternalInferenceEngineRegistry;
use crate::recommendation_for;

const DISCOVERY_ATTEMPTS: usize = 2;
const DISCOVERY_RETRY_DELAY: Duration = Duration::from_millis(25);

#[derive(Debug, Error)]
pub enum BackendDiscoveryError {
    #[error("无法创建本机后端探测客户端")]
    Client,
    #[error("内置后端探测地址无效")]
    InvalidTarget,
}

pub struct LocalBackendDiscoveryService {
    client: Client,
    targets: Vec<DiscoveryTarget>,
    external_engines: Arc<ExternalInferenceEngineRegistry>,
}

#[derive(Clone)]
struct DiscoveryTarget {
    kind: BackendKind,
    display_name: &'static str,
    api_root: &'static str,
    probe_url: Url,
    evidence: &'static str,
    protocol: DiscoveryProtocol,
}

#[derive(Clone, Copy)]
enum DiscoveryProtocol {
    OpenAiModels,
}

impl LocalBackendDiscoveryService {
    pub fn new() -> Result<Self, BackendDiscoveryError> {
        // Keep the default discovery path aligned with the production composition root: any
        // reviewed acceptance promotions must be visible to discovery as well as runtime
        // profile management. This constructor is also used by embedding callers that do not
        // inject a registry explicitly, so falling back to the unpromoted registry here would
        // make capability and saved-profile views disagree.
        let registry =
            ExternalInferenceEngineRegistry::standard_with_reviewed_acceptance_promotions()
                .map_err(|_| BackendDiscoveryError::Client)?;
        Self::with_registry(Arc::new(registry))
    }

    pub fn with_registry(
        external_engines: Arc<ExternalInferenceEngineRegistry>,
    ) -> Result<Self, BackendDiscoveryError> {
        let targets = vec![DiscoveryTarget::new(
            BackendKind::ExternalLlamaCpp,
            "本机 llama.cpp Server 候选",
            "http://127.0.0.1:8080/v1/",
            "http://127.0.0.1:8080/v1/models",
            "llama.cpp Server 常用回环端口返回 OpenAI Models",
            DiscoveryProtocol::OpenAiModels,
        )?];
        Self::with_parts(targets, external_engines)
    }

    #[cfg(test)]
    fn with_targets(targets: Vec<DiscoveryTarget>) -> Result<Self, BackendDiscoveryError> {
        let registry = ExternalInferenceEngineRegistry::new(Vec::new())
            .map_err(|_| BackendDiscoveryError::Client)?;
        Self::with_parts(targets, Arc::new(registry))
    }

    fn with_parts(
        targets: Vec<DiscoveryTarget>,
        external_engines: Arc<ExternalInferenceEngineRegistry>,
    ) -> Result<Self, BackendDiscoveryError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_millis(350))
            .timeout(Duration::from_secs(1))
            .no_proxy()
            .user_agent(concat!(
                "HAL100/",
                env!("CARGO_PKG_VERSION"),
                " local-backend-discovery"
            ))
            .build()
            .map_err(|_| BackendDiscoveryError::Client)?;
        Ok(Self {
            client,
            targets,
            external_engines,
        })
    }

    pub async fn discover(&self) -> LocalBackendDiscovery {
        let external_adapters = self.external_engines.adapters();
        let checked_targets = self.targets.len()
            + external_adapters
                .iter()
                .filter(|adapter| adapter.default_target().is_some())
                .count();
        let target_candidates = join_all(
            self.targets
                .iter()
                .cloned()
                .map(|target| self.probe(target)),
        )
        .await
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        let external_observations = join_all(external_adapters.iter().map(|engine| async move {
            let adapter_id = engine.manifest().adapter_id;
            let target = engine.default_target()?;
            let snapshot = engine.inspect(&target).await.ok()?;
            (snapshot.engine == adapter_id.engine).then_some((adapter_id, snapshot))
        }))
        .await
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        let mut candidates = target_candidates;
        candidates.extend(
            external_observations
                .iter()
                .filter_map(|(adapter_id, snapshot)| external_candidate(adapter_id, snapshot)),
        );
        let external_engines = external_observations
            .into_iter()
            .map(|(_, snapshot)| snapshot)
            .collect();
        LocalBackendDiscovery {
            candidates,
            checked_targets,
            external_engines,
        }
    }

    pub async fn engine_capabilities(
        &self,
        host: &HostCapabilitySnapshot,
    ) -> Vec<InferenceEngineCapability> {
        let external_adapters = self.external_engines.adapters();
        let inspections = join_all(external_adapters.iter().map(|engine| async move {
            let target = engine.default_target()?;
            engine.inspect(&target).await.ok()
        }))
        .await;
        external_adapters
            .iter()
            .zip(inspections)
            .map(|(engine, inspection)| {
                let manifest = engine.manifest();
                let compatibility = manifest.compatibility_with(host);
                let descriptor = manifest.descriptor;
                let recommendation = recommendation_for(
                    &compatibility,
                    descriptor.ownership,
                    inspection.as_ref().map_or(0, |_| 1),
                );
                let support_evidence =
                    crate::support_evidence_for(descriptor.kind, compatibility.support_status);
                InferenceEngineCapability {
                    descriptor,
                    compatibility,
                    external_runtimes: inspection.into_iter().collect(),
                    support_evidence: Some(support_evidence),
                    recommendation: Some(recommendation),
                }
            })
            .collect()
    }

    async fn probe(&self, target: DiscoveryTarget) -> Option<LocalBackendCandidate> {
        for attempt in 0..DISCOVERY_ATTEMPTS {
            if let Ok(response) = self.client.get(target.probe_url.clone()).send().await
                && response.status().is_success()
                && let Some(body) = read_bounded_body(response, 1024 * 1024).await
                && let Some(version) = validate_discovery_body(target.protocol, &body)
            {
                return Some(LocalBackendCandidate {
                    kind: target.kind,
                    engine: target.kind.engine_kind(),
                    adapter_variant: target.kind.default_adapter_variant().map(str::to_owned),
                    display_name: target.display_name.to_owned(),
                    api_root: target.api_root.to_owned(),
                    evidence: target.evidence.to_owned(),
                    version,
                });
            }
            if attempt + 1 < DISCOVERY_ATTEMPTS {
                tokio::time::sleep(DISCOVERY_RETRY_DELAY).await;
            }
        }
        None
    }
}

fn external_candidate(
    adapter_id: &EngineAdapterId,
    snapshot: &ExternalEngineSnapshot,
) -> Option<LocalBackendCandidate> {
    if snapshot.engine != adapter_id.engine {
        return None;
    }
    let (kind, evidence) = match snapshot.engine {
        InferenceEngineKind::Ollama => (
            BackendKind::ExternalOllama,
            if snapshot.model_catalog_complete {
                "Ollama 固定回环 API 返回版本与模型目录"
            } else {
                "Ollama 固定回环 API 返回版本；模型目录当前不可用"
            },
        ),
        InferenceEngineKind::Vllm => (
            BackendKind::ExternalVllm,
            "vLLM 固定回环 API 返回版本、健康状态与模型目录",
        ),
        InferenceEngineKind::MlxLm => (
            BackendKind::ExternalOpenAi,
            "MLX-LM 官方固定回环 API 返回健康状态与模型目录",
        ),
        InferenceEngineKind::MlcLlm => (
            BackendKind::ExternalOpenAi,
            "MLC LLM 官方固定回环 API 返回模型目录",
        ),
        InferenceEngineKind::OpenVino => (
            BackendKind::ExternalOpenAi,
            "OpenVINO Model Server 固定回环 API 返回版本、健康状态与模型目录",
        ),
        InferenceEngineKind::Sglang => (
            BackendKind::ExternalOpenAi,
            "SGLang 官方固定回环 API 返回版本、健康状态与模型目录",
        ),
        InferenceEngineKind::LmDeploy => (
            BackendKind::ExternalOpenAi,
            "LMDeploy 官方固定回环 API 返回健康状态与模型目录",
        ),
        InferenceEngineKind::TensorRtLlm => (
            BackendKind::ExternalOpenAi,
            "TensorRT-LLM 官方固定回环 API 返回版本、健康状态与模型目录",
        ),
        _ => return None,
    };
    Some(LocalBackendCandidate {
        kind,
        engine: Some(adapter_id.engine),
        adapter_variant: Some(adapter_id.variant.clone()),
        display_name: snapshot.display_name.clone(),
        api_root: snapshot.api_root.clone(),
        evidence: evidence.to_owned(),
        version: snapshot
            .engine_version_exact
            .then(|| snapshot.version.clone()),
    })
}

impl DiscoveryTarget {
    fn new(
        kind: BackendKind,
        display_name: &'static str,
        api_root: &'static str,
        probe_url: &str,
        evidence: &'static str,
        protocol: DiscoveryProtocol,
    ) -> Result<Self, BackendDiscoveryError> {
        let probe_url = Url::parse(probe_url).map_err(|_| BackendDiscoveryError::InvalidTarget)?;
        let api_root_url =
            Url::parse(api_root).map_err(|_| BackendDiscoveryError::InvalidTarget)?;
        if !is_safe_loopback_http(&probe_url) || !is_safe_loopback_http(&api_root_url) {
            return Err(BackendDiscoveryError::InvalidTarget);
        }
        Ok(Self {
            kind,
            display_name,
            api_root,
            probe_url,
            evidence,
            protocol,
        })
    }
}

fn is_safe_loopback_http(url: &Url) -> bool {
    url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
}

fn validate_discovery_body(protocol: DiscoveryProtocol, body: &[u8]) -> Option<Option<String>> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    match protocol {
        DiscoveryProtocol::OpenAiModels => {
            value.get("data").and_then(Value::as_array).map(|_| None)
        }
    }
}

async fn read_bounded_body(response: reqwest::Response, limit: usize) -> Option<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return None;
    }
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.ok()?;
        if body.len().saturating_add(chunk.len()) > limit {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    Some(body)
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use axum::{
        Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get,
    };
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn discovers_only_valid_fixed_loopback_candidates_and_retries_once() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route(
                "/v1/models",
                get(|State(attempts): State<Arc<AtomicUsize>>| async move {
                    if attempts.fetch_add(1, Ordering::AcqRel) == 0 {
                        return StatusCode::SERVICE_UNAVAILABLE.into_response();
                    }
                    Json(json!({"data":[]})).into_response()
                }),
            )
            .with_state(attempts.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let address = listener.local_addr().expect("address");
        let task = tokio::spawn(async move { axum::serve(listener, app).await });
        let target = DiscoveryTarget::new(
            BackendKind::ExternalVllm,
            "测试 vLLM",
            "http://127.0.0.1:8000/v1/",
            &format!("http://127.0.0.1:{}/v1/models", address.port()),
            "测试证据",
            DiscoveryProtocol::OpenAiModels,
        )
        .expect("target");
        let service = LocalBackendDiscoveryService::with_targets(vec![target]).expect("service");

        let result = service.discover().await;
        assert_eq!(result.checked_targets, 1);
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].version, None);
        assert_eq!(attempts.load(Ordering::Acquire), 2);
        task.abort();
    }

    #[test]
    fn rejects_non_loopback_or_credential_bearing_probe_targets() {
        assert!(
            DiscoveryTarget::new(
                BackendKind::ExternalOpenAi,
                "unsafe",
                "https://example.com/v1/",
                "https://example.com/v1/models",
                "unsafe",
                DiscoveryProtocol::OpenAiModels,
            )
            .is_err()
        );
        assert!(
            DiscoveryTarget::new(
                BackendKind::ExternalOpenAi,
                "unsafe",
                "http://127.0.0.1/v1/",
                "http://user:secret@127.0.0.1/v1/models",
                "unsafe",
                DiscoveryProtocol::OpenAiModels,
            )
            .is_err()
        );
    }

    #[test]
    fn external_candidate_preserves_the_exact_adapter_variant() {
        let adapter_id = EngineAdapterId {
            engine: InferenceEngineKind::MlcLlm,
            variant: "official-openai-metal".to_owned(),
            contract_revision: hal100_protocol::ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
        };
        let snapshot = ExternalEngineSnapshot {
            engine: InferenceEngineKind::MlcLlm,
            display_name: "MLC LLM（Metal）".to_owned(),
            api_root: "http://127.0.0.1:8000/v1/".to_owned(),
            version: "qualification-required".to_owned(),
            engine_version_exact: false,
            models: Vec::new(),
            model_catalog_complete: true,
        };
        let candidate = external_candidate(&adapter_id, &snapshot).expect("MLC candidate");
        assert_eq!(candidate.engine, Some(InferenceEngineKind::MlcLlm));
        assert_eq!(
            candidate.adapter_variant.as_deref(),
            Some("official-openai-metal")
        );

        let mismatched = EngineAdapterId {
            engine: InferenceEngineKind::Vllm,
            ..adapter_id
        };
        assert!(external_candidate(&mismatched, &snapshot).is_none());
    }
}
