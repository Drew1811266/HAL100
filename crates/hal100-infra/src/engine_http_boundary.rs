use std::time::Duration;

use futures_util::StreamExt;
use reqwest::{Client, redirect::Policy};
use serde::Serialize;
use thiserror::Error;

use crate::{EngineTargetError, VerifiedEngineTarget};

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum EngineHttpError {
    #[error("无法创建推理引擎HTTP客户端")]
    Client,
    #[error("推理引擎目标无效")]
    Target,
    #[error("推理引擎当前不可达")]
    Unreachable,
    #[error("推理引擎返回了无效或过大的响应")]
    InvalidResponse,
}

#[derive(Clone)]
pub struct BoundedEngineHttpClient {
    client: Client,
}

impl BoundedEngineHttpClient {
    pub fn new(user_agent_component: &str) -> Result<Self, EngineHttpError> {
        Self::with_timeouts(
            user_agent_component,
            Duration::from_millis(350),
            Duration::from_secs(1),
        )
    }

    pub(crate) fn with_timeouts(
        user_agent_component: &str,
        connect_timeout: Duration,
        total_timeout: Duration,
    ) -> Result<Self, EngineHttpError> {
        if user_agent_component.is_empty()
            || user_agent_component.len() > 64
            || !user_agent_component
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        {
            return Err(EngineHttpError::Client);
        }
        if connect_timeout.is_zero()
            || connect_timeout > Duration::from_secs(10)
            || total_timeout < connect_timeout
            || total_timeout > Duration::from_secs(120)
        {
            return Err(EngineHttpError::Client);
        }
        let client = Client::builder()
            .connect_timeout(connect_timeout)
            .timeout(total_timeout)
            .redirect(Policy::none())
            .no_proxy()
            .user_agent(format!(
                "HAL100/{} {user_agent_component}",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .map_err(|_| EngineHttpError::Client)?;
        Ok(Self { client })
    }

    pub async fn get_bounded(
        &self,
        target: &VerifiedEngineTarget,
        absolute_path: &str,
        limit: usize,
    ) -> Result<Vec<u8>, EngineHttpError> {
        if limit == 0 {
            return Err(EngineHttpError::InvalidResponse);
        }
        let endpoint = target
            .origin()
            .endpoint(absolute_path)
            .map_err(map_target_error)?;
        let response = target
            .authenticate(self.client.get(endpoint))
            .send()
            .await
            .map_err(|_| EngineHttpError::Unreachable)?;
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|length| length > limit as u64)
        {
            return Err(EngineHttpError::InvalidResponse);
        }
        let mut stream = response.bytes_stream();
        let mut body = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| EngineHttpError::InvalidResponse)?;
            if body.len().saturating_add(chunk.len()) > limit {
                return Err(EngineHttpError::InvalidResponse);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }

    pub async fn post_json_bounded<T: Serialize + ?Sized>(
        &self,
        target: &VerifiedEngineTarget,
        absolute_path: &str,
        body: &T,
        limit: usize,
    ) -> Result<Vec<u8>, EngineHttpError> {
        if limit == 0 {
            return Err(EngineHttpError::InvalidResponse);
        }
        let endpoint = target
            .origin()
            .endpoint(absolute_path)
            .map_err(map_target_error)?;
        let response = target
            .authenticate(self.client.post(endpoint).json(body))
            .send()
            .await
            .map_err(|_| EngineHttpError::Unreachable)?;
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|length| length > limit as u64)
        {
            return Err(EngineHttpError::InvalidResponse);
        }
        let mut stream = response.bytes_stream();
        let mut response_body = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| EngineHttpError::InvalidResponse)?;
            if response_body.len().saturating_add(chunk.len()) > limit {
                return Err(EngineHttpError::InvalidResponse);
            }
            response_body.extend_from_slice(&chunk);
        }
        Ok(response_body)
    }
}

fn map_target_error(_: EngineTargetError) -> EngineHttpError {
    EngineHttpError::Target
}

