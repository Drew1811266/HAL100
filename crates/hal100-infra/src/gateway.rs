use std::{
    collections::{HashMap, HashSet},
    fmt,
    future::Future,
    io,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    pin::Pin,
    sync::{
        Arc, RwLock,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    task::{Context, Poll},
    time::{Duration, Instant, SystemTime},
};

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Extension, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{Next, from_fn_with_state},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{Stream, StreamExt};
use hal100_protocol::{
    AnthropicError, AnthropicErrorEnvelope, AnthropicMessagesRequestMetadata, AnthropicUsage,
    BackendProbeResult, BackendProbeStatus, GatewayHealth, InferenceEngineKind,
    OpenAiChatRequestMetadata, OpenAiError, OpenAiErrorEnvelope, OpenAiResponsesRequestMetadata,
    OpenAiResponsesUsage, OpenAiUsage,
};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{Mutex as AsyncMutex, Notify, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;
use tower::limit::ConcurrencyLimitLayer;
use uuid::Uuid;

use crate::{
    CredentialRegistry, ErrorEmission, RepeatedErrorAggregator, UsageRequestRecord, UsageWriter,
    gateway_auth::AuthenticatedClient,
};

pub const DEFAULT_GATEWAY_ADDRESS: SocketAddr =
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 10_100);
const MAX_REQUEST_BODY_BYTES: usize = 2 * 1024 * 1024;
const MAX_NON_STREAM_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_SSE_DATA_LINE_BYTES: usize = 1024 * 1024;
const MAX_SSE_LINES_PER_FEED: usize = 16 * 1024;
const MAX_CONCURRENT_REQUESTS: usize = 64;
const MAX_ACTIVE_STREAMS: usize = 16;
const MAX_STREAM_RESPONSE_BYTES: usize = 64 * 1024 * 1024;
const MAX_MODEL_ID_BYTES: usize = 256;
const MAX_BACKEND_ID_BYTES: usize = 128;
const REQUEST_BODY_TIMEOUT: Duration = Duration::from_secs(15);
const UPSTREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
const UPSTREAM_TOTAL_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const CIRCUIT_FAILURE_THRESHOLD: usize = 3;
const IDEMPOTENT_MAX_ATTEMPTS: usize = 2;
const IDEMPOTENT_RETRY_DELAY: Duration = Duration::from_millis(25);
#[cfg(not(test))]
const CIRCUIT_OPEN_DURATION: Duration = Duration::from_secs(15);
#[cfg(test)]
const CIRCUIT_OPEN_DURATION: Duration = Duration::from_millis(20);

#[derive(Clone)]
pub struct BackendConfig {
    id: String,
    api_root: Url,
    api_key: Option<String>,
    auth_style: BackendAuthStyle,
    response_compatibility: BackendResponseCompatibility,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum BackendResponseCompatibility {
    #[default]
    Strict,
    /// Official MLC LLM currently serializes `function.arguments` as an object instead of the
    /// JSON string required by OpenAI clients. HAL100 normalizes only the qualified unary shape.
    MlcLlmStructuredToolArguments,
}

#[derive(Clone, Debug)]
pub struct ActiveGatewayRoute {
    backend: BackendConfig,
    resolved_model: Option<String>,
}

impl ActiveGatewayRoute {
    pub fn passthrough(backend: BackendConfig) -> Self {
        Self {
            backend,
            resolved_model: None,
        }
    }

    pub fn resolved(
        backend: BackendConfig,
        resolved_model: impl Into<String>,
    ) -> Result<Self, GatewayRouteError> {
        let resolved_model = resolved_model.into();
        validate_resolved_model(&resolved_model)?;
        Ok(Self {
            backend,
            resolved_model: Some(resolved_model),
        })
    }

    pub fn backend(&self) -> &BackendConfig {
        &self.backend
    }

    pub fn resolved_model(&self) -> Option<&str> {
        self.resolved_model.as_deref()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BackendAuthStyle {
    #[default]
    Bearer,
    AnthropicApiKey,
}

impl fmt::Debug for BackendConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BackendConfig")
            .field("id", &self.id)
            .field("api_root", &self.api_root)
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .field("auth_style", &self.auth_style)
            .field("response_compatibility", &self.response_compatibility)
            .finish()
    }
}

impl BackendConfig {
    pub fn new(
        id: impl Into<String>,
        api_root: &str,
        api_key: Option<String>,
    ) -> Result<Self, GatewayBuildError> {
        let id = id.into();
        if validate_backend_id(&id).is_err() {
            return Err(GatewayBuildError::InvalidBackendId);
        }
        let mut api_root = Url::parse(api_root)
            .map_err(|error| GatewayBuildError::InvalidBackendUrl(error.to_string()))?;
        if !matches!(api_root.scheme(), "http" | "https")
            || !api_root.username().is_empty()
            || api_root.password().is_some()
            || api_root.query().is_some()
            || api_root.fragment().is_some()
        {
            return Err(GatewayBuildError::UnsafeBackendUrl);
        }
        if !api_root.path().ends_with('/') {
            let path = format!("{}/", api_root.path());
            api_root.set_path(&path);
        }
        Ok(Self {
            id,
            api_root,
            api_key,
            auth_style: BackendAuthStyle::Bearer,
            response_compatibility: BackendResponseCompatibility::Strict,
        })
    }

    pub fn with_auth_style(mut self, auth_style: BackendAuthStyle) -> Self {
        self.auth_style = auth_style;
        self
    }

    /// Bind protocol compatibility behavior to a Rust-validated engine identity.
    ///
    /// Generic or unknown OpenAI backends remain byte-for-byte strict. This method does not infer
    /// an engine from a URL or model name.
    pub fn with_engine_kind(mut self, engine: Option<InferenceEngineKind>) -> Self {
        self.response_compatibility = match engine {
            Some(InferenceEngineKind::MlcLlm) => {
                BackendResponseCompatibility::MlcLlmStructuredToolArguments
            }
            _ => BackendResponseCompatibility::Strict,
        };
        self
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn api_root(&self) -> &Url {
        &self.api_root
    }

    fn endpoint(&self, relative_path: &str) -> Result<Url, GatewayError> {
        self.api_root
            .join(relative_path)
            .map_err(|_| GatewayError::BackendConfiguration)
    }

    fn authenticate(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let Some(api_key) = &self.api_key else {
            return request;
        };
        match self.auth_style {
            BackendAuthStyle::Bearer => request.bearer_auth(api_key),
            BackendAuthStyle::AnthropicApiKey => request.header("x-api-key", api_key),
        }
    }

    fn validate_protocol_request(
        &self,
        protocol: InferenceProtocol,
        body: &[u8],
        streaming: bool,
    ) -> Result<(), GatewayError> {
        if self.response_compatibility
            != BackendResponseCompatibility::MlcLlmStructuredToolArguments
            || protocol != InferenceProtocol::OpenAiChatCompletions
            || !streaming
        {
            return Ok(());
        }
        let request = serde_json::from_slice::<serde_json::Value>(body)
            .map_err(|_| GatewayError::InvalidRequest(protocol))?;
        if request
            .get("tools")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|tools| !tools.is_empty())
        {
            // MLC's unary structured arguments have a bounded compatibility transform below.
            // Streaming tool-call deltas have not passed the HAL100 protocol contract, so do not
            // forward a shape that standard OpenAI clients may misparse.
            return Err(GatewayError::InvalidRequest(protocol));
        }
        Ok(())
    }

    fn normalize_success_response(
        &self,
        protocol: InferenceProtocol,
        body: Bytes,
    ) -> Result<Bytes, GatewayError> {
        if self.response_compatibility
            != BackendResponseCompatibility::MlcLlmStructuredToolArguments
            || protocol != InferenceProtocol::OpenAiChatCompletions
        {
            return Ok(body);
        }
        normalize_mlc_llm_unary_tool_arguments(body)
    }
}

fn normalize_mlc_llm_unary_tool_arguments(body: Bytes) -> Result<Bytes, GatewayError> {
    let mut response = serde_json::from_slice::<serde_json::Value>(&body)
        .map_err(|_| GatewayError::BackendProtocol)?;
    let mut changed = false;
    if let Some(choices) = response
        .get_mut("choices")
        .and_then(serde_json::Value::as_array_mut)
    {
        for tool_call in choices
            .iter_mut()
            .filter_map(|choice| choice.get_mut("message"))
            .filter_map(|message| message.get_mut("tool_calls"))
            .filter_map(serde_json::Value::as_array_mut)
            .flatten()
        {
            let Some(arguments) = tool_call
                .get_mut("function")
                .and_then(|function| function.get_mut("arguments"))
            else {
                return Err(GatewayError::BackendProtocol);
            };
            match arguments {
                serde_json::Value::String(value) => {
                    if !serde_json::from_str::<serde_json::Value>(value)
                        .is_ok_and(|value| value.is_object())
                    {
                        return Err(GatewayError::BackendProtocol);
                    }
                }
                serde_json::Value::Object(object) => {
                    let encoded =
                        serde_json::to_string(object).map_err(|_| GatewayError::BackendProtocol)?;
                    *arguments = serde_json::Value::String(encoded);
                    changed = true;
                }
                _ => return Err(GatewayError::BackendProtocol),
            }
        }
    }
    if !changed {
        return Ok(body);
    }
    let normalized = serde_json::to_vec(&response).map_err(|_| GatewayError::BackendProtocol)?;
    if normalized.len() > MAX_NON_STREAM_RESPONSE_BYTES {
        return Err(GatewayError::BackendResponseTooLarge);
    }
    Ok(Bytes::from(normalized))
}

#[derive(Debug, Error)]
pub enum GatewayBuildError {
    #[error("backend identifier must be non-empty")]
    InvalidBackendId,
    #[error("backend URL is invalid: {0}")]
    InvalidBackendUrl(String),
    #[error("backend URL must use HTTP(S), exclude embedded credentials, query, and fragment")]
    UnsafeBackendUrl,
    #[error("failed to build the HTTP client: {0}")]
    HttpClient(#[from] reqwest::Error),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GatewayRouteError {
    #[error("backend identifier is invalid")]
    InvalidBackendId,
    #[error("model alias is invalid")]
    InvalidAlias,
    #[error("resolved model identifier is invalid")]
    InvalidResolvedModel,
    #[error("hal100-active is reserved for the active backend")]
    ReservedActiveAlias,
    #[error("route references an unknown backend")]
    UnknownBackend,
    #[error("backend is still referenced by a model route")]
    BackendInUse,
    #[error("active backend cannot be removed")]
    BackendActive,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GatewayRouteSwitchError {
    #[error("backend identifier is invalid")]
    InvalidBackendId,
    #[error("active route did not drain before the deadline; {active_requests} requests remain")]
    DrainTimeout { active_requests: usize },
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GatewayProbeError {
    #[error("backend is not loaded in the gateway")]
    UnknownBackend,
    #[error("backend is currently draining active requests")]
    RouteDraining,
    #[error("backend probe endpoint is invalid")]
    BackendConfiguration,
    #[error("backend probe was cancelled by a forced route switch")]
    ForcedRouteSwitch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRouteSnapshot {
    pub alias: String,
    pub backend_id: String,
    pub resolved_model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayRoutingSnapshot {
    pub active_backend_id: Option<String>,
    pub active_resolved_model: Option<String>,
    pub backend_ids: Vec<String>,
    pub model_routes: Vec<ModelRouteSnapshot>,
    pub backend_health: Vec<BackendHealthSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendHealthSnapshot {
    pub backend_id: String,
    pub consecutive_failures: usize,
    pub circuit_open: bool,
}

#[derive(Clone)]
struct ModelRoute {
    backend_id: String,
    resolved_model: String,
}

struct RoutedBackend {
    backend: BackendConfig,
    resolved_model: String,
    lease: BackendRequestLease,
}

struct BackendActivity {
    active_requests: AtomicUsize,
    drained: Notify,
    consecutive_failures: AtomicUsize,
    circuit_open_until_ms: AtomicU64,
    request_generation: RwLock<CancellationToken>,
}

impl BackendActivity {
    fn new() -> Self {
        Self {
            active_requests: AtomicUsize::new(0),
            drained: Notify::new(),
            consecutive_failures: AtomicUsize::new(0),
            circuit_open_until_ms: AtomicU64::new(0),
            request_generation: RwLock::new(CancellationToken::new()),
        }
    }

    fn reset_health(&self) {
        self.consecutive_failures.store(0, Ordering::Release);
        self.circuit_open_until_ms.store(0, Ordering::Release);
    }

    fn record_success(&self) {
        self.reset_health();
    }

    fn record_failure(&self, now_ms: u64) {
        let failures = self.consecutive_failures.fetch_add(1, Ordering::AcqRel) + 1;
        if failures >= CIRCUIT_FAILURE_THRESHOLD {
            let open_duration_ms =
                u64::try_from(CIRCUIT_OPEN_DURATION.as_millis()).unwrap_or(u64::MAX);
            self.circuit_open_until_ms.store(
                now_ms.saturating_add(open_duration_ms).max(1),
                Ordering::Release,
            );
            self.consecutive_failures.store(0, Ordering::Release);
        }
    }

    fn circuit_open(&self, now_ms: u64) -> bool {
        let open_until = self.circuit_open_until_ms.load(Ordering::Acquire);
        if open_until == 0 {
            return false;
        }
        if open_until > now_ms {
            return true;
        }
        let _ = self.circuit_open_until_ms.compare_exchange(
            open_until,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        false
    }

    fn request_token(&self) -> CancellationToken {
        self.request_generation
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn cancel_request_generation(&self) {
        let mut generation = self
            .request_generation
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        generation.cancel();
        *generation = CancellationToken::new();
    }
}

struct BackendRequestLease {
    activity: Arc<BackendActivity>,
    cancellation: CancellationToken,
}

impl BackendRequestLease {
    fn was_forced(&self) -> bool {
        self.cancellation.is_cancelled()
    }
}

impl Drop for BackendRequestLease {
    fn drop(&mut self) {
        if self.activity.active_requests.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.activity.drained.notify_waiters();
        }
    }
}

struct GatewayRoutes {
    active_route: Option<ActiveGatewayRoute>,
    backends: HashMap<String, BackendConfig>,
    model_routes: HashMap<String, ModelRoute>,
    activities: HashMap<String, Arc<BackendActivity>>,
    draining_backends: HashSet<String>,
}

impl GatewayRoutes {
    fn new(active_backend: Option<BackendConfig>) -> Self {
        let mut activities = HashMap::new();
        if let Some(backend) = &active_backend {
            activities.insert(backend.id.clone(), Arc::new(BackendActivity::new()));
        }
        Self {
            active_route: active_backend.map(ActiveGatewayRoute::passthrough),
            backends: HashMap::new(),
            model_routes: HashMap::new(),
            activities,
            draining_backends: HashSet::new(),
        }
    }

    fn activity(&mut self, backend_id: &str) -> Arc<BackendActivity> {
        self.activities
            .entry(backend_id.to_owned())
            .or_insert_with(|| Arc::new(BackendActivity::new()))
            .clone()
    }

    fn backend(&self, backend_id: &str) -> Option<BackendConfig> {
        self.active_route
            .as_ref()
            .map(ActiveGatewayRoute::backend)
            .filter(|backend| backend.id == backend_id)
            .cloned()
            .or_else(|| self.backends.get(backend_id).cloned())
    }

    fn snapshot(&self, now_ms: u64) -> GatewayRoutingSnapshot {
        let active_backend_id = self
            .active_route
            .as_ref()
            .map(|route| route.backend.id.clone());
        let active_resolved_model = self
            .active_route
            .as_ref()
            .and_then(|route| route.resolved_model.clone());
        let mut backend_ids = self.backends.keys().cloned().collect::<HashSet<_>>();
        if let Some(active_backend_id) = &active_backend_id {
            backend_ids.insert(active_backend_id.clone());
        }
        let mut backend_ids = backend_ids.into_iter().collect::<Vec<_>>();
        backend_ids.sort();
        let mut model_routes = self
            .model_routes
            .iter()
            .map(|(alias, route)| ModelRouteSnapshot {
                alias: alias.clone(),
                backend_id: route.backend_id.clone(),
                resolved_model: route.resolved_model.clone(),
            })
            .collect::<Vec<_>>();
        model_routes.sort_by(|left, right| left.alias.cmp(&right.alias));
        let mut backend_health = self
            .activities
            .iter()
            .filter(|(backend_id, _)| backend_ids.contains(backend_id))
            .map(|(backend_id, activity)| BackendHealthSnapshot {
                backend_id: backend_id.clone(),
                consecutive_failures: activity.consecutive_failures.load(Ordering::Acquire),
                circuit_open: activity.circuit_open(now_ms),
            })
            .collect::<Vec<_>>();
        backend_health.sort_by(|left, right| left.backend_id.cmp(&right.backend_id));
        GatewayRoutingSnapshot {
            active_backend_id,
            active_resolved_model,
            backend_ids,
            model_routes,
            backend_health,
        }
    }
}

#[derive(Clone)]
pub struct GatewayState {
    inner: Arc<GatewayStateInner>,
}

struct GatewayStateInner {
    routes: RwLock<GatewayRoutes>,
    credentials: CredentialRegistry,
    http_client: reqwest::Client,
    usage_writer: UsageWriter,
    stream_slots: Arc<Semaphore>,
    errors: RepeatedErrorAggregator,
    route_switch: AsyncMutex<()>,
    started: Instant,
}

impl GatewayState {
    pub fn new(
        backend: Option<BackendConfig>,
        credentials: CredentialRegistry,
        usage_writer: UsageWriter,
    ) -> Result<Self, GatewayBuildError> {
        let http_client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .read_timeout(UPSTREAM_IDLE_TIMEOUT)
            .timeout(UPSTREAM_TOTAL_TIMEOUT)
            .pool_idle_timeout(Duration::from_secs(60))
            .no_proxy()
            .build()?;
        Ok(Self {
            inner: Arc::new(GatewayStateInner {
                routes: RwLock::new(GatewayRoutes::new(backend)),
                credentials,
                http_client,
                usage_writer,
                stream_slots: Arc::new(Semaphore::new(MAX_ACTIVE_STREAMS)),
                errors: RepeatedErrorAggregator::new(Duration::from_secs(60)),
                route_switch: AsyncMutex::new(()),
                started: Instant::now(),
            }),
        })
    }

    pub fn has_backend(&self) -> bool {
        self.backend_config().is_some()
    }

    pub fn has_client_credentials(&self) -> bool {
        !self.inner.credentials.is_empty()
    }

    pub fn backend_config(&self) -> Option<BackendConfig> {
        self.active_route().map(|route| route.backend)
    }

    pub fn active_route(&self) -> Option<ActiveGatewayRoute> {
        self.inner
            .routes
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .active_route
            .clone()
    }

    pub fn replace_backend(&self, backend: Option<BackendConfig>) -> Option<BackendConfig> {
        self.replace_active_route(backend.map(ActiveGatewayRoute::passthrough))
            .map(|route| route.backend)
    }

    pub fn replace_active_route(
        &self,
        route: Option<ActiveGatewayRoute>,
    ) -> Option<ActiveGatewayRoute> {
        let mut routes = self
            .inner
            .routes
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(route) = &route {
            routes.activity(&route.backend.id);
        }
        std::mem::replace(&mut routes.active_route, route)
    }

    pub async fn replace_backend_when_idle(
        &self,
        backend: Option<BackendConfig>,
        drain_timeout: Duration,
    ) -> Result<Option<BackendConfig>, GatewayRouteSwitchError> {
        self.replace_active_route_when_idle(
            backend.map(ActiveGatewayRoute::passthrough),
            drain_timeout,
        )
        .await
        .map(|route| route.map(|route| route.backend))
    }

    pub async fn replace_active_route_when_idle(
        &self,
        route: Option<ActiveGatewayRoute>,
        drain_timeout: Duration,
    ) -> Result<Option<ActiveGatewayRoute>, GatewayRouteSwitchError> {
        if let Some(route) = &route {
            validate_backend_id(&route.backend.id)
                .map_err(|_| GatewayRouteSwitchError::InvalidBackendId)?;
        }
        let _switch_guard = self.inner.route_switch.lock().await;
        let draining = {
            let mut routes = self
                .inner
                .routes
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(route) = &route {
                routes.activity(&route.backend.id);
            }
            let active_id = routes
                .active_route
                .as_ref()
                .map(|active| active.backend.id.clone());
            active_id.map(|active_id| {
                let activity = routes.activity(&active_id);
                routes.draining_backends.insert(active_id.clone());
                (active_id, activity)
            })
        };

        if let Some((backend_id, activity)) = &draining {
            let deadline = tokio::time::Instant::now() + drain_timeout;
            loop {
                let notified = activity.drained.notified();
                let active_requests = activity.active_requests.load(Ordering::Acquire);
                if active_requests == 0 {
                    break;
                }
                if tokio::time::timeout_at(deadline, notified).await.is_err() {
                    self.inner
                        .routes
                        .write()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .draining_backends
                        .remove(backend_id);
                    return Err(GatewayRouteSwitchError::DrainTimeout {
                        active_requests: activity.active_requests.load(Ordering::Acquire),
                    });
                }
            }
        }

        let mut routes = self
            .inner
            .routes
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((backend_id, _)) = &draining {
            routes.draining_backends.remove(backend_id);
        }
        Ok(std::mem::replace(&mut routes.active_route, route))
    }

    pub async fn force_replace_backend(
        &self,
        backend: Option<BackendConfig>,
    ) -> Result<Option<BackendConfig>, GatewayRouteSwitchError> {
        self.force_replace_active_route(backend.map(ActiveGatewayRoute::passthrough))
            .await
            .map(|route| route.map(|route| route.backend))
    }

    pub async fn force_replace_active_route(
        &self,
        route: Option<ActiveGatewayRoute>,
    ) -> Result<Option<ActiveGatewayRoute>, GatewayRouteSwitchError> {
        if let Some(route) = &route {
            validate_backend_id(&route.backend.id)
                .map_err(|_| GatewayRouteSwitchError::InvalidBackendId)?;
        }
        let _switch_guard = self.inner.route_switch.lock().await;
        let mut routes = self
            .inner
            .routes
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(route) = &route {
            routes.activity(&route.backend.id);
        }
        if let Some(active_id) = routes
            .active_route
            .as_ref()
            .map(|active| active.backend.id.clone())
        {
            routes.activity(&active_id).cancel_request_generation();
            routes.draining_backends.remove(&active_id);
        }
        Ok(std::mem::replace(&mut routes.active_route, route))
    }

    pub fn upsert_routed_backend(&self, backend: BackendConfig) -> Result<(), GatewayRouteError> {
        validate_backend_id(&backend.id)?;
        let mut routes = self
            .inner
            .routes
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let activity = routes.activity(&backend.id);
        activity.reset_health();
        if routes
            .active_route
            .as_ref()
            .is_some_and(|active| active.backend.id == backend.id)
            && let Some(active) = routes.active_route.as_mut()
        {
            active.backend = backend.clone();
        }
        routes.backends.insert(backend.id.clone(), backend);
        Ok(())
    }

    pub fn remove_routed_backend(&self, backend_id: &str) -> Result<bool, GatewayRouteError> {
        validate_backend_id(backend_id)?;
        let mut routes = self
            .inner
            .routes
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if routes
            .active_route
            .as_ref()
            .is_some_and(|active| active.backend.id == backend_id)
        {
            return Err(GatewayRouteError::BackendActive);
        }
        if routes
            .model_routes
            .values()
            .any(|route| route.backend_id == backend_id)
        {
            return Err(GatewayRouteError::BackendInUse);
        }
        let removed = routes.backends.remove(backend_id).is_some();
        if removed {
            routes.activities.remove(backend_id);
            routes.draining_backends.remove(backend_id);
        }
        Ok(removed)
    }

    pub fn set_model_route(
        &self,
        alias: &str,
        backend_id: &str,
        resolved_model: &str,
    ) -> Result<(), GatewayRouteError> {
        validate_model_alias(alias)?;
        validate_backend_id(backend_id)?;
        validate_resolved_model(resolved_model)?;
        let mut routes = self
            .inner
            .routes
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if routes.backend(backend_id).is_none() {
            return Err(GatewayRouteError::UnknownBackend);
        }
        routes.model_routes.insert(
            alias.to_owned(),
            ModelRoute {
                backend_id: backend_id.to_owned(),
                resolved_model: resolved_model.to_owned(),
            },
        );
        Ok(())
    }

    pub fn remove_model_route(&self, alias: &str) -> Result<bool, GatewayRouteError> {
        validate_model_alias(alias)?;
        Ok(self
            .inner
            .routes
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .model_routes
            .remove(alias)
            .is_some())
    }

    pub fn routing_snapshot(&self) -> GatewayRoutingSnapshot {
        let now_ms = self.monotonic_ms();
        self.inner
            .routes
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot(now_ms)
    }

    pub async fn probe_backend(
        &self,
        backend_id: &str,
    ) -> Result<BackendProbeResult, GatewayProbeError> {
        let (backend, lease) = self.probe_backend_route(backend_id)?;
        let cancellation = lease.cancellation.clone();
        let endpoint = backend
            .endpoint("models")
            .map_err(|_| GatewayProbeError::BackendConfiguration)?;
        let started = Instant::now();
        for attempt in 0..IDEMPOTENT_MAX_ATTEMPTS {
            let request = backend.authenticate(self.inner.http_client.get(endpoint.clone()));
            let send_result = tokio::select! {
                biased;
                _ = cancellation.cancelled() => {
                    return Err(GatewayProbeError::ForcedRouteSwitch);
                }
                result = request.send() => result,
            };
            match send_result {
                Ok(response)
                    if is_retryable_backend_status(response.status())
                        && attempt + 1 < IDEMPOTENT_MAX_ATTEMPTS =>
                {
                    self.record_backend_failure(backend.id());
                    if retry_delay_cancelled(&cancellation).await {
                        return Err(GatewayProbeError::ForcedRouteSwitch);
                    }
                }
                Ok(response) => {
                    let status = response.status();
                    if status.is_server_error() {
                        self.record_backend_failure(backend.id());
                    } else {
                        self.record_backend_success(backend.id());
                    }
                    let result = if status.is_success() {
                        let collected = tokio::select! {
                            biased;
                            _ = cancellation.cancelled() => {
                                return Err(GatewayProbeError::ForcedRouteSwitch);
                            }
                            collected = collect_limited(response, 1024 * 1024, None) => collected,
                        };
                        match collected {
                            Ok(body) => {
                                let model_count =
                                    serde_json::from_slice::<serde_json::Value>(&body)
                                        .ok()
                                        .and_then(|value| {
                                            value
                                                .get("data")
                                                .and_then(serde_json::Value::as_array)
                                                .map(Vec::len)
                                        });
                                BackendProbeResult {
                                    backend_id: backend_id.to_owned(),
                                    status: if model_count.is_some() {
                                        BackendProbeStatus::Healthy
                                    } else {
                                        BackendProbeStatus::InvalidResponse
                                    },
                                    http_status: Some(status.as_u16()),
                                    latency_ms: duration_ms_u64(started.elapsed()),
                                    model_count,
                                }
                            }
                            Err(_) => {
                                self.record_backend_failure(backend.id());
                                BackendProbeResult {
                                    backend_id: backend_id.to_owned(),
                                    status: BackendProbeStatus::InvalidResponse,
                                    http_status: Some(status.as_u16()),
                                    latency_ms: duration_ms_u64(started.elapsed()),
                                    model_count: None,
                                }
                            }
                        }
                    } else {
                        BackendProbeResult {
                            backend_id: backend_id.to_owned(),
                            status: if matches!(
                                status,
                                reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
                            ) {
                                BackendProbeStatus::AuthenticationFailed
                            } else {
                                BackendProbeStatus::UpstreamError
                            },
                            http_status: Some(status.as_u16()),
                            latency_ms: duration_ms_u64(started.elapsed()),
                            model_count: None,
                        }
                    };
                    drop(lease);
                    return Ok(result);
                }
                Err(_) if attempt + 1 < IDEMPOTENT_MAX_ATTEMPTS => {
                    self.record_backend_failure(backend.id());
                    if retry_delay_cancelled(&cancellation).await {
                        return Err(GatewayProbeError::ForcedRouteSwitch);
                    }
                }
                Err(_) => {
                    self.record_backend_failure(backend.id());
                    drop(lease);
                    return Ok(BackendProbeResult {
                        backend_id: backend_id.to_owned(),
                        status: BackendProbeStatus::Unreachable,
                        http_status: None,
                        latency_ms: duration_ms_u64(started.elapsed()),
                        model_count: None,
                    });
                }
            }
        }
        drop(lease);
        Ok(BackendProbeResult {
            backend_id: backend_id.to_owned(),
            status: BackendProbeStatus::Unreachable,
            http_status: None,
            latency_ms: duration_ms_u64(started.elapsed()),
            model_count: None,
        })
    }

    fn probe_backend_route(
        &self,
        backend_id: &str,
    ) -> Result<(BackendConfig, BackendRequestLease), GatewayProbeError> {
        let routes = self
            .inner
            .routes
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let backend = routes
            .backend(backend_id)
            .ok_or(GatewayProbeError::UnknownBackend)?;
        if routes.draining_backends.contains(backend_id) {
            return Err(GatewayProbeError::RouteDraining);
        }
        let activity = routes
            .activities
            .get(backend_id)
            .cloned()
            .ok_or(GatewayProbeError::UnknownBackend)?;
        let cancellation = activity.request_token();
        activity.active_requests.fetch_add(1, Ordering::AcqRel);
        Ok((
            backend,
            BackendRequestLease {
                activity,
                cancellation,
            },
        ))
    }

    fn route_backend(&self, requested_model: &str) -> Result<RoutedBackend, GatewayError> {
        let routes = self
            .inner
            .routes
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (backend, resolved_model) = if requested_model == "hal100-active" {
            let route = routes
                .active_route
                .clone()
                .ok_or(GatewayError::BackendUnavailable)?;
            let resolved_model = route
                .resolved_model
                .unwrap_or_else(|| requested_model.to_owned());
            (route.backend, resolved_model)
        } else if let Some(route) = routes.model_routes.get(requested_model) {
            let backend = routes
                .backend(&route.backend_id)
                .ok_or(GatewayError::BackendUnavailable)?;
            (backend, route.resolved_model.clone())
        } else {
            let route = routes
                .active_route
                .clone()
                .ok_or(GatewayError::BackendUnavailable)?;
            (route.backend, requested_model.to_owned())
        };
        if routes.draining_backends.contains(&backend.id) {
            return Err(GatewayError::RouteDraining);
        }
        let activity = routes
            .activities
            .get(&backend.id)
            .cloned()
            .ok_or(GatewayError::BackendUnavailable)?;
        if activity.circuit_open(self.monotonic_ms()) {
            return Err(GatewayError::BackendCircuitOpen);
        }
        let cancellation = activity.request_token();
        activity.active_requests.fetch_add(1, Ordering::AcqRel);
        Ok(RoutedBackend {
            backend,
            resolved_model,
            lease: BackendRequestLease {
                activity,
                cancellation,
            },
        })
    }

    fn record_usage(&self, usage: UsageRequestRecord) {
        if self.inner.usage_writer.record(usage).is_err() {
            self.report_repeated_error("usage_queue_unavailable");
        }
    }

    fn record_backend_success(&self, backend_id: &str) {
        if let Some(activity) = self.backend_activity(backend_id) {
            activity.record_success();
        }
    }

    fn record_backend_failure(&self, backend_id: &str) {
        if let Some(activity) = self.backend_activity(backend_id) {
            activity.record_failure(self.monotonic_ms());
        }
    }

    fn backend_activity(&self, backend_id: &str) -> Option<Arc<BackendActivity>> {
        self.inner
            .routes
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .activities
            .get(backend_id)
            .cloned()
    }

    fn monotonic_ms(&self) -> u64 {
        u64::try_from(self.inner.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn report_repeated_error(&self, error_code: &'static str) {
        match self.inner.errors.observe(error_code) {
            ErrorEmission::Emit => tracing::error!(error_code, "gateway_runtime_error"),
            ErrorEmission::Suppress => {}
            ErrorEmission::EmitWithSummary { suppressed } => {
                tracing::error!(error_code, suppressed, "gateway_runtime_error_repeated")
            }
        }
    }
}

fn validate_backend_id(backend_id: &str) -> Result<(), GatewayRouteError> {
    if backend_id.is_empty()
        || backend_id.len() > MAX_BACKEND_ID_BYTES
        || !backend_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(GatewayRouteError::InvalidBackendId);
    }
    Ok(())
}

fn validate_model_alias(alias: &str) -> Result<(), GatewayRouteError> {
    if alias == "hal100-active" {
        return Err(GatewayRouteError::ReservedActiveAlias);
    }
    if alias.trim().is_empty()
        || alias.len() > MAX_MODEL_ID_BYTES
        || alias.chars().any(char::is_whitespace)
        || alias.chars().any(char::is_control)
    {
        return Err(GatewayRouteError::InvalidAlias);
    }
    Ok(())
}

fn validate_resolved_model(resolved_model: &str) -> Result<(), GatewayRouteError> {
    if resolved_model.trim().is_empty()
        || resolved_model.len() > MAX_MODEL_ID_BYTES
        || resolved_model.chars().any(char::is_control)
    {
        return Err(GatewayRouteError::InvalidResolvedModel);
    }
    Ok(())
}

pub fn health_router() -> Router {
    Router::new().route("/healthz", get(health))
}

pub fn gateway_router(state: GatewayState) -> Router {
    let protected = Router::new()
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/responses", post(responses))
        .route("/v1/messages", post(messages))
        .route_layer(from_fn_with_state(state.clone(), authenticate));

    Router::new()
        .route("/healthz", get(health))
        .merge(protected)
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .layer(ConcurrencyLimitLayer::new(MAX_CONCURRENT_REQUESTS))
        .with_state(state)
}

pub async fn serve_gateway(listener: std::net::TcpListener, state: GatewayState) -> io::Result<()> {
    listener.set_nonblocking(true)?;
    let listener = tokio::net::TcpListener::from_std(listener)?;
    axum::serve(listener, gateway_router(state)).await
}

async fn health() -> Json<GatewayHealth> {
    Json(GatewayHealth {
        service: "hal100-gateway".to_owned(),
        status: "ok".to_owned(),
        protocol_version: 1,
    })
}

async fn authenticate(
    State(state): State<GatewayState>,
    mut request: Request,
    next: Next,
) -> Response {
    let protocol = InferenceProtocol::from_path(request.uri().path());
    let Some(key) = client_token(request.headers()) else {
        return GatewayError::Unauthorized.into_response_for(protocol);
    };
    let Some(client) = state.inner.credentials.authenticate(key) else {
        return GatewayError::Unauthorized.into_response_for(protocol);
    };
    request.extensions_mut().insert(client);
    next.run(request).await
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    value.strip_prefix("Bearer ")
}

fn client_token(headers: &HeaderMap) -> Option<&str> {
    let bearer = bearer_token(headers);
    let anthropic = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok());
    match (bearer, anthropic) {
        (Some(bearer), Some(anthropic)) if bearer == anthropic => Some(bearer),
        (Some(bearer), None) => Some(bearer),
        (None, Some(anthropic)) => Some(anthropic),
        _ => None,
    }
}

async fn models(State(state): State<GatewayState>) -> Result<Response, GatewayError> {
    let RoutedBackend { backend, lease, .. } = state.route_backend("hal100-active")?;
    let cancellation = lease.cancellation.clone();
    let endpoint = backend.endpoint("models")?;
    let mut final_upstream = None;
    for attempt in 0..IDEMPOTENT_MAX_ATTEMPTS {
        let request = backend.authenticate(state.inner.http_client.get(endpoint.clone()));
        let send_result = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(GatewayError::ForcedRouteSwitch),
            result = request.send() => result,
        };
        match send_result {
            Ok(upstream)
                if is_retryable_backend_status(upstream.status())
                    && attempt + 1 < IDEMPOTENT_MAX_ATTEMPTS =>
            {
                state.record_backend_failure(backend.id());
                if retry_delay_cancelled(&cancellation).await {
                    return Err(GatewayError::ForcedRouteSwitch);
                }
            }
            Ok(upstream) => {
                if upstream.status().is_server_error() {
                    state.record_backend_failure(backend.id());
                } else {
                    state.record_backend_success(backend.id());
                }
                final_upstream = Some(upstream);
                break;
            }
            Err(_) if attempt + 1 < IDEMPOTENT_MAX_ATTEMPTS => {
                state.record_backend_failure(backend.id());
                if retry_delay_cancelled(&cancellation).await {
                    return Err(GatewayError::ForcedRouteSwitch);
                }
            }
            Err(_) => {
                state.record_backend_failure(backend.id());
                return Err(GatewayError::BackendTransport);
            }
        }
    }
    let upstream = final_upstream.ok_or(GatewayError::BackendTransport)?;
    let response = tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(GatewayError::ForcedRouteSwitch),
        response = proxy_bounded_response(upstream) => response,
    };
    if matches!(response, Err(GatewayError::BackendStream)) {
        state.record_backend_failure(backend.id());
    }
    drop(lease);
    response
}

async fn retry_delay_cancelled(cancellation: &CancellationToken) -> bool {
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => true,
        _ = tokio::time::sleep(IDEMPOTENT_RETRY_DELAY) => false,
    }
}

fn is_retryable_backend_status(status: reqwest::StatusCode) -> bool {
    matches!(
        status,
        reqwest::StatusCode::BAD_GATEWAY
            | reqwest::StatusCode::SERVICE_UNAVAILABLE
            | reqwest::StatusCode::GATEWAY_TIMEOUT
    )
}

async fn chat_completions(
    State(state): State<GatewayState>,
    Extension(client): Extension<AuthenticatedClient>,
    request: Request,
) -> Response {
    let protocol = InferenceProtocol::OpenAiChatCompletions;
    proxy_inference(state, client, request, protocol)
        .await
        .unwrap_or_else(|error| error.into_response_for(Some(protocol)))
}

async fn responses(
    State(state): State<GatewayState>,
    Extension(client): Extension<AuthenticatedClient>,
    request: Request,
) -> Response {
    let protocol = InferenceProtocol::OpenAiResponses;
    proxy_inference(state, client, request, protocol)
        .await
        .unwrap_or_else(|error| error.into_response_for(Some(protocol)))
}

async fn messages(
    State(state): State<GatewayState>,
    Extension(client): Extension<AuthenticatedClient>,
    request: Request,
) -> Response {
    let protocol = InferenceProtocol::AnthropicMessages;
    proxy_inference(state, client, request, protocol)
        .await
        .unwrap_or_else(|error| error.into_response_for(Some(protocol)))
}

async fn proxy_inference(
    state: GatewayState,
    client: AuthenticatedClient,
    request: Request,
    protocol: InferenceProtocol,
) -> Result<Response, GatewayError> {
    let request_headers = request.headers().clone();
    let body = tokio::time::timeout(
        REQUEST_BODY_TIMEOUT,
        axum::body::to_bytes(request.into_body(), MAX_REQUEST_BODY_BYTES),
    )
    .await
    .map_err(|_| GatewayError::RequestTimeout)?
    .map_err(|_| GatewayError::InvalidRequest(protocol))?;
    let metadata = protocol
        .request_metadata(&body)
        .ok_or(GatewayError::InvalidRequest(protocol))?;
    if metadata.model.trim().is_empty()
        || metadata.model.len() > MAX_MODEL_ID_BYTES
        || metadata.model.chars().any(char::is_control)
    {
        return Err(GatewayError::InvalidRequest(protocol));
    }

    let stream_permit = if metadata.stream {
        Some(
            state
                .inner
                .stream_slots
                .clone()
                .try_acquire_owned()
                .map_err(|_| GatewayError::TooManyStreams)?,
        )
    } else {
        None
    };

    let requested_model = metadata.model;
    let RoutedBackend {
        backend,
        resolved_model,
        lease,
    } = state.route_backend(&requested_model)?;
    backend.validate_protocol_request(protocol, &body, metadata.stream)?;
    let body = rewrite_request_model(body, &resolved_model, protocol)?;
    let route_cancellation = lease.cancellation.clone();
    let mut route_lease = Some(lease);
    let request_id = Uuid::new_v4().to_string();
    let mut usage = UsageTracker::new(
        state.clone(),
        UsageRequestContext {
            request_id: request_id.clone(),
            client_app_id: client.client_app_id,
            requested_model,
            resolved_model,
            backend_id: backend.id.clone(),
        },
        protocol,
        metadata.stream,
    );
    let endpoint = backend.endpoint(protocol.upstream_path())?;
    let request = state
        .inner
        .http_client
        .post(endpoint)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body);
    let request = protocol.forward_protocol_headers(request, &request_headers);
    let request = backend.authenticate(request);
    let send_result = tokio::select! {
        biased;
        _ = route_cancellation.cancelled() => {
            usage.finish_forced_route_switch();
            return Err(GatewayError::ForcedRouteSwitch);
        }
        result = request.send() => result,
    };
    let upstream = match send_result {
        Ok(response) => response,
        Err(_) => {
            state.record_backend_failure(backend.id());
            usage.finish_failed("backend_transport");
            return Err(GatewayError::BackendTransport);
        }
    };

    let status = upstream.status();
    if status.is_server_error() {
        state.record_backend_failure(backend.id());
    } else {
        state.record_backend_success(backend.id());
    }
    let content_type = upstream.headers().get(header::CONTENT_TYPE).cloned();
    let successful_status = status.is_success();
    let mut response = if metadata.stream {
        let stream = GatewayBodyStream::new(
            upstream.bytes_stream(),
            usage,
            successful_status,
            stream_permit.expect("streaming requests reserve a slot"),
            route_lease.take().expect("routed requests own a lease"),
        );
        Response::builder()
            .status(status)
            .body(Body::from_stream(stream))
            .expect("valid streaming proxy response")
    } else {
        let collected = tokio::select! {
            biased;
            _ = route_cancellation.cancelled() => {
                usage.finish_forced_route_switch();
                return Err(GatewayError::ForcedRouteSwitch);
            }
            collected = collect_inference_response(upstream, &mut usage) => collected,
        };
        let bytes = match collected {
            Ok(bytes) => bytes,
            Err(error) => {
                if matches!(error, GatewayError::BackendStream) {
                    state.record_backend_failure(backend.id());
                }
                return Err(error);
            }
        };
        let bytes = if successful_status {
            match backend.normalize_success_response(protocol, bytes) {
                Ok(bytes) => bytes,
                Err(error) => {
                    state.record_backend_failure(backend.id());
                    usage.finish_failed("backend_protocol");
                    return Err(error);
                }
            }
        } else {
            bytes
        };
        usage.finish_http_status(successful_status);
        Response::builder()
            .status(status)
            .body(Body::from(bytes))
            .expect("valid buffered proxy response")
    };
    if let Some(content_type) = content_type {
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, content_type);
    }
    response.headers_mut().insert(
        "x-hal100-request-id",
        HeaderValue::from_str(&request_id).expect("UUID is a valid header value"),
    );
    if matches!(protocol, InferenceProtocol::AnthropicMessages) {
        response.headers_mut().insert(
            "request-id",
            HeaderValue::from_str(&request_id).expect("UUID is a valid header value"),
        );
    }
    Ok(response)
}

fn rewrite_request_model(
    body: Bytes,
    resolved_model: &str,
    protocol: InferenceProtocol,
) -> Result<Bytes, GatewayError> {
    let requested_model = match protocol.request_metadata(&body) {
        Some(metadata) => metadata.model,
        None => return Err(GatewayError::InvalidRequest(protocol)),
    };
    if requested_model == resolved_model {
        return Ok(body);
    }
    let mut request: serde_json::Value =
        serde_json::from_slice(&body).map_err(|_| GatewayError::InvalidRequest(protocol))?;
    let Some(model) = request.get_mut("model") else {
        return Err(GatewayError::InvalidRequest(protocol));
    };
    *model = serde_json::Value::String(resolved_model.to_owned());
    let rewritten =
        serde_json::to_vec(&request).map_err(|_| GatewayError::InvalidRequest(protocol))?;
    if rewritten.len() > MAX_REQUEST_BODY_BYTES {
        return Err(GatewayError::InvalidRequest(protocol));
    }
    Ok(Bytes::from(rewritten))
}

async fn proxy_bounded_response(upstream: reqwest::Response) -> Result<Response, GatewayError> {
    let status = upstream.status();
    let content_type = upstream.headers().get(header::CONTENT_TYPE).cloned();
    let bytes = collect_limited(upstream, MAX_NON_STREAM_RESPONSE_BYTES, None).await?;
    let mut response = Response::builder()
        .status(status)
        .body(Body::from(bytes))
        .expect("valid proxy response");
    if let Some(content_type) = content_type {
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, content_type);
    }
    Ok(response)
}

async fn collect_inference_response(
    upstream: reqwest::Response,
    usage: &mut UsageTracker,
) -> Result<Bytes, GatewayError> {
    let bytes = collect_limited(upstream, MAX_NON_STREAM_RESPONSE_BYTES, Some(usage)).await?;
    if let Some(exact_usage) = usage.protocol.parse_usage(&bytes) {
        usage.exact_usage = Some(exact_usage);
    }
    Ok(bytes)
}

async fn collect_limited(
    upstream: reqwest::Response,
    limit: usize,
    mut usage: Option<&mut UsageTracker>,
) -> Result<Bytes, GatewayError> {
    if upstream
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        if let Some(usage) = usage.as_deref_mut() {
            usage.finish_failed("backend_response_too_large");
        }
        return Err(GatewayError::BackendResponseTooLarge);
    }
    let mut stream = upstream.bytes_stream();
    let mut collected = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(_) => {
                if let Some(usage) = usage.as_deref_mut() {
                    usage.finish_failed("backend_stream");
                }
                return Err(GatewayError::BackendStream);
            }
        };
        if collected.len().saturating_add(chunk.len()) > limit {
            if let Some(usage) = usage.as_deref_mut() {
                usage.finish_failed("backend_response_too_large");
            }
            return Err(GatewayError::BackendResponseTooLarge);
        }
        if let Some(usage) = usage.as_deref_mut() {
            usage.observe_bytes(&chunk);
        }
        collected.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(collected))
}

