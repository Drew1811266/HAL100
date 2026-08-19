use std::{
    convert::Infallible,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{StreamExt, stream};
use hal100_infra::{
    BackendAuthStyle, BackendConfig, CredentialRegistry, Database, GatewayState, UsageWriter,
    gateway_router, stored_client_credential,
};
use hal100_protocol::BackendProbeStatus;
use serde_json::{Value, json};

const LOCAL_CLIENT_KEY: &str = "hal100_local_0123456789abcdef";
const OPENCODE_CLIENT_KEY: &str = "hal100_opencode_0123456789abcdef0123456789abcdef";
const BACKEND_KEY: &str = "backend-secret-never-forward-local-key";
const ROUTED_BACKEND_KEY: &str = "routed-backend-secret";

#[tokio::test]
async fn opencode_tool_call_is_transparent_and_usage_is_attributed_to_opencode() {
    let harness =
        TestHarness::start_with_client("opencode-key", "opencode", "OpenCode", OPENCODE_CLIENT_KEY)
            .await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test client");
    let response = client
        .post(format!("{}/v1/chat/completions", harness.gateway_root))
        .bearer_auth(OPENCODE_CLIENT_KEY)
        .json(&json!({
            "model": "hal100-active",
            "messages": [
                {"role":"system","content":"OpenCode agent"},
                {"role":"user","content":"List the current directory"}
            ],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "bash",
                    "description": "Run a command",
                    "parameters": {
                        "type": "object",
                        "properties": {"command": {"type": "string"}},
                        "required": ["command"]
                    }
                }
            }],
            "tool_choice": "auto",
            "stream": false
        }))
        .send()
        .await
        .expect("OpenCode-shaped response");
    assert_eq!(response.status(), StatusCode::OK);
    let request_id = request_id(&response);
    let body = response.json::<Value>().await.expect("tool call JSON");
    assert_eq!(
        body["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
        "bash"
    );
    assert_eq!(
        body["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
        "{\"command\":\"pwd\"}"
    );

    harness
        .usage_writer
        .flush(Duration::from_secs(1))
        .expect("flush OpenCode usage");
    let usage = harness
        .database
        .usage_request(&request_id)
        .expect("OpenCode usage");
    assert_eq!(usage.client_app_id, "opencode");
    assert_eq!(usage.requested_model, "hal100-active");
    assert_eq!(usage.input_tokens, Some(21));
    assert_eq!(usage.output_tokens, Some(9));
    assert_eq!(usage.total_tokens, Some(30));
    assert_eq!(usage.usage_accuracy, "exact_backend_response");
}

#[tokio::test]
async fn proxies_models_and_chat_then_persists_exact_usage() {
    let harness = TestHarness::start().await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test client");

    let models = client
        .get(format!("{}/v1/models", harness.gateway_root))
        .bearer_auth(LOCAL_CLIENT_KEY)
        .send()
        .await
        .expect("models response");
    assert_eq!(models.status(), StatusCode::OK);
    assert_eq!(
        models.json::<Value>().await.expect("models JSON"),
        json!({"object":"list","data":[{"id":"mock-model","object":"model"}]})
    );

    let non_stream = client
        .post(format!("{}/v1/chat/completions", harness.gateway_root))
        .bearer_auth(LOCAL_CLIENT_KEY)
        .json(&json!({
            "model": "mock-model",
            "messages": [{"role":"user","content":"not persisted"}],
            "stream": false
        }))
        .send()
        .await
        .expect("non-stream response");
    assert_eq!(non_stream.status(), StatusCode::OK);
    let non_stream_request_id = request_id(&non_stream);
    let non_stream_body = non_stream.json::<Value>().await.expect("chat JSON");
    assert_eq!(non_stream_body["choices"][0]["message"]["content"], "pong");

    let streaming = client
        .post(format!("{}/v1/chat/completions", harness.gateway_root))
        .bearer_auth(LOCAL_CLIENT_KEY)
        .json(&json!({
            "model": "mock-model",
            "messages": [{"role":"user","content":"stream please"}],
            "stream": true,
            "stream_options": {"include_usage": true}
        }))
        .send()
        .await
        .expect("stream response");
    assert_eq!(streaming.status(), StatusCode::OK);
    assert_eq!(
        streaming
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/event-stream")
    );
    let streaming_request_id = request_id(&streaming);
    let streaming_body = streaming.text().await.expect("SSE body");
    assert!(streaming_body.contains("data: {\"choices\""));
    assert!(streaming_body.ends_with("data: [DONE]\n\n"));

    harness
        .usage_writer
        .flush(Duration::from_secs(1))
        .expect("flush usage");
    assert_eq!(
        harness.database.usage_request_count().expect("usage count"),
        2
    );
    let non_stream_usage = harness
        .database
        .usage_request(&non_stream_request_id)
        .expect("non-stream usage");
    assert_eq!(non_stream_usage.client_app_id, "integration-client");
    assert_eq!(non_stream_usage.input_tokens, Some(12));
    assert_eq!(non_stream_usage.cached_tokens, Some(2));
    assert_eq!(non_stream_usage.output_tokens, Some(5));
    assert_eq!(non_stream_usage.total_tokens, Some(17));
    assert_eq!(non_stream_usage.status, "succeeded");
    assert_eq!(non_stream_usage.usage_accuracy, "exact_backend_response");
    let stream_usage = harness
        .database
        .usage_request(&streaming_request_id)
        .expect("stream usage");
    assert_eq!(stream_usage.input_tokens, Some(8));
    assert_eq!(stream_usage.output_tokens, Some(4));
    assert_eq!(stream_usage.total_tokens, Some(12));
    assert_eq!(stream_usage.status, "succeeded");
    assert_eq!(stream_usage.usage_accuracy, "exact_backend_event");
}

#[tokio::test]
async fn retries_only_idempotent_models_requests_and_never_replays_inference_posts() {
    let state = RetryBackendState::default();
    let backend_router = Router::new()
        .route("/v1/models", get(retry_models))
        .route("/v1/chat/completions", post(never_retry_chat))
        .with_state(state.clone());
    let backend_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("backend listener");
    let backend_address = backend_listener.local_addr().expect("backend address");
    let backend_task = tokio::spawn(async move {
        axum::serve(backend_listener, backend_router)
            .await
            .expect("retry backend");
    });
    let database = Arc::new(Database::open_in_memory().expect("database"));
    let credential = stored_client_credential(
        "retry-key",
        "retry-client",
        "Retry client",
        LOCAL_CLIENT_KEY,
    )
    .expect("credential");
    let gateway_state = GatewayState::new(
        Some(
            BackendConfig::new(
                "retry-backend",
                &format!("http://{backend_address}/v1"),
                None,
            )
            .expect("backend config"),
        ),
        CredentialRegistry::new(vec![credential]),
        UsageWriter::start(database),
    )
    .expect("gateway");
    let gateway_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("gateway listener");
    let gateway_address = gateway_listener.local_addr().expect("gateway address");
    let probe_state = gateway_state.clone();
    let gateway_task = tokio::spawn(async move {
        axum::serve(gateway_listener, gateway_router(gateway_state))
            .await
            .expect("gateway server");
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("client");

    let models = client
        .get(format!("http://{gateway_address}/v1/models"))
        .bearer_auth(LOCAL_CLIENT_KEY)
        .send()
        .await
        .expect("models response");
    assert_eq!(models.status(), StatusCode::OK);
    assert_eq!(state.models_attempts.load(Ordering::Acquire), 2);

    let inference = client
        .post(format!("http://{gateway_address}/v1/chat/completions"))
        .bearer_auth(LOCAL_CLIENT_KEY)
        .json(&json!({
            "model": "mock-model",
            "messages": [{"role":"user","content":"do not replay"}]
        }))
        .send()
        .await
        .expect("inference response");
    assert_eq!(inference.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(state.chat_attempts.load(Ordering::Acquire), 1);
    let probe = probe_state
        .probe_backend("retry-backend")
        .await
        .expect("on-demand probe");
    assert_eq!(probe.status, BackendProbeStatus::Healthy);
    assert_eq!(probe.http_status, Some(200));
    assert_eq!(probe.model_count, Some(0));

    gateway_task.abort();
    backend_task.abort();
}

#[tokio::test]
async fn proxies_openai_responses_tools_and_tracks_response_and_event_usage() {
    let harness = TestHarness::start().await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test client");

    let non_stream = client
        .post(format!("{}/v1/responses", harness.gateway_root))
        .bearer_auth(LOCAL_CLIENT_KEY)
        .json(&json!({
            "model": "mock-model",
            "input": "Call the diagnostic tool",
            "tools": [{
                "type": "function",
                "name": "diagnose",
                "description": "Run a diagnostic",
                "parameters": {"type": "object", "properties": {}}
            }]
        }))
        .send()
        .await
        .expect("Responses non-stream response");
    assert_eq!(non_stream.status(), StatusCode::OK);
    let non_stream_request_id = request_id(&non_stream);
    let non_stream_body = non_stream.json::<Value>().await.expect("Responses JSON");
    assert_eq!(non_stream_body["output"][0]["type"], "function_call");
    assert_eq!(non_stream_body["output"][0]["name"], "diagnose");

    let streaming = client
        .post(format!("{}/v1/responses", harness.gateway_root))
        .bearer_auth(LOCAL_CLIENT_KEY)
        .json(&json!({
            "model": "mock-model",
            "input": "Stream a response",
            "stream": true
        }))
        .send()
        .await
        .expect("Responses stream response");
    assert_eq!(streaming.status(), StatusCode::OK);
    assert_eq!(
        streaming
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/event-stream")
    );
    let stream_request_id = request_id(&streaming);
    let stream_body = streaming.text().await.expect("Responses SSE body");
    assert!(stream_body.contains("response.output_text.delta"));
    assert!(stream_body.contains("response.completed"));

    harness
        .usage_writer
        .flush(Duration::from_secs(1))
        .expect("flush Responses usage");
    let non_stream_usage = harness
        .database
        .usage_request(&non_stream_request_id)
        .expect("non-stream Responses usage");
    assert_eq!(non_stream_usage.protocol, "openai_responses");
    assert_eq!(non_stream_usage.input_tokens, Some(31));
    assert_eq!(non_stream_usage.cached_tokens, Some(6));
    assert_eq!(non_stream_usage.output_tokens, Some(7));
    assert_eq!(non_stream_usage.total_tokens, Some(38));
    assert_eq!(non_stream_usage.usage_accuracy, "exact_backend_response");
    let stream_usage = harness
        .database
        .usage_request(&stream_request_id)
        .expect("stream Responses usage");
    assert_eq!(stream_usage.protocol, "openai_responses");
    assert_eq!(stream_usage.input_tokens, Some(14));
    assert_eq!(stream_usage.cached_tokens, Some(3));
    assert_eq!(stream_usage.output_tokens, Some(6));
    assert_eq!(stream_usage.total_tokens, Some(20));
    assert_eq!(stream_usage.status, "succeeded");
    assert_eq!(stream_usage.usage_accuracy, "exact_backend_event");
}

#[tokio::test]
async fn proxies_anthropic_messages_with_protocol_auth_tools_and_stream_usage() {
    let harness = TestHarness::start_anthropic().await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test client");

    let non_stream = client
        .post(format!("{}/v1/messages", harness.gateway_root))
        .header("x-api-key", LOCAL_CLIENT_KEY)
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", "tools-2024-05-16")
        .json(&json!({
            "model": "claude-compatible-model",
            "max_tokens": 128,
            "messages": [{"role": "user", "content": "Run diagnostics"}],
            "tools": [{
                "name": "diagnose",
                "description": "Run diagnostics",
                "input_schema": {"type": "object", "properties": {}}
            }]
        }))
        .send()
        .await
        .expect("Messages non-stream response");
    assert_eq!(non_stream.status(), StatusCode::OK);
    assert!(non_stream.headers().contains_key("request-id"));
    let non_stream_request_id = request_id(&non_stream);
    let non_stream_body = non_stream.json::<Value>().await.expect("Messages JSON");
    assert_eq!(non_stream_body["content"][0]["type"], "tool_use");
    assert_eq!(non_stream_body["content"][0]["name"], "diagnose");

    let streaming = client
        .post(format!("{}/v1/messages", harness.gateway_root))
        .header("x-api-key", LOCAL_CLIENT_KEY)
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": "claude-compatible-model",
            "max_tokens": 128,
            "messages": [{"role": "user", "content": "Stream a response"}],
            "stream": true
        }))
        .send()
        .await
        .expect("Messages stream response");
    assert_eq!(streaming.status(), StatusCode::OK);
    let stream_request_id = request_id(&streaming);
    let stream_body = streaming.text().await.expect("Messages SSE body");
    assert!(stream_body.contains("content_block_delta"));
    assert!(stream_body.contains("message_stop"));

    harness
        .usage_writer
        .flush(Duration::from_secs(1))
        .expect("flush Messages usage");
    let non_stream_usage = harness
        .database
        .usage_request(&non_stream_request_id)
        .expect("non-stream Messages usage");
    assert_eq!(non_stream_usage.protocol, "anthropic_messages");
    assert_eq!(non_stream_usage.input_tokens, Some(34));
    assert_eq!(non_stream_usage.cached_tokens, Some(20));
    assert_eq!(non_stream_usage.output_tokens, Some(6));
    assert_eq!(non_stream_usage.total_tokens, Some(40));
    let stream_usage = harness
        .database
        .usage_request(&stream_request_id)
        .expect("stream Messages usage");
    assert_eq!(stream_usage.protocol, "anthropic_messages");
    assert_eq!(stream_usage.input_tokens, Some(34));
    assert_eq!(stream_usage.cached_tokens, Some(20));
    assert_eq!(stream_usage.output_tokens, Some(15));
    assert_eq!(stream_usage.total_tokens, Some(49));
    assert_eq!(stream_usage.status, "succeeded");
    assert_eq!(stream_usage.usage_accuracy, "exact_backend_event");
}

#[tokio::test]
async fn explicit_model_alias_routes_to_a_second_backend_and_records_resolution() {
    let harness = TestHarness::start().await;
    let routed_router = Router::new().route("/v1/chat/completions", post(mock_routed_chat));
    let routed_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("routed backend listener");
    let routed_address = routed_listener
        .local_addr()
        .expect("routed backend address");
    let routed_task = tokio::spawn(async move {
        axum::serve(routed_listener, routed_router)
            .await
            .expect("routed backend server");
    });
    let routed_backend = BackendConfig::new(
        "secondary-backend",
        &format!("http://{routed_address}/v1"),
        Some(ROUTED_BACKEND_KEY.to_owned()),
    )
    .expect("routed backend config");
    harness
        .gateway_state
        .upsert_routed_backend(routed_backend)
        .expect("add routed backend");
    harness
        .gateway_state
        .set_model_route("fast-local", "secondary-backend", "actual-fast-model")
        .expect("add model route");

    let snapshot = harness.gateway_state.routing_snapshot();
    assert_eq!(snapshot.active_backend_id.as_deref(), Some("mock-backend"));
    assert_eq!(
        snapshot.backend_ids,
        vec!["mock-backend".to_owned(), "secondary-backend".to_owned()]
    );
    assert_eq!(snapshot.model_routes.len(), 1);
    assert_eq!(snapshot.model_routes[0].alias, "fast-local");

    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test client");
    let routed = client
        .post(format!("{}/v1/chat/completions", harness.gateway_root))
        .bearer_auth(LOCAL_CLIENT_KEY)
        .json(&json!({
            "model": "fast-local",
            "messages": [{"role": "user", "content": "route me"}]
        }))
        .send()
        .await
        .expect("routed response");
    assert_eq!(routed.status(), StatusCode::OK);
    let routed_request_id = request_id(&routed);
    let routed_body = routed.json::<Value>().await.expect("routed JSON");
    assert_eq!(routed_body["model"], "actual-fast-model");
    assert_eq!(
        routed_body["choices"][0]["message"]["content"],
        "secondary-pong"
    );

    let active = client
        .post(format!("{}/v1/chat/completions", harness.gateway_root))
        .bearer_auth(LOCAL_CLIENT_KEY)
        .json(&json!({
            "model": "mock-model",
            "messages": [{"role": "user", "content": "stay active"}]
        }))
        .send()
        .await
        .expect("active response");
    assert_eq!(active.status(), StatusCode::OK);
    let active_request_id = request_id(&active);
    assert_eq!(
        active.json::<Value>().await.expect("active JSON")["choices"][0]["message"]["content"],
        "pong"
    );

    harness
        .usage_writer
        .flush(Duration::from_secs(1))
        .expect("flush routed usage");
    let routed_usage = harness
        .database
        .usage_request(&routed_request_id)
        .expect("routed usage");
    assert_eq!(routed_usage.requested_model, "fast-local");
    assert_eq!(routed_usage.resolved_model, "actual-fast-model");
    assert_eq!(routed_usage.backend_id, "secondary-backend");
    assert_eq!(routed_usage.total_tokens, Some(50));
    let active_usage = harness
        .database
        .usage_request(&active_request_id)
        .expect("active usage");
    assert_eq!(active_usage.requested_model, "mock-model");
    assert_eq!(active_usage.resolved_model, "mock-model");
    assert_eq!(active_usage.backend_id, "mock-backend");

    assert_eq!(
        harness
            .gateway_state
            .remove_routed_backend("secondary-backend"),
        Err(hal100_infra::GatewayRouteError::BackendInUse)
    );
    assert!(
        harness
            .gateway_state
            .remove_model_route("fast-local")
            .expect("remove model route")
    );
    assert!(
        harness
            .gateway_state
            .remove_routed_backend("secondary-backend")
            .expect("remove routed backend")
    );
    routed_task.abort();
}

#[tokio::test]
async fn dropping_the_client_stream_cancels_upstream_and_records_cancellation() {
    let harness = TestHarness::start().await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test client");
    let response = client
        .post(format!("{}/v1/chat/completions", harness.gateway_root))
        .bearer_auth(LOCAL_CLIENT_KEY)
        .json(&json!({
            "model": "cancel-model",
            "messages": [{"role":"user","content":"cancel after one chunk"}],
            "stream": true
        }))
        .send()
        .await
        .expect("stream response");
    let request_id = request_id(&response);
    let mut body = response.bytes_stream();
    assert!(body.next().await.expect("first item").is_ok());
    drop(body);

    wait_until(Duration::from_secs(2), || {
        harness.backend_stream_cancelled.load(Ordering::Acquire)
    })
    .await;
    harness
        .usage_writer
        .flush(Duration::from_secs(1))
        .expect("flush usage");
    let usage = harness
        .database
        .usage_request(&request_id)
        .expect("cancelled usage");
    assert_eq!(usage.status, "cancelled");
    assert_eq!(usage.error_category.as_deref(), Some("client_cancelled"));
    assert_eq!(usage.usage_accuracy, "unavailable");
}

#[tokio::test]
async fn confirmed_force_switch_cancels_the_old_stream_and_marks_usage() {
    let harness = TestHarness::start().await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test client");
    let response = client
        .post(format!("{}/v1/chat/completions", harness.gateway_root))
        .bearer_auth(LOCAL_CLIENT_KEY)
        .json(&json!({
            "model": "cancel-model",
            "messages": [{"role":"user","content":"force switch after one chunk"}],
            "stream": true
        }))
        .send()
        .await
        .expect("stream response");
    let request_id = request_id(&response);
    let mut body = response.bytes_stream();
    assert!(body.next().await.expect("first item").is_ok());

    let replacement = BackendConfig::new(
        "replacement-backend",
        &format!("{}/v1", harness.backend_root),
        Some(BACKEND_KEY.to_owned()),
    )
    .expect("replacement backend");
    harness
        .gateway_state
        .force_replace_backend(Some(replacement))
        .await
        .expect("force route switch");
    assert!(body.next().await.is_some_and(|item| item.is_err()));

    wait_until(Duration::from_secs(2), || {
        harness.backend_stream_cancelled.load(Ordering::Acquire)
    })
    .await;
    harness
        .usage_writer
        .flush(Duration::from_secs(1))
        .expect("flush forced usage");
    let usage = harness
        .database
        .usage_request(&request_id)
        .expect("forced usage");
    assert_eq!(usage.status, "failed");
    assert_eq!(usage.error_category.as_deref(), Some("forced_route_switch"));
    assert_eq!(usage.usage_accuracy, "unavailable");
}

#[tokio::test]
async fn handles_twenty_concurrent_requests_without_losing_usage() {
    let harness = TestHarness::start().await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test client");
    let requests = (0..20).map(|index| {
        let client = client.clone();
        let endpoint = format!("{}/v1/chat/completions", harness.gateway_root);
        async move {
            let response = client
                .post(endpoint)
                .bearer_auth(LOCAL_CLIENT_KEY)
                .json(&json!({
                    "model": "mock-model",
                    "messages": [{"role":"user","content":format!("request {index}")}],
                    "stream": false
                }))
                .send()
                .await
                .expect("concurrent response");
            assert_eq!(response.status(), StatusCode::OK);
            response.bytes().await.expect("concurrent body");
        }
    });
    futures_util::future::join_all(requests).await;
    harness
        .usage_writer
        .flush(Duration::from_secs(1))
        .expect("flush usage");

    assert_eq!(
        harness.database.usage_request_count().expect("usage count"),
        20
    );
    assert_eq!(harness.usage_writer.dropped_record_count(), 0);
}

#[tokio::test]
async fn bounds_twenty_concurrent_streams_and_releases_every_stream_slot() {
    let harness = TestHarness::start().await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test client");
    let barrier = Arc::new(tokio::sync::Barrier::new(20));
    let requests = (0..20).map(|index| {
        let client = client.clone();
        let barrier = barrier.clone();
        let endpoint = format!("{}/v1/chat/completions", harness.gateway_root);
        async move {
            barrier.wait().await;
            let response = client
                .post(endpoint)
                .bearer_auth(LOCAL_CLIENT_KEY)
                .json(&json!({
                    "model": "concurrent-stream",
                    "messages": [{"role":"user","content":format!("stream {index}")}],
                    "stream": true,
                    "stream_options": {"include_usage": true}
                }))
                .send()
                .await
                .expect("concurrent stream response");
            let status = response.status();
            let body = response.text().await.expect("concurrent stream body");
            (status, body)
        }
    });
    let results = futures_util::future::join_all(requests).await;
    let succeeded = results
        .iter()
        .filter(|(status, body)| *status == StatusCode::OK && body.ends_with("data: [DONE]\n\n"))
        .count();
    let limited = results
        .iter()
        .filter(|(status, body)| {
            *status == StatusCode::TOO_MANY_REQUESTS && body.contains("too_many_streams")
        })
        .count();
    assert_eq!(succeeded, 16);
    assert_eq!(limited, 4);
    assert_eq!(
        harness.backend_max_active_streams.load(Ordering::Acquire),
        16
    );

    harness
        .usage_writer
        .flush(Duration::from_secs(1))
        .expect("flush concurrent stream usage");
    assert_eq!(
        harness.database.usage_request_count().expect("usage count"),
        16
    );

    let follow_up = client
        .post(format!("{}/v1/chat/completions", harness.gateway_root))
        .bearer_auth(LOCAL_CLIENT_KEY)
        .json(&json!({
            "model": "mock-model",
            "messages": [{"role":"user","content":"stream slots released"}],
            "stream": true
        }))
        .send()
        .await
        .expect("follow-up stream response");
    assert_eq!(follow_up.status(), StatusCode::OK);
    assert!(
        follow_up
            .text()
            .await
            .expect("follow-up stream body")
            .ends_with("data: [DONE]\n\n")
    );
}

#[tokio::test]
#[ignore = "local latency probe; run explicitly for performance records"]
async fn gateway_p95_overhead_stays_below_five_milliseconds() {
    let harness = TestHarness::start().await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test client");

    for _ in 0..10 {
        measured_chat(&client, &harness.backend_root, BACKEND_KEY).await;
        measured_chat(&client, &harness.gateway_root, LOCAL_CLIENT_KEY).await;
    }
    let mut backend_samples = Vec::with_capacity(200);
    let mut gateway_samples = Vec::with_capacity(200);
    for _ in 0..200 {
        backend_samples.push(measured_chat(&client, &harness.backend_root, BACKEND_KEY).await);
        gateway_samples.push(measured_chat(&client, &harness.gateway_root, LOCAL_CLIENT_KEY).await);
    }
    let backend_p95 = percentile_95(&mut backend_samples);
    let gateway_p95 = percentile_95(&mut gateway_samples);
    let overhead = gateway_p95.saturating_sub(backend_p95);
    println!(
        "backend_p95_us={} gateway_p95_us={} overhead_us={}",
        backend_p95.as_micros(),
        gateway_p95.as_micros(),
        overhead.as_micros()
    );
    assert!(overhead < Duration::from_millis(5));
}

struct TestHarness {
    gateway_root: String,
    backend_root: String,
    database: Arc<Database>,
    usage_writer: UsageWriter,
    gateway_state: GatewayState,
    backend_stream_cancelled: Arc<AtomicBool>,
    backend_max_active_streams: Arc<AtomicUsize>,
    gateway_task: tokio::task::JoinHandle<()>,
    backend_task: tokio::task::JoinHandle<()>,
}

impl TestHarness {
    async fn start() -> Self {
        Self::start_with_client(
            "integration-key",
            "integration-client",
            "Integration client",
            LOCAL_CLIENT_KEY,
        )
        .await
    }

    async fn start_with_client(
        key_id: &str,
        client_app_id: &str,
        display_name: &str,
        client_key: &str,
    ) -> Self {
        Self::start_with_client_and_backend_auth(
            key_id,
            client_app_id,
            display_name,
            client_key,
            BackendAuthStyle::Bearer,
        )
        .await
    }

    async fn start_anthropic() -> Self {
        Self::start_with_client_and_backend_auth(
            "integration-key",
            "integration-client",
            "Integration client",
            LOCAL_CLIENT_KEY,
            BackendAuthStyle::AnthropicApiKey,
        )
        .await
    }

    async fn start_with_client_and_backend_auth(
        key_id: &str,
        client_app_id: &str,
        display_name: &str,
        client_key: &str,
        backend_auth_style: BackendAuthStyle,
    ) -> Self {
        let backend_stream_cancelled = Arc::new(AtomicBool::new(false));
        let backend_active_streams = Arc::new(AtomicUsize::new(0));
        let backend_max_active_streams = Arc::new(AtomicUsize::new(0));
        let backend_state = MockBackendState {
            stream_cancelled: backend_stream_cancelled.clone(),
            active_streams: backend_active_streams,
            max_active_streams: backend_max_active_streams.clone(),
        };
        let backend_router = Router::new()
            .route("/v1/models", get(mock_models))
            .route("/v1/chat/completions", post(mock_chat))
            .route("/v1/responses", post(mock_responses))
            .route("/v1/messages", post(mock_messages))
            .with_state(backend_state);
        let backend_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("backend listener");
        let backend_address = backend_listener.local_addr().expect("backend address");
        let backend_task = tokio::spawn(async move {
            axum::serve(backend_listener, backend_router)
                .await
                .expect("mock backend server");
        });

        let database = Arc::new(Database::open_in_memory().expect("database"));
        let credential = stored_client_credential(key_id, client_app_id, display_name, client_key)
            .expect("client credential");
        database
            .upsert_client_credential(&credential, 1_700_000_000_000)
            .expect("persist credential hash");
        let credentials = CredentialRegistry::new(
            database
                .load_client_credentials()
                .expect("load credentials"),
        );
        let usage_writer = UsageWriter::start(database.clone());
        let backend = BackendConfig::new(
            "mock-backend",
            &format!("http://{backend_address}/v1"),
            Some(BACKEND_KEY.to_owned()),
        )
        .expect("backend config")
        .with_auth_style(backend_auth_style);
        let state = GatewayState::new(Some(backend), credentials, usage_writer.clone())
            .expect("gateway state");
        let gateway_state = state.clone();
        let gateway_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("gateway listener");
        let gateway_address = gateway_listener.local_addr().expect("gateway address");
        let gateway_task = tokio::spawn(async move {
            axum::serve(gateway_listener, gateway_router(state))
                .await
                .expect("gateway server");
        });

        Self {
            gateway_root: format!("http://{gateway_address}"),
            backend_root: format!("http://{backend_address}"),
            database,
            usage_writer,
            gateway_state,
            backend_stream_cancelled,
            backend_max_active_streams,
            gateway_task,
            backend_task,
        }
    }
}

async fn measured_chat(client: &reqwest::Client, root: &str, key: &str) -> Duration {
    let started = Instant::now();
    let response = client
        .post(format!("{root}/v1/chat/completions"))
        .bearer_auth(key)
        .json(&json!({
            "model": "mock-model",
            "messages": [{"role":"user","content":"latency probe"}],
            "stream": false
        }))
        .send()
        .await
        .expect("latency response");
    assert_eq!(response.status(), StatusCode::OK);
    response.bytes().await.expect("latency response body");
    started.elapsed()
}

fn percentile_95(samples: &mut [Duration]) -> Duration {
    samples.sort_unstable();
    samples[(samples.len() * 95).div_ceil(100).saturating_sub(1)]
}

impl Drop for TestHarness {
    fn drop(&mut self) {
        self.gateway_task.abort();
        self.backend_task.abort();
    }
}

#[derive(Clone)]
struct MockBackendState {
    stream_cancelled: Arc<AtomicBool>,
    active_streams: Arc<AtomicUsize>,
    max_active_streams: Arc<AtomicUsize>,
}

#[derive(Clone, Default)]
struct RetryBackendState {
    models_attempts: Arc<AtomicUsize>,
    chat_attempts: Arc<AtomicUsize>,
}

async fn retry_models(State(state): State<RetryBackendState>) -> Response {
    if state.models_attempts.fetch_add(1, Ordering::AcqRel) == 0 {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    Json(json!({"object":"list","data":[]})).into_response()
}

async fn never_retry_chat(State(state): State<RetryBackendState>) -> Response {
    state.chat_attempts.fetch_add(1, Ordering::AcqRel);
    StatusCode::SERVICE_UNAVAILABLE.into_response()
}

async fn mock_models(headers: HeaderMap) -> Response {
    if !has_backend_authorization(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(json!({
        "object": "list",
        "data": [{"id":"mock-model","object":"model"}]
    }))
    .into_response()
}

async fn mock_chat(
    State(state): State<MockBackendState>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response {
    if !has_backend_authorization(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let model = request["model"].as_str().unwrap_or_default();
    let streaming = request["stream"].as_bool().unwrap_or(false);
    if model == "cancel-model" {
        return cancellation_stream(state.stream_cancelled);
    }
    if model == "concurrent-stream" {
        return concurrent_usage_stream(state.active_streams, state.max_active_streams);
    }
    if request.get("tools").is_some() {
        return Json(json!({
            "id": "chatcmpl-tool",
            "object": "chat.completion",
            "model": model,
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "bash", "arguments": "{\"command\":\"pwd\"}"}
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 21,
                "completion_tokens": 9,
                "total_tokens": 30
            }
        }))
        .into_response();
    }
    if streaming {
        return finite_usage_stream();
    }
    Json(json!({
        "id": "chatcmpl-mock",
        "object": "chat.completion",
        "model": model,
        "choices": [{"index":0,"message":{"role":"assistant","content":"pong"},"finish_reason":"stop"}],
        "usage": {
            "prompt_tokens": 12,
            "completion_tokens": 5,
            "total_tokens": 17,
            "prompt_tokens_details": {"cached_tokens": 2}
        }
    }))
    .into_response()
}

async fn mock_responses(headers: HeaderMap, Json(request): Json<Value>) -> Response {
    if !has_backend_authorization(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let model = request["model"].as_str().unwrap_or_default();
    if request["stream"].as_bool().unwrap_or(false) {
        return finite_responses_usage_stream();
    }
    Json(json!({
        "id": "resp_mock",
        "object": "response",
        "status": "completed",
        "model": model,
        "output": [{
            "id": "fc_mock",
            "type": "function_call",
            "call_id": "call_mock",
            "name": "diagnose",
            "arguments": "{}",
            "status": "completed"
        }],
        "usage": {
            "input_tokens": 31,
            "input_tokens_details": {"cached_tokens": 6},
            "output_tokens": 7,
            "output_tokens_details": {"reasoning_tokens": 2},
            "total_tokens": 38
        }
    }))
    .into_response()
}

async fn mock_messages(headers: HeaderMap, Json(request): Json<Value>) -> Response {
    if !has_anthropic_backend_authorization(&headers)
        || headers
            .get("anthropic-version")
            .and_then(|value| value.to_str().ok())
            != Some("2023-06-01")
        || (request.get("tools").is_some()
            && headers
                .get("anthropic-beta")
                .and_then(|value| value.to_str().ok())
                != Some("tools-2024-05-16"))
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if request["stream"].as_bool().unwrap_or(false) {
        return finite_anthropic_usage_stream();
    }
    Json(json!({
        "id": "msg_mock",
        "type": "message",
        "role": "assistant",
        "model": request["model"],
        "content": [{
            "type": "tool_use",
            "id": "toolu_mock",
            "name": "diagnose",
            "input": {}
        }],
        "stop_reason": "tool_use",
        "stop_sequence": null,
        "usage": {
            "input_tokens": 10,
            "cache_creation_input_tokens": 4,
            "cache_read_input_tokens": 20,
            "output_tokens": 6
        }
    }))
    .into_response()
}

async fn mock_routed_chat(headers: HeaderMap, Json(request): Json<Value>) -> Response {
    let expected_authorization = format!("Bearer {ROUTED_BACKEND_KEY}");
    if headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        != Some(expected_authorization.as_str())
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let model = request["model"].as_str().unwrap_or_default();
    if model != "actual-fast-model" {
        return StatusCode::BAD_REQUEST.into_response();
    }
    Json(json!({
        "id": "chatcmpl-routed",
        "object": "chat.completion",
        "model": model,
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "secondary-pong"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 40, "completion_tokens": 10, "total_tokens": 50}
    }))
    .into_response()
}

fn finite_usage_stream() -> Response {
    let chunks = [
        "data: {\"choices\":[{\"delta\":{\"content\":\"po\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"ng\"}}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":8,\"completion_tokens\":4,\"total_tokens\":12}}\n\n",
        "data: [DONE]\n\n",
    ];
    let body = Body::from_stream(stream::iter(
        chunks.map(|chunk| Ok::<_, Infallible>(Bytes::from_static(chunk.as_bytes()))),
    ));
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(body)
        .expect("SSE response")
}

fn concurrent_usage_stream(
    active_streams: Arc<AtomicUsize>,
    max_active_streams: Arc<AtomicUsize>,
) -> Response {
    let active = active_streams.fetch_add(1, Ordering::AcqRel) + 1;
    max_active_streams.fetch_max(active, Ordering::AcqRel);
    let chunks = [
        Bytes::from_static(b"data: {\"choices\":[{\"delta\":{\"content\":\"po\"}}]}\n\n"),
        Bytes::from_static(b"data: {\"choices\":[{\"delta\":{\"content\":\"ng\"}}]}\n\n"),
        Bytes::from_static(b"data: {\"choices\":[],\"usage\":{\"prompt_tokens\":8,\"completion_tokens\":4,\"total_tokens\":12}}\n\n"),
        Bytes::from_static(b"data: [DONE]\n\n"),
    ];
    let body = Body::from_stream(stream::unfold(
        (0_usize, ActiveStreamGuard(active_streams)),
        move |(index, guard)| {
            let chunk = chunks.get(index).cloned();
            async move {
                let chunk = chunk?;
                tokio::time::sleep(Duration::from_millis(50)).await;
                Some((Ok::<_, Infallible>(chunk), (index + 1, guard)))
            }
        },
    ));
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(body)
        .expect("concurrent SSE response")
}

struct ActiveStreamGuard(Arc<AtomicUsize>);

impl Drop for ActiveStreamGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn finite_responses_usage_stream() -> Response {
    let chunks = [
        "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_stream\",\"status\":\"in_progress\"}}\n\n",
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"pong\",\"sequence_number\":1}\n\n",
        "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_stream\",\"status\":\"completed\",\"usage\":{\"input_tokens\":14,\"input_tokens_details\":{\"cached_tokens\":3},\"output_tokens\":6,\"total_tokens\":20}}}\n\n",
    ];
    let body = Body::from_stream(stream::iter(
        chunks.map(|chunk| Ok::<_, Infallible>(Bytes::from_static(chunk.as_bytes()))),
    ));
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(body)
        .expect("Responses SSE response")
}

fn finite_anthropic_usage_stream() -> Response {
    let chunks = [
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_stream\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-compatible-model\",\"stop_reason\":null,\"usage\":{\"input_tokens\":10,\"cache_creation_input_tokens\":4,\"cache_read_input_tokens\":20,\"output_tokens\":1}}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"pong\"}}\n\n",
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":15}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
    ];
    let body = Body::from_stream(stream::iter(
        chunks.map(|chunk| Ok::<_, Infallible>(Bytes::from_static(chunk.as_bytes()))),
    ));
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(body)
        .expect("Messages SSE response")
}

fn cancellation_stream(cancelled: Arc<AtomicBool>) -> Response {
    let stream = stream::unfold(
        (0_u8, DropSignal(cancelled)),
        |(index, signal)| async move {
            if index == 0 {
                return Some((
                    Ok::<_, Infallible>(Bytes::from_static(
                        b"data: {\"choices\":[{\"delta\":{\"content\":\"first\"}}]}\n\n",
                    )),
                    (1, signal),
                ));
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
            Some((
                Ok::<_, Infallible>(Bytes::from_static(b"data: [DONE]\n\n")),
                (2, signal),
            ))
        },
    );
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(stream))
        .expect("cancellation response")
}

struct DropSignal(Arc<AtomicBool>);

impl Drop for DropSignal {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

fn has_backend_authorization(headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        == Some(&format!("Bearer {BACKEND_KEY}"))
}

fn has_anthropic_backend_authorization(headers: &HeaderMap) -> bool {
    headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
        == Some(BACKEND_KEY)
        && !headers.contains_key(header::AUTHORIZATION)
}

fn request_id(response: &reqwest::Response) -> String {
    response
        .headers()
        .get("x-hal100-request-id")
        .and_then(|value| value.to_str().ok())
        .expect("HAL100 request ID")
        .to_owned()
}

async fn wait_until(timeout: Duration, predicate: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + timeout;
    while !predicate() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "condition timed out"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
