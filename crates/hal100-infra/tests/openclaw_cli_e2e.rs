use std::{
    collections::HashSet,
    convert::Infallible,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::{Arc, Mutex},
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
use futures_util::stream;
use hal100_infra::{
    BackendConfig, CredentialRegistry, Database, ExternalModelProfileRegistry, GatewayState,
    OpenClawIntegrationAdapter, OpenClawPaths, UsageWriter, gateway_router,
};
use hal100_protocol::ExternalAgentGatewayProtocol;
use serde_json::{Value, json};
use uuid::Uuid;

const BACKEND_KEY: &str = "openclaw-real-cli-backend-key";
const OPENCLAW_PACKAGE: &str = "openclaw@2026.7.1-2";
const EXPECTED_REPLY: &str = "HAL100_OPENCLAW_E2E_OK";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "downloads/runs the pinned official OpenClaw CLI in an isolated HOME across three protocols"]
async fn official_openclaw_cli_uses_each_managed_protocol_and_records_isolated_usage() {
    let package = selected_openclaw_package();
    let temp = TestDirectory::new();
    let project = temp.0.join("project");
    let fake_home = temp.0.join("home");
    let app_data = temp.0.join("hal100-data");
    let install_directory = temp.0.join("official-openclaw");
    fs::create_dir_all(&project).expect("project directory");
    fs::create_dir_all(&fake_home).expect("fake HOME");
    let binary = tokio::task::spawn_blocking(move || {
        install_official_openclaw(&package, &install_directory)
    })
    .await
    .expect("OpenClaw installation task")
    .expect("install official OpenClaw package");

    let captured = Arc::new(Mutex::new(Vec::new()));
    let backend_router = Router::new()
        .route("/v1/models", get(mock_models))
        .route("/v1/chat/completions", post(mock_chat))
        .route("/v1/responses", post(mock_responses))
        .route("/v1/messages", post(mock_messages))
        .with_state(MockBackendState {
            requests: captured.clone(),
        });
    let backend_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock backend listener");
    let backend_address = backend_listener.local_addr().expect("backend address");
    let backend_task = tokio::spawn(async move {
        axum::serve(backend_listener, backend_router)
            .await
            .expect("mock backend");
    });

    let database = Arc::new(Database::open(temp.0.join("hal100.sqlite")).expect("database"));
    let credentials = CredentialRegistry::new(Vec::new());
    let gateway_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("HAL100 gateway listener");
    let gateway_address = gateway_listener.local_addr().expect("gateway address");
    let gateway_base_url = format!("http://{gateway_address}/v1");
    let state_directory = fake_home.join(".openclaw");
    let config_path = state_directory.join("openclaw.json");
    let runtime_directory = find_executable_in_path("node")
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .expect("Node.js must be available for the official OpenClaw package");
    let manager = OpenClawIntegrationAdapter::with_gateway_base_url(
        database.clone(),
        credentials.clone(),
        ExternalModelProfileRegistry::conservative_managed_route(),
        OpenClawPaths {
            home_directory: fake_home.clone(),
            state_directory: state_directory.clone(),
            config_path: config_path.clone(),
            credential_path: app_data.join("credentials/openclaw-gateway.key"),
            temporary_directory: app_data.join("integration-plans/openclaw"),
            binary_candidates: vec![binary.clone()],
            runtime_directories: vec![runtime_directory],
        },
        gateway_base_url,
    );

    let usage_writer = UsageWriter::start(database.clone());
    let backend = BackendConfig::new(
        "official-openclaw-mock-backend",
        &format!("http://{backend_address}/v1"),
        Some(BACKEND_KEY.to_owned()),
    )
    .expect("backend configuration");
    let gateway =
        GatewayState::new(Some(backend), credentials, usage_writer.clone()).expect("gateway state");
    let gateway_task = tokio::spawn(async move {
        axum::serve(gateway_listener, gateway_router(gateway))
            .await
            .expect("HAL100 gateway");
    });

    for protocol in [
        ExternalAgentGatewayProtocol::OpenAiChatCompletions,
        ExternalAgentGatewayProtocol::OpenAiResponses,
        ExternalAgentGatewayProtocol::AnthropicMessages,
    ] {
        let plan = manager
            .plan_configuration(protocol)
            .unwrap_or_else(|error| {
                panic!("official OpenClaw dry-run failed for {protocol:?}: {error}")
            });
        manager
            .apply_configuration(&plan.plan_id)
            .unwrap_or_else(|error| panic!("OpenClaw apply failed for {protocol:?}: {error}"));
        let binary = binary.clone();
        let fake_home = fake_home.clone();
        let project = project.clone();
        let state_directory = state_directory.clone();
        let config_path = config_path.clone();
        let output = tokio::task::spawn_blocking(move || {
            run_official_openclaw(
                &binary,
                &fake_home,
                &project,
                &state_directory,
                &config_path,
            )
        })
        .await
        .expect("OpenClaw inference task")
        .expect("run official OpenClaw inference");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "OpenClaw {protocol:?} exited unsuccessfully\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(
            stdout.contains(EXPECTED_REPLY),
            "OpenClaw {protocol:?} output missed the mock reply\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }

    usage_writer
        .flush(Duration::from_secs(3))
        .expect("flush OpenClaw usage");
    assert!(
        database
            .usage_request_count_for_client("openclaw")
            .expect("OpenClaw usage count")
            >= 3
    );
    assert_eq!(
        database
            .usage_request_count_for_client("hal100-agent")
            .expect("built-in Agent usage count"),
        0,
        "external OpenClaw must never be attributed to HAL100's built-in Agent runtime"
    );
    let requests = captured.lock().expect("captured requests");
    let protocols = requests
        .iter()
        .map(|request| request.protocol)
        .collect::<HashSet<_>>();
    assert_eq!(
        protocols,
        HashSet::from(["chat", "responses", "anthropic"]),
        "the real OpenClaw client must reach every configured Gateway protocol"
    );
    assert!(
        requests
            .iter()
            .all(|request| request.body["model"] == "hal100-active")
    );

    gateway_task.abort();
    backend_task.abort();
}

fn install_official_openclaw(package: &str, directory: &Path) -> std::io::Result<PathBuf> {
    fs::create_dir_all(directory)?;
    let output = Command::new("pnpm")
        .args(["add", "--dir"])
        .arg(directory)
        .args(["--save-exact", "--ignore-scripts", package])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "OpenClaw package installation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let binary = directory.join("node_modules/.bin/openclaw");
    if !binary.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "OpenClaw package did not expose its official CLI",
        ));
    }
    Ok(binary)
}