#[derive(Deserialize)]
struct UsageEnvelope {
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[derive(Deserialize)]
struct ResponsesUsageEnvelope {
    #[serde(default)]
    usage: Option<OpenAiResponsesUsage>,
}

#[derive(Deserialize)]
struct ResponsesEventEnvelope {
    #[serde(default)]
    response: Option<ResponsesUsageEnvelope>,
}

#[derive(Deserialize)]
struct AnthropicUsageEnvelope {
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

#[derive(Default, Deserialize)]
struct AnthropicPartialUsage {
    input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct AnthropicStreamMessage {
    #[serde(default)]
    usage: Option<AnthropicPartialUsage>,
}

#[derive(Deserialize)]
struct AnthropicStreamEvent {
    #[serde(default)]
    usage: Option<AnthropicPartialUsage>,
    #[serde(default)]
    message: Option<AnthropicStreamMessage>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InferenceProtocol {
    OpenAiChatCompletions,
    OpenAiResponses,
    AnthropicMessages,
}

impl InferenceProtocol {
    fn request_metadata(self, body: &[u8]) -> Option<RequestMetadata> {
        match self {
            Self::OpenAiChatCompletions => {
                serde_json::from_slice::<OpenAiChatRequestMetadata>(body)
                    .ok()
                    .map(|metadata| RequestMetadata {
                        model: metadata.model,
                        stream: metadata.stream,
                    })
            }
            Self::OpenAiResponses => serde_json::from_slice::<OpenAiResponsesRequestMetadata>(body)
                .ok()
                .map(|metadata| RequestMetadata {
                    model: metadata.model,
                    stream: metadata.stream,
                }),
            Self::AnthropicMessages => {
                serde_json::from_slice::<AnthropicMessagesRequestMetadata>(body)
                    .ok()
                    .map(|metadata| RequestMetadata {
                        model: metadata.model,
                        stream: metadata.stream,
                    })
            }
        }
    }

