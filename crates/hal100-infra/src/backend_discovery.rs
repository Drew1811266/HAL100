use std::time::Duration;

use futures_util::{StreamExt, future::join_all};
use hal100_protocol::{BackendKind, LocalBackendCandidate, LocalBackendDiscovery};
use reqwest::{Client, Url};
use serde_json::Value;
use thiserror::Error;

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
    OllamaVersion,
    OpenAiModels,
}

impl LocalBackendDiscoveryService {
    pub fn new() -> Result<Self, BackendDiscoveryError> {
        let targets = vec![
            DiscoveryTarget::new(
                BackendKind::ExternalOllama,
                "本机 Ollama",
                "http://127.0.0.1:11434/v1/",
                "http://127.0.0.1:11434/api/version",
                "Ollama 默认回环端口返回版本信息",
                DiscoveryProtocol::OllamaVersion,
            )?,
            DiscoveryTarget::new(
                BackendKind::ExternalVllm,
                "本机 vLLM 候选",
                "http://127.0.0.1:8000/v1/",
                "http://127.0.0.1:8000/v1/models",
                "vLLM 常用回环端口返回 OpenAI Models",
                DiscoveryProtocol::OpenAiModels,
            )?,
            DiscoveryTarget::new(
                BackendKind::ExternalLlamaCpp,
                "本机 llama.cpp Server 候选",
                "http://127.0.0.1:8080/v1/",
                "http://127.0.0.1:8080/v1/models",
                "llama.cpp Server 常用回环端口返回 OpenAI Models",
                DiscoveryProtocol::OpenAiModels,
            )?,
        ];
        Self::with_targets(targets)
    }

    fn with_targets(targets: Vec<DiscoveryTarget>) -> Result<Self, BackendDiscoveryError> {
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
        Ok(Self { client, targets })
    }

    pub async fn discover(&self) -> LocalBackendDiscovery {
        let checked_targets = self.targets.len();
        let candidates = join_all(
            self.targets
                .iter()
                .cloned()
                .map(|target| self.probe(target)),
        )
        .await
        .into_iter()
        .flatten()
        .collect();
        LocalBackendDiscovery {
            candidates,
            checked_targets,
        }
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
        DiscoveryProtocol::OllamaVersion => value
            .get("version")
            .and_then(Value::as_str)
            .filter(|version| !version.is_empty() && version.len() <= 128)
            .map(|version| Some(version.to_owned())),
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
                "/api/version",
                get(|State(attempts): State<Arc<AtomicUsize>>| async move {
                    if attempts.fetch_add(1, Ordering::AcqRel) == 0 {
                        return StatusCode::SERVICE_UNAVAILABLE.into_response();
                    }
                    Json(json!({"version":"0.12.0-test"})).into_response()
                }),
            )
            .with_state(attempts.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let address = listener.local_addr().expect("address");
        let task = tokio::spawn(async move { axum::serve(listener, app).await });
        let target = DiscoveryTarget::new(
            BackendKind::ExternalOllama,
            "测试 Ollama",
            "http://127.0.0.1:11434/v1/",
            &format!("http://127.0.0.1:{}/api/version", address.port()),
            "测试证据",
            DiscoveryProtocol::OllamaVersion,
        )
        .expect("target");
        let service = LocalBackendDiscoveryService::with_targets(vec![target]).expect("service");

        let result = service.discover().await;
        assert_eq!(result.checked_targets, 1);
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].version.as_deref(), Some("0.12.0-test"));
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
}