fn run_official_openclaw(
    binary: &Path,
    fake_home: &Path,
    project: &Path,
    state_directory: &Path,
    config_path: &Path,
) -> std::io::Result<Output> {
    fs::create_dir_all(fake_home.join("tmp"))?;
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut command = Command::new(binary);
    command
        .args([
            "infer",
            "model",
            "run",
            "--local",
            "--model",
            "hal100/hal100-active",
            "--prompt",
            "Reply with exactly HAL100_OPENCLAW_E2E_OK.",
            "--json",
        ])
        .current_dir(project)
        .env_clear()
        .env("PATH", path)
        .env("HOME", fake_home)
        .env("TMPDIR", fake_home.join("tmp"))
        .env("OPENCLAW_HOME", fake_home)
        .env("OPENCLAW_STATE_DIR", state_directory)
        .env("OPENCLAW_CONFIG_PATH", config_path)
        .env("OPENCLAW_OFFLINE", "1")
        .env("OPENCLAW_NO_AUTO_UPDATE", "1")
        .env("OPENCLAW_LOAD_SHELL_ENV", "0")
        .env("OPENCLAW_EXEC_SHELL_SNAPSHOT", "0")
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("NO_COLOR", "1")
        .env("CI", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    run_with_timeout(command, Duration::from_secs(120))
}

fn selected_openclaw_package() -> String {
    let package = std::env::var("HAL100_OPENCLAW_TEST_PACKAGE")
        .unwrap_or_else(|_| OPENCLAW_PACKAGE.to_owned());
    let Some(version) = package.strip_prefix("openclaw@") else {
        panic!("HAL100_OPENCLAW_TEST_PACKAGE must be an exact openclaw version");
    };
    assert!(
        !version.is_empty()
            && version
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'-')),
        "HAL100_OPENCLAW_TEST_PACKAGE must be an exact version"
    );
    package
}