    fn upstream_path(self) -> &'static str {
        match self {
            Self::OpenAiChatCompletions => "chat/completions",
            Self::OpenAiResponses => "responses",
            Self::AnthropicMessages => "messages",
        }
    }

    fn usage_protocol(self) -> &'static str {
        match self {
            Self::OpenAiChatCompletions => "openai_chat_completions",
            Self::OpenAiResponses => "openai_responses",
            Self::AnthropicMessages => "anthropic_messages",
        }
    }

    fn invalid_request_message(self) -> &'static str {
        match self {
            Self::OpenAiChatCompletions => "请求不是有效的OpenAI Chat Completions请求。",
            Self::OpenAiResponses => "请求不是有效的OpenAI Responses请求。",
            Self::AnthropicMessages => "请求不是有效的Anthropic Messages请求。",
        }
    }

    fn from_path(path: &str) -> Option<Self> {
        match path {
            "/v1/chat/completions" => Some(Self::OpenAiChatCompletions),
            "/v1/responses" => Some(Self::OpenAiResponses),
            "/v1/messages" => Some(Self::AnthropicMessages),
            _ => None,
        }
    }

    fn forward_protocol_headers(
        self,
        mut request: reqwest::RequestBuilder,
        headers: &HeaderMap,
    ) -> reqwest::RequestBuilder {
        if !matches!(self, Self::AnthropicMessages) {
            return request;
        }
        if let Some(version) = headers.get("anthropic-version") {
            request = request.header("anthropic-version", version);
        } else {
            request = request.header("anthropic-version", "2023-06-01");
        }
        if let Some(beta) = headers.get("anthropic-beta") {
            request = request.header("anthropic-beta", beta);
        }
        request
    }