#[cfg(test)]
mod tests {
    use axum::{Router, http::HeaderMap, response::Redirect, routing::get};
    use hal100_protocol::{
        EngineAdapterId, InferenceAccelerator, InferenceArchitecture, InferenceDeployment,
        InferenceEngineDescriptor, InferenceEngineKind, InferenceEngineManifest,
        InferenceEngineOwnership, InferenceEngineSupportStatus, InferenceEngineSupportUnit,
        InferenceModelFormat, InferencePlatform, InferenceProtocol,
    };

    use super::*;

    fn manifest() -> InferenceEngineManifest {
        InferenceEngineManifest {
            adapter_id: EngineAdapterId {
                engine: InferenceEngineKind::Ollama,
                variant: "test".to_owned(),
                contract_revision: hal100_protocol::ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
            },
            descriptor: InferenceEngineDescriptor {
                kind: InferenceEngineKind::Ollama,
                display_name: "test".to_owned(),
                ownership: InferenceEngineOwnership::External,
                deployment: InferenceDeployment::Local,
                protocols: vec![InferenceProtocol::OpenAi],
                platforms: vec![InferencePlatform::MacOs],
                architectures: vec![InferenceArchitecture::Aarch64],
                accelerators: vec![InferenceAccelerator::Cpu],
                model_formats: vec![InferenceModelFormat::Gguf],
                managed_lifecycle: false,
            },
            support_units: vec![InferenceEngineSupportUnit {
                platform: InferencePlatform::MacOs,
                architecture: InferenceArchitecture::Aarch64,
                accelerator: InferenceAccelerator::Cpu,
                deployment: InferenceDeployment::Local,
                status: InferenceEngineSupportStatus::VerifiedExternal,
                evidence: Some(crate::support_evidence_for(
                    InferenceEngineKind::Ollama,
                    Some(InferenceEngineSupportStatus::VerifiedExternal),
                )),
            }],
        }
    }

    async fn target_for(app: Router) -> (VerifiedEngineTarget, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let address = listener.local_addr().expect("address");
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("test server");
        });
        let target = VerifiedEngineTarget::external_local(
            "http-boundary-test",
            &manifest(),
            &format!("http://127.0.0.1:{}/v1/", address.port()),
            1,
        )
        .expect("target");
        (target, task)
    }

    #[tokio::test]
    async fn bounds_body_and_refuses_cross_path_redirects() {
        let app = Router::new()
            .route("/ok", get(|| async { "bounded" }))
            .route("/large", get(|| async { "x".repeat(64) }))
            .route("/redirect", get(|| async { Redirect::temporary("/ok") }));
        let (target, task) = target_for(app).await;
        let client = BoundedEngineHttpClient::new("test-engine").expect("client");

        assert_eq!(
            client.get_bounded(&target, "/ok", 16).await.expect("body"),
            b"bounded"
        );
        assert_eq!(
            client.get_bounded(&target, "/large", 8).await.err(),
            Some(EngineHttpError::InvalidResponse)
        );
        assert_eq!(
            client.get_bounded(&target, "/redirect", 16).await.err(),
            Some(EngineHttpError::InvalidResponse)
        );
        task.abort();
    }

    #[tokio::test]
    async fn injects_target_auth_only_at_the_validated_same_origin_boundary() {
        let app = Router::new().route(
            "/authorized",
            get(|headers: HeaderMap| async move {
                if headers
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    == Some("Bearer bounded-secret")
                {
                    "authorized"
                } else {
                    "missing"
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let address = listener.local_addr().expect("address");
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("test server");
        });
        let target = VerifiedEngineTarget::external_local_with_auth(
            "http-auth-test",
            &manifest(),
            &format!("http://127.0.0.1:{}/v1/", address.port()),
            1,
            crate::EngineRequestAuth::bearer("bounded-secret").expect("auth"),
        )
        .expect("target");
        let client = BoundedEngineHttpClient::new("test-engine").expect("client");

        assert_eq!(
            client
                .get_bounded(&target, "/authorized", 32)
                .await
                .expect("authorized body"),
            b"authorized"
        );
        task.abort();
    }
}