fn run_with_timeout(mut command: Command, timeout: Duration) -> std::io::Result<Output> {
    let mut child = command.spawn()?;
    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait()?.is_some() {
            return child.wait_with_output();
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child.wait_with_output()?;
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!(
                    "OpenClaw timed out; stderr: {}",
                    String::from_utf8_lossy(&output.stderr)
                ),
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn find_executable_in_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

#[derive(Debug, Clone)]
struct CapturedRequest {
    protocol: &'static str,
    body: Value,
}

#[derive(Clone)]
struct MockBackendState {
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
}

async fn mock_models(headers: HeaderMap) -> Response {
    if !has_backend_key(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(json!({
        "object": "list",
        "data": [{"id": "hal100-active", "object": "model", "owned_by": "hal100"}]
    }))
    .into_response()
}

async fn mock_chat(
    State(state): State<MockBackendState>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response {
    if !has_backend_key(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    state
        .requests
        .lock()
        .expect("capture")
        .push(CapturedRequest {
            protocol: "chat",
            body: request.clone(),
        });
    if request["stream"].as_bool().unwrap_or(false) {
        return sse_response(&[
            "data: {\"id\":\"chatcmpl-openclaw\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"hal100-active\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"HAL100_OPENCLAW_E2E_OK\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-openclaw\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"hal100-active\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":13,\"completion_tokens\":4,\"total_tokens\":17}}\n\n",
            "data: [DONE]\n\n",
        ]);
    }
    Json(json!({
        "id": "chatcmpl-openclaw",
        "object": "chat.completion",
        "model": "hal100-active",
        "choices": [{"index": 0, "message": {"role": "assistant", "content": EXPECTED_REPLY}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 13, "completion_tokens": 4, "total_tokens": 17}
    }))
    .into_response()
}

async fn mock_responses(
    State(state): State<MockBackendState>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response {
    if !has_backend_key(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    state
        .requests
        .lock()
        .expect("capture")
        .push(CapturedRequest {
            protocol: "responses",
            body: request.clone(),
        });
    if request["stream"].as_bool().unwrap_or(false) {
        return sse_response(&[
            "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_openclaw\",\"object\":\"response\",\"status\":\"in_progress\",\"model\":\"hal100-active\",\"output\":[]}}\n\n",
            "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"msg_openclaw\",\"type\":\"message\",\"status\":\"in_progress\",\"role\":\"assistant\",\"content\":[]},\"sequence_number\":1}\n\n",
            "event: response.content_part.added\ndata: {\"type\":\"response.content_part.added\",\"item_id\":\"msg_openclaw\",\"output_index\":0,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\",\"annotations\":[]},\"sequence_number\":2}\n\n",
            "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_openclaw\",\"output_index\":0,\"content_index\":0,\"delta\":\"HAL100_OPENCLAW_E2E_OK\",\"sequence_number\":3}\n\n",
            "event: response.output_text.done\ndata: {\"type\":\"response.output_text.done\",\"item_id\":\"msg_openclaw\",\"output_index\":0,\"content_index\":0,\"text\":\"HAL100_OPENCLAW_E2E_OK\",\"sequence_number\":4}\n\n",
            "event: response.content_part.done\ndata: {\"type\":\"response.content_part.done\",\"item_id\":\"msg_openclaw\",\"output_index\":0,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"HAL100_OPENCLAW_E2E_OK\",\"annotations\":[]},\"sequence_number\":5}\n\n",
            "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"msg_openclaw\",\"type\":\"message\",\"status\":\"completed\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"HAL100_OPENCLAW_E2E_OK\",\"annotations\":[]}]},\"sequence_number\":6}\n\n",
            "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_openclaw\",\"object\":\"response\",\"status\":\"completed\",\"model\":\"hal100-active\",\"output\":[{\"id\":\"msg_openclaw\",\"type\":\"message\",\"status\":\"completed\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"HAL100_OPENCLAW_E2E_OK\",\"annotations\":[]}]}],\"usage\":{\"input_tokens\":14,\"output_tokens\":6,\"total_tokens\":20}}}\n\n",
        ]);
    }
    Json(json!({
        "id": "resp_openclaw",
        "object": "response",
        "status": "completed",
        "model": "hal100-active",
        "output": [{"id": "msg_openclaw", "type": "message", "status": "completed", "role": "assistant", "content": [{"type": "output_text", "text": EXPECTED_REPLY, "annotations": []}]}],
        "usage": {"input_tokens": 14, "output_tokens": 6, "total_tokens": 20}
    }))
    .into_response()
}

async fn mock_messages(
    State(state): State<MockBackendState>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response {
    if !has_backend_key(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    state
        .requests
        .lock()
        .expect("capture")
        .push(CapturedRequest {
            protocol: "anthropic",
            body: request.clone(),
        });
    if request["stream"].as_bool().unwrap_or(false) {
        return sse_response(&[
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_openclaw\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"hal100-active\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":1}}}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"HAL100_OPENCLAW_E2E_OK\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":5}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        ]);
    }
    Json(json!({
        "id": "msg_openclaw",
        "type": "message",
        "role": "assistant",
        "model": "hal100-active",
        "content": [{"type": "text", "text": EXPECTED_REPLY}],
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {"input_tokens": 10, "output_tokens": 5}
    }))
    .into_response()
}

fn sse_response(chunks: &[&'static str]) -> Response {
    let chunks = chunks
        .iter()
        .copied()
        .map(|chunk| Ok::<_, Infallible>(Bytes::from_static(chunk.as_bytes())))
        .collect::<Vec<_>>();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(stream::iter(chunks)))
        .expect("SSE response")
}

fn has_backend_key(headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        == Some(&format!("Bearer {BACKEND_KEY}"))
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("hal100-openclaw-cli-{}", Uuid::new_v4()));
        fs::create_dir(&path).expect("create test directory");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