    fn parse_usage(self, body: &[u8]) -> Option<ExactUsage> {
        match self {
            Self::OpenAiChatCompletions => serde_json::from_slice::<UsageEnvelope>(body)
                .ok()?
                .usage
                .map(ExactUsage::from),
            Self::OpenAiResponses => {
                if let Some(usage) = serde_json::from_slice::<ResponsesUsageEnvelope>(body)
                    .ok()
                    .and_then(|envelope| envelope.usage)
                {
                    return Some(ExactUsage::from(usage));
                }
                serde_json::from_slice::<ResponsesEventEnvelope>(body)
                    .ok()?
                    .response?
                    .usage
                    .map(ExactUsage::from)
            }
            Self::AnthropicMessages => serde_json::from_slice::<AnthropicUsageEnvelope>(body)
                .ok()?
                .usage
                .map(ExactUsage::from),
        }
    }

    fn merge_stream_usage(self, body: &[u8], current: Option<ExactUsage>) -> Option<ExactUsage> {
        if !body
            .windows(b"\"usage\"".len())
            .any(|window| window == b"\"usage\"")
        {
            return None;
        }
        if !matches!(self, Self::AnthropicMessages) {
            return self.parse_usage(body);
        }
        let event = serde_json::from_slice::<AnthropicStreamEvent>(body).ok()?;
        let partial = event
            .usage
            .or_else(|| event.message.and_then(|message| message.usage))?;
        let mut usage = current.unwrap_or_default();
        if partial.input_tokens.is_some()
            || partial.cache_creation_input_tokens.is_some()
            || partial.cache_read_input_tokens.is_some()
        {
            usage.input_tokens = partial
                .input_tokens
                .unwrap_or_default()
                .saturating_add(partial.cache_creation_input_tokens.unwrap_or_default())
                .saturating_add(partial.cache_read_input_tokens.unwrap_or_default());
            usage.cached_tokens = partial.cache_read_input_tokens.unwrap_or_default();
        }
        if let Some(output_tokens) = partial.output_tokens {
            usage.output_tokens = output_tokens;
        }
        usage.total_tokens = usage.input_tokens.saturating_add(usage.output_tokens);
        Some(usage)
    }
}

struct RequestMetadata {
    model: String,
    stream: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ExactUsage {
    input_tokens: u64,
    cached_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
}

impl From<OpenAiUsage> for ExactUsage {
    fn from(usage: OpenAiUsage) -> Self {
        Self {
            input_tokens: usage.prompt_tokens,
            cached_tokens: usage.cached_tokens(),
            output_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
        }
    }
}

impl From<OpenAiResponsesUsage> for ExactUsage {
    fn from(usage: OpenAiResponsesUsage) -> Self {
        Self {
            input_tokens: usage.input_tokens,
            cached_tokens: usage.cached_tokens(),
            output_tokens: usage.output_tokens,
            total_tokens: usage.total_tokens,
        }
    }
}

impl From<AnthropicUsage> for ExactUsage {
    fn from(usage: AnthropicUsage) -> Self {
        Self {
            input_tokens: usage.normalized_input_tokens(),
            cached_tokens: usage.cache_read_input_tokens,
            output_tokens: usage.output_tokens,
            total_tokens: usage.normalized_total_tokens(),
        }
    }
}

struct UsageTracker {
    state: GatewayState,
    backend_id: String,
    record: Option<UsageRequestRecord>,
    started: Instant,
    protocol: InferenceProtocol,
    exact_usage: Option<ExactUsage>,
    sse_parser: SseUsageParser,
    streaming: bool,
}

struct UsageRequestContext {
    request_id: String,
    client_app_id: String,
    requested_model: String,
    resolved_model: String,
    backend_id: String,
}

impl UsageTracker {
    fn new(
        state: GatewayState,
        context: UsageRequestContext,
        protocol: InferenceProtocol,
        streaming: bool,
    ) -> Self {
        let started_at_ms = unix_time_ms();
        Self {
            state,
            backend_id: context.backend_id.clone(),
            record: Some(UsageRequestRecord {
                request_id: context.request_id,
                client_app_id: context.client_app_id,
                protocol: protocol.usage_protocol().to_owned(),
                resolved_model: context.resolved_model,
                requested_model: context.requested_model,
                backend_id: context.backend_id,
                started_at_ms,
                first_token_at_ms: None,
                completed_at_ms: started_at_ms,
                input_tokens: None,
                cached_tokens: None,
                output_tokens: None,
                total_tokens: None,
                status: "cancelled".to_owned(),
                error_category: Some("client_cancelled".to_owned()),
                usage_accuracy: "unavailable".to_owned(),
            }),
            started: Instant::now(),
            protocol,
            exact_usage: None,
            sse_parser: SseUsageParser::new(protocol),
            streaming,
        }
    }

    fn observe_bytes(&mut self, bytes: &[u8]) {
        if let Some(record) = self.record.as_mut()
            && record.first_token_at_ms.is_none()
            && !bytes.is_empty()
        {
            record.first_token_at_ms = Some(
                record
                    .started_at_ms
                    .saturating_add(duration_ms_i64(self.started.elapsed())),
            );
        }
        if let Some(usage) = self.sse_parser.feed(bytes) {
            self.exact_usage = Some(usage);
        }
    }

    fn finish_http_status(&mut self, successful_status: bool) {
        if successful_status {
            self.finish("succeeded", None);
        } else {
            self.finish("failed", Some("backend_http_status"));
        }
    }

    fn finish_failed(&mut self, error_category: &'static str) {
        self.finish("failed", Some(error_category));
    }

    fn finish(&mut self, status: &'static str, error_category: Option<&'static str>) {
        let Some(mut record) = self.record.take() else {
            return;
        };
        record.completed_at_ms = record
            .started_at_ms
            .saturating_add(duration_ms_i64(self.started.elapsed()));
        record.status = status.to_owned();
        record.error_category = error_category.map(str::to_owned);
        if let Some(usage) = self.exact_usage.take() {
            record.input_tokens = Some(token_count_i64(usage.input_tokens));
            record.cached_tokens = Some(token_count_i64(usage.cached_tokens));
            record.output_tokens = Some(token_count_i64(usage.output_tokens));
            record.total_tokens = Some(token_count_i64(usage.total_tokens));
            record.usage_accuracy = if self.streaming {
                "exact_backend_event"
            } else {
                "exact_backend_response"
            }
            .to_owned();
        }
        self.state.record_usage(record);
    }

    fn finish_cancelled(&mut self) {
        self.finish("cancelled", Some("client_cancelled"));
    }

    fn finish_forced_route_switch(&mut self) {
        self.finish("failed", Some("forced_route_switch"));
    }

    fn record_backend_failure(&self) {
        self.state.record_backend_failure(&self.backend_id);
    }
}

struct GatewayBodyStream {
    upstream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    usage: UsageTracker,
    successful_status: bool,
    complete: bool,
    forwarded_bytes: usize,
    cancellation_wait: Pin<Box<dyn Future<Output = ()> + Send>>,
    _permit: OwnedSemaphorePermit,
    route_lease: BackendRequestLease,
}

impl GatewayBodyStream {
    fn new(
        upstream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
        usage: UsageTracker,
        successful_status: bool,
        permit: OwnedSemaphorePermit,
        route_lease: BackendRequestLease,
    ) -> Self {
        let cancellation_wait = Box::pin(route_lease.cancellation.clone().cancelled_owned());
        Self {
            upstream: Box::pin(upstream),
            usage,
            successful_status,
            complete: false,
            forwarded_bytes: 0,
            cancellation_wait,
            _permit: permit,
            route_lease,
        }
    }
}

impl Stream for GatewayBodyStream {
    type Item = Result<Bytes, io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.cancellation_wait.as_mut().poll(context).is_ready() {
            self.complete = true;
            self.usage.finish_forced_route_switch();
            return Poll::Ready(Some(Err(io::Error::other(
                "backend stream cancelled by forced route switch",
            ))));
        }
        match self.upstream.as_mut().poll_next(context) {
            Poll::Ready(Some(Ok(bytes))) => {
                if self.forwarded_bytes.saturating_add(bytes.len()) > MAX_STREAM_RESPONSE_BYTES {
                    self.complete = true;
                    self.usage.finish_failed("backend_response_too_large");
                    return Poll::Ready(Some(Err(io::Error::other(
                        "backend stream exceeded HAL100 limit",
                    ))));
                }
                self.forwarded_bytes = self.forwarded_bytes.saturating_add(bytes.len());
                self.usage.observe_bytes(&bytes);
                Poll::Ready(Some(Ok(bytes)))
            }
            Poll::Ready(Some(Err(_))) => {
                self.complete = true;
                self.usage.record_backend_failure();
                self.usage.finish_failed("backend_stream");
                Poll::Ready(Some(Err(io::Error::other("backend stream failed"))))
            }
            Poll::Ready(None) => {
                self.complete = true;
                let successful_status = self.successful_status;
                self.usage.finish_http_status(successful_status);
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for GatewayBodyStream {
    fn drop(&mut self) {
        if !self.complete {
            if self.route_lease.was_forced() {
                self.usage.finish_forced_route_switch();
            } else {
                self.usage.finish_cancelled();
            }
        }
    }
}

struct SseUsageParser {
    protocol: InferenceProtocol,
    pending: Vec<u8>,
    disabled: bool,
    latest_usage: Option<ExactUsage>,
}

impl SseUsageParser {
    fn new(protocol: InferenceProtocol) -> Self {
        Self {
            protocol,
            pending: Vec::new(),
            disabled: false,
            latest_usage: None,
        }
    }

    fn feed(&mut self, bytes: &[u8]) -> Option<ExactUsage> {
        if self.disabled {
            return None;
        }
        if self.pending.len().saturating_add(bytes.len()) > MAX_SSE_DATA_LINE_BYTES {
            self.pending.clear();
            self.disabled = true;
            return None;
        }
        self.pending.extend_from_slice(bytes);
        let mut latest_usage = None;
        let mut consumed = 0;
        let mut parsed_lines = 0;
        while let Some(relative_newline) = self.pending[consumed..]
            .iter()
            .position(|byte| *byte == b'\n')
        {
            parsed_lines += 1;
            if parsed_lines > MAX_SSE_LINES_PER_FEED {
                self.pending.clear();
                self.disabled = true;
                return latest_usage;
            }
            let newline = consumed + relative_newline;
            let mut line = &self.pending[consumed..newline];
            if line.last() == Some(&b'\r') {
                line = &line[..line.len() - 1];
            }
            consumed = newline + 1;
            let Some(data) = line.strip_prefix(b"data:") else {
                continue;
            };
            let data = data.strip_prefix(b" ").unwrap_or(data);
            if data == b"[DONE]" {
                continue;
            }
            if let Some(usage) = self.protocol.merge_stream_usage(data, self.latest_usage) {
                self.latest_usage = Some(usage);
                latest_usage = Some(usage);
            }
        }
        if consumed > 0 {
            self.pending.drain(..consumed);
        }
        latest_usage
    }
}

#[derive(Debug)]
enum GatewayError {
    Unauthorized,
    InvalidRequest(InferenceProtocol),
    BackendUnavailable,
    BackendConfiguration,
    BackendTransport,
    BackendStream,
    BackendProtocol,
    BackendResponseTooLarge,
    RequestTimeout,
    TooManyStreams,
    RouteDraining,
    BackendCircuitOpen,
    ForcedRouteSwitch,
}

impl GatewayError {
    fn parts(&self) -> (StatusCode, &'static str, &'static str) {
        match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "invalid_client_key",
                "缺少或无效的HAL100本地客户端凭据。",
            ),
            Self::InvalidRequest(protocol) => (
                StatusCode::BAD_REQUEST,
                "invalid_request",
                protocol.invalid_request_message(),
            ),
            Self::BackendUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "backend_unavailable",
                "HAL100尚未配置活动推理后端。",
            ),
            Self::BackendConfiguration => (
                StatusCode::BAD_GATEWAY,
                "backend_configuration",
                "推理后端地址配置无效。",
            ),
            Self::BackendTransport => (
                StatusCode::BAD_GATEWAY,
                "backend_transport",
                "HAL100无法连接推理后端。",
            ),
            Self::BackendStream => (
                StatusCode::BAD_GATEWAY,
                "backend_stream",
                "推理后端响应流异常终止。",
            ),
            Self::BackendProtocol => (
                StatusCode::BAD_GATEWAY,
                "backend_protocol",
                "推理后端返回了不兼容的协议响应。",
            ),
            Self::BackendResponseTooLarge => (
                StatusCode::BAD_GATEWAY,
                "backend_response_too_large",
                "推理后端响应超过HAL100大小限制。",
            ),
            Self::RequestTimeout => (
                StatusCode::REQUEST_TIMEOUT,
                "request_timeout",
                "请求体读取超时。",
            ),
            Self::TooManyStreams => (
                StatusCode::TOO_MANY_REQUESTS,
                "too_many_streams",
                "当前流式请求过多，请稍后重试。",
            ),
            Self::RouteDraining => (
                StatusCode::SERVICE_UNAVAILABLE,
                "route_draining",
                "当前活动后端正在排空请求，请稍后重试。",
            ),
            Self::BackendCircuitOpen => (
                StatusCode::SERVICE_UNAVAILABLE,
                "backend_circuit_open",
                "推理后端连续故障，HAL100已暂时停止向它发送新请求。",
            ),
            Self::ForcedRouteSwitch => (
                StatusCode::SERVICE_UNAVAILABLE,
                "forced_route_switch",
                "当前请求已由用户确认的强制路由切换中断。",
            ),
        }
    }

    fn anthropic_error_type(&self) -> &'static str {
        match self {
            Self::Unauthorized => "authentication_error",
            Self::InvalidRequest(_) => "invalid_request_error",
            Self::TooManyStreams => "rate_limit_error",
            Self::RequestTimeout => "timeout_error",
            Self::BackendUnavailable
            | Self::BackendConfiguration
            | Self::BackendTransport
            | Self::BackendStream
            | Self::BackendProtocol
            | Self::BackendResponseTooLarge
            | Self::RouteDraining
            | Self::BackendCircuitOpen
            | Self::ForcedRouteSwitch => "api_error",
        }
    }

    fn into_response_for(self, protocol: Option<InferenceProtocol>) -> Response {
        let protocol = protocol.or(match &self {
            Self::InvalidRequest(protocol) => Some(*protocol),
            _ => None,
        });
        let request_id = Uuid::new_v4().to_string();
        let (status, code, message) = self.parts();
        let mut response = if matches!(protocol, Some(InferenceProtocol::AnthropicMessages)) {
            (
                status,
                Json(AnthropicErrorEnvelope {
                    envelope_type: "error".to_owned(),
                    error: AnthropicError {
                        error_type: self.anthropic_error_type().to_owned(),
                        message: message.to_owned(),
                    },
                    request_id: request_id.clone(),
                }),
            )
                .into_response()
        } else {
            (
                status,
                Json(OpenAiErrorEnvelope {
                    error: OpenAiError {
                        message: message.to_owned(),
                        error_type: "hal100_gateway_error".to_owned(),
                        code: code.to_owned(),
                    },
                }),
            )
                .into_response()
        };
        response.headers_mut().insert(
            "x-hal100-request-id",
            HeaderValue::from_str(&request_id).expect("UUID is a valid header value"),
        );
        if matches!(protocol, Some(InferenceProtocol::AnthropicMessages)) {
            response.headers_mut().insert(
                "request-id",
                HeaderValue::from_str(&request_id).expect("UUID is a valid header value"),
            );
        }
        response
    }
}

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        self.into_response_for(None)
    }
}

fn unix_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, duration_ms_i64)
}

fn duration_ms_i64(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

fn duration_ms_u64(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn token_count_i64(tokens: u64) -> i64 {
    i64::try_from(tokens).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;
    use crate::{Database, stored_client_credential};

    const TEST_KEY: &str = "hal100_test_0123456789abcdef";

    #[test]
    fn mlc_llm_backend_normalizes_only_unary_structured_tool_arguments() {
        let backend = BackendConfig::new("mlc", "http://127.0.0.1:8000/v1/", None)
            .expect("MLC backend")
            .with_engine_kind(Some(InferenceEngineKind::MlcLlm));
        let normalized = backend
            .normalize_success_response(
                InferenceProtocol::OpenAiChatCompletions,
                Bytes::from_static(
                    br#"{"choices":[{"message":{"tool_calls":[{"function":{"name":"probe","arguments":{"value":"ok"}}}]}}],"usage":{"prompt_tokens":1,"completion_tokens":1}}"#,
                ),
            )
            .expect("normalize MLC response");
        let response = serde_json::from_slice::<serde_json::Value>(&normalized)
            .expect("normalized response JSON");
        let arguments = response["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .expect("OpenAI string arguments");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(arguments).expect("argument JSON"),
            serde_json::json!({"value":"ok"})
        );

        let strict = BackendConfig::new("strict", "http://127.0.0.1:8000/v1/", None)
            .expect("strict backend");
        let original = Bytes::from_static(br#"{"choices":[]}"#);
        assert_eq!(
            strict
                .normalize_success_response(
                    InferenceProtocol::OpenAiChatCompletions,
                    original.clone()
                )
                .expect("strict pass-through"),
            original
        );
    }

    #[test]
    fn mlc_llm_backend_rejects_unqualified_streaming_tool_calls() {
        let backend = BackendConfig::new("mlc", "http://127.0.0.1:8000/v1/", None)
            .expect("MLC backend")
            .with_engine_kind(Some(InferenceEngineKind::MlcLlm));
        let request = br#"{"model":"model","stream":true,"tools":[{"type":"function"}]}"#;
        assert!(matches!(
            backend.validate_protocol_request(
                InferenceProtocol::OpenAiChatCompletions,
                request,
                true,
            ),
            Err(GatewayError::InvalidRequest(
                InferenceProtocol::OpenAiChatCompletions
            ))
        ));
        assert!(
            backend
                .validate_protocol_request(
                    InferenceProtocol::OpenAiChatCompletions,
                    br#"{"model":"model","stream":true}"#,
                    true,
                )
                .is_ok()
        );
    }

    #[tokio::test]
    async fn health_endpoint_is_reachable_without_authentication() {
        let response = health_router()
            .oneshot(
                Request::get("/healthz")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("router response");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn protected_routes_reject_missing_credentials() {
        let response = test_router()
            .oneshot(
                Request::get("/v1/models")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("router response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("error body")
            .to_bytes();
        assert!(!String::from_utf8_lossy(&body).contains(TEST_KEY));
    }

    #[tokio::test]
    async fn messages_authentication_uses_anthropic_error_shape() {
        let response = test_router()
            .oneshot(
                Request::post("/v1/messages")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"model":"hal100-active","max_tokens":16,"messages":[]}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("router response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().contains_key("request-id"));
        let body = response
            .into_body()
            .collect()
            .await
            .expect("error body")
            .to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&body).expect("Anthropic error JSON");
        assert_eq!(body["type"], "error");
        assert_eq!(body["error"]["type"], "authentication_error");
        assert!(body["request_id"].as_str().is_some_and(|id| !id.is_empty()));
    }

    #[tokio::test]
    async fn conflicting_local_auth_headers_are_rejected() {
        let response = test_router()
            .oneshot(
                Request::post("/v1/messages")
                    .header(header::AUTHORIZATION, format!("Bearer {TEST_KEY}"))
                    .header("x-api-key", "a-different-key")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"model":"hal100-active","max_tokens":16,"messages":[]}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("router response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn authenticated_requests_fail_closed_without_a_backend() {
        let response = test_router()
            .oneshot(
                Request::get("/v1/models")
                    .header(header::AUTHORIZATION, format!("Bearer {TEST_KEY}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("router response");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn oversized_model_identifiers_are_rejected_before_accounting() {
        let request_body = serde_json::json!({
            "model": "m".repeat(MAX_MODEL_ID_BYTES + 1),
            "messages": [{"role": "user", "content": "hello"}]
        })
        .to_string();
        let response = test_router()
            .oneshot(
                Request::post("/v1/chat/completions")
                    .header(header::AUTHORIZATION, format!("Bearer {TEST_KEY}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(request_body))
                    .expect("request"),
            )
            .await
            .expect("router response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn parser_reads_usage_across_sse_chunk_boundaries() {
        let mut parser = SseUsageParser::new(InferenceProtocol::OpenAiChatCompletions);
        assert!(
            parser
                .feed(b"data: {\"usage\":{\"prompt_tokens\":4,")
                .is_none()
        );
        let usage = parser
            .feed(b"\"completion_tokens\":3,\"total_tokens\":7}}\n\n")
            .expect("usage");

        assert_eq!(usage.input_tokens, 4);
        assert_eq!(usage.output_tokens, 3);
        assert_eq!(usage.total_tokens, 7);
    }

    #[test]
    fn parser_handles_many_lines_with_one_final_compaction() {
        let mut input = b"\n".repeat(10_000);
        input.extend_from_slice(
            b"data: {\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":1,\"total_tokens\":3}}\n",
        );
        let usage = SseUsageParser::new(InferenceProtocol::OpenAiChatCompletions)
            .feed(&input)
            .expect("usage after dense lines");

        assert_eq!(usage.total_tokens, 3);
    }

    #[test]
    fn responses_parser_reads_nested_completed_event_usage() {
        let mut parser = SseUsageParser::new(InferenceProtocol::OpenAiResponses);
        let usage = parser
            .feed(
                b"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":14,\"output_tokens\":6,\"total_tokens\":20,\"input_tokens_details\":{\"cached_tokens\":3}}}}\n\n",
            )
            .expect("Responses completed usage");

        assert_eq!(usage.input_tokens, 14);
        assert_eq!(usage.cached_tokens, 3);
        assert_eq!(usage.output_tokens, 6);
        assert_eq!(usage.total_tokens, 20);
    }

    #[test]
    fn anthropic_parser_merges_message_start_and_cumulative_delta_usage() {
        let mut parser = SseUsageParser::new(InferenceProtocol::AnthropicMessages);
        let initial = parser
            .feed(
                b"data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":10,\"cache_creation_input_tokens\":4,\"cache_read_input_tokens\":20,\"output_tokens\":1}}}\n\n",
            )
            .expect("message_start usage");
        assert_eq!(initial.input_tokens, 34);
        assert_eq!(initial.cached_tokens, 20);
        assert_eq!(initial.output_tokens, 1);

        let final_usage = parser
            .feed(b"data: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":15}}\n\n")
            .expect("message_delta usage");
        assert_eq!(final_usage.input_tokens, 34);
        assert_eq!(final_usage.cached_tokens, 20);
        assert_eq!(final_usage.output_tokens, 15);
        assert_eq!(final_usage.total_tokens, 49);
    }

    #[test]
    fn backend_configuration_rejects_unsafe_urls_and_redacts_keys() {
        assert!(BackendConfig::new("backend", "file:///tmp/model", None).is_err());
        assert!(
            BackendConfig::new("backend", "http://user:password@localhost:8000/v1", None).is_err()
        );
        let backend = BackendConfig::new(
            "backend",
            "http://127.0.0.1:8000/v1",
            Some("backend-secret".to_owned()),
        )
        .expect("safe backend");
        assert_eq!(
            backend.endpoint("models").expect("models URL").as_str(),
            "http://127.0.0.1:8000/v1/models"
        );
        assert!(!format!("{backend:?}").contains("backend-secret"));
    }

    #[test]
    fn route_table_rejects_reserved_invalid_and_dangling_aliases() {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        let usage_writer = UsageWriter::start(database);
        let active = BackendConfig::new("active-backend", "http://127.0.0.1:8000/v1", None)
            .expect("active backend");
        let state = GatewayState::new(
            Some(active),
            CredentialRegistry::new(Vec::new()),
            usage_writer,
        )
        .expect("gateway state");

        assert_eq!(
            state.set_model_route("hal100-active", "active-backend", "model"),
            Err(GatewayRouteError::ReservedActiveAlias)
        );
        assert_eq!(
            state.set_model_route("bad alias", "active-backend", "model"),
            Err(GatewayRouteError::InvalidAlias)
        );
        assert_eq!(
            state.set_model_route("alias", "missing-backend", "model"),
            Err(GatewayRouteError::UnknownBackend)
        );
        assert_eq!(
            state.set_model_route("alias", "active-backend", ""),
            Err(GatewayRouteError::InvalidResolvedModel)
        );
        assert!(BackendConfig::new("unsafe backend", "http://127.0.0.1:8000/v1", None).is_err());

        state
            .set_model_route("stable-alias", "active-backend", "actual-model")
            .expect("route to active backend");
        assert_eq!(
            state.remove_routed_backend("active-backend"),
            Err(GatewayRouteError::BackendActive)
        );
        let snapshot = state.routing_snapshot();
        assert_eq!(
            snapshot.active_backend_id.as_deref(),
            Some("active-backend")
        );
        assert_eq!(snapshot.model_routes[0].alias, "stable-alias");
        assert_eq!(snapshot.model_routes[0].resolved_model, "actual-model");
    }

    #[tokio::test]
    async fn active_route_resolves_hal100_active_and_restores_as_one_value() {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        let usage_writer = UsageWriter::start(database);
        let original_backend = BackendConfig::new("original", "http://127.0.0.1:8000/v1", None)
            .expect("original backend");
        let state = GatewayState::new(None, CredentialRegistry::new(Vec::new()), usage_writer)
            .expect("gateway state");
        state.replace_active_route(Some(
            ActiveGatewayRoute::resolved(original_backend, "qwen3.5:9b")
                .expect("resolved original route"),
        ));

        let routed = state
            .route_backend("hal100-active")
            .expect("resolved active route");
        assert_eq!(routed.backend.id(), "original");
        assert_eq!(routed.resolved_model, "qwen3.5:9b");
        drop(routed);
        let snapshot = state.routing_snapshot();
        assert_eq!(snapshot.active_backend_id.as_deref(), Some("original"));
        assert_eq!(
            snapshot.active_resolved_model.as_deref(),
            Some("qwen3.5:9b")
        );

        let managed_backend = BackendConfig::new("managed", "http://127.0.0.1:8001/v1", None)
            .expect("managed backend");
        let previous = state
            .replace_active_route_when_idle(
                Some(ActiveGatewayRoute::passthrough(managed_backend)),
                Duration::from_secs(1),
            )
            .await
            .expect("switch to managed")
            .expect("previous complete route");
        assert_eq!(previous.backend().id(), "original");
        assert_eq!(previous.resolved_model(), Some("qwen3.5:9b"));
        assert_eq!(
            state
                .route_backend("hal100-active")
                .expect("managed passthrough")
                .resolved_model,
            "hal100-active"
        );

        state
            .replace_active_route_when_idle(Some(previous), Duration::from_secs(1))
            .await
            .expect("restore complete route");
        let restored = state
            .route_backend("hal100-active")
            .expect("restored external route");
        assert_eq!(restored.backend.id(), "original");
        assert_eq!(restored.resolved_model, "qwen3.5:9b");
    }

    #[tokio::test]
    async fn safe_active_backend_switch_drains_or_rolls_back_atomically() {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        let usage_writer = UsageWriter::start(database);
        let active = BackendConfig::new("old-backend", "http://127.0.0.1:8000/v1", None)
            .expect("old backend");
        let state = GatewayState::new(
            Some(active),
            CredentialRegistry::new(Vec::new()),
            usage_writer,
        )
        .expect("gateway state");
        let held_request = state.route_backend("hal100-active").expect("held route");
        let next = BackendConfig::new("new-backend", "http://127.0.0.1:8001/v1", None)
            .expect("new backend");

        assert!(matches!(
            state
                .replace_backend_when_idle(Some(next.clone()), Duration::from_millis(1))
                .await,
            Err(GatewayRouteSwitchError::DrainTimeout { active_requests: 1 })
        ));
        let still_old = state
            .route_backend("hal100-active")
            .expect("timeout restores old route");
        assert_eq!(still_old.backend.id(), "old-backend");
        drop(still_old);

        let switching_state = state.clone();
        let switch = tokio::spawn(async move {
            switching_state
                .replace_backend_when_idle(Some(next), Duration::from_secs(1))
                .await
        });
        loop {
            match state.route_backend("hal100-active") {
                Err(GatewayError::RouteDraining) => break,
                Ok(transient) => drop(transient),
                Err(error) => panic!("unexpected route error: {error:?}"),
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(
            state.backend_config().expect("old active").id(),
            "old-backend"
        );
        drop(held_request);

        let previous = switch
            .await
            .expect("switch task")
            .expect("drained switch")
            .expect("previous backend");
        assert_eq!(previous.id(), "old-backend");
        assert_eq!(
            state.backend_config().expect("new active").id(),
            "new-backend"
        );
        let routed = state.route_backend("hal100-active").expect("new route");
        assert_eq!(routed.backend.id(), "new-backend");
    }

    #[tokio::test]
    async fn circuit_breaker_opens_after_consecutive_backend_failures_and_recovers_lazily() {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        let usage_writer = UsageWriter::start(database);
        let active =
            BackendConfig::new("unstable", "http://127.0.0.1:8000/v1", None).expect("backend");
        let state = GatewayState::new(
            Some(active),
            CredentialRegistry::new(Vec::new()),
            usage_writer,
        )
        .expect("gateway state");

        for _ in 0..CIRCUIT_FAILURE_THRESHOLD {
            state.record_backend_failure("unstable");
        }
        assert!(matches!(
            state.route_backend("hal100-active"),
            Err(GatewayError::BackendCircuitOpen)
        ));
        let open = state.routing_snapshot();
        assert!(open.backend_health[0].circuit_open);
        assert_eq!(open.backend_health[0].consecutive_failures, 0);

        tokio::time::sleep(CIRCUIT_OPEN_DURATION + Duration::from_millis(5)).await;
        let recovered = state
            .route_backend("hal100-active")
            .expect("cooldown expires without a timer");
        drop(recovered);
        state.record_backend_success("unstable");
        let healthy = state.routing_snapshot();
        assert!(!healthy.backend_health[0].circuit_open);
        assert_eq!(healthy.backend_health[0].consecutive_failures, 0);
    }

    #[tokio::test]
    async fn force_switch_cancels_only_the_old_request_generation() {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        let usage_writer = UsageWriter::start(database);
        let active =
            BackendConfig::new("old", "http://127.0.0.1:8000/v1", None).expect("old backend");
        let state = GatewayState::new(
            Some(active),
            CredentialRegistry::new(Vec::new()),
            usage_writer,
        )
        .expect("gateway state");
        let old_request = state.route_backend("hal100-active").expect("old request");
        assert!(!old_request.lease.was_forced());
        let next =
            BackendConfig::new("new", "http://127.0.0.1:8001/v1", None).expect("new backend");

        let previous = state
            .force_replace_backend(Some(next))
            .await
            .expect("force switch")
            .expect("previous backend");
        assert_eq!(previous.id(), "old");
        assert!(old_request.lease.was_forced());
        let new_request = state.route_backend("hal100-active").expect("new request");
        assert_eq!(new_request.backend.id(), "new");
        assert!(!new_request.lease.was_forced());
    }

    fn test_router() -> Router {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        let usage_writer = UsageWriter::start(database);
        let credential =
            stored_client_credential("key-1", "client-1", "Test", TEST_KEY).expect("credential");
        let state = GatewayState::new(
            None,
            CredentialRegistry::new(vec![credential]),
            usage_writer,
        )
        .expect("gateway state");
        gateway_router(state)
    }
}
