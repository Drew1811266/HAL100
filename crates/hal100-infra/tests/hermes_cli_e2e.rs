use std::{
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
    HermesAgentIntegrationAdapter, HermesAgentPaths, UsageWriter, gateway_router,
};
use hal100_protocol::{ExternalAgentInputModality, ExternalAgentModelProfile};
use serde_json::{Value, json};
use uuid::Uuid;

const BACKEND_KEY: &str = "hermes-real-cli-backend-key";
const HERMES_PACKAGE: &str = "hermes-agent==0.18.2";
const EXPECTED_REPLY: &str = "HAL100_HERMES_E2E_OK";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "downloads/runs the pinned official Hermes Agent CLI in an isolated HOME"]
async fn official_hermes_cli_uses_managed_provider_and_records_isolated_usage() {
    let package = selected_hermes_package();
    let temp = TestDirectory::new();
    let project = temp.0.join("project");
    let fake_home = temp.0.join("home");
    let app_data = temp.0.join("hal100-data");
    let install_directory = temp.0.join("official-hermes");
    fs::create_dir_all(&project).expect("project directory");
    fs::create_dir_all(&fake_home).expect("fake HOME");
    let binary =
        tokio::task::spawn_blocking(move || install_official_hermes(&package, &install_directory))
            .await
            .expect("Hermes installation task")
            .expect("install official Hermes package");

    let captured_requests = Arc::new(Mutex::new(Vec::new()));
    let backend_router = Router::new()
        .route("/v1/models", get(mock_models))
        .route("/v1/chat/completions", post(mock_chat))
        .with_state(MockBackendState {
            requests: captured_requests.clone(),
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
        .expect("HAL100 isolated gateway listener");
    let gateway_address = gateway_listener.local_addr().expect("gateway address");
    let gateway_base_url = format!("http://{gateway_address}/v1");
    let hermes_directory = fake_home.join(".hermes");
    let manager = HermesAgentIntegrationAdapter::with_gateway_base_url(
        database.clone(),
        credentials.clone(),
        ExternalModelProfileRegistry::new(ExternalAgentModelProfile {
            model_id: "hal100-active".to_owned(),
            display_name: "HAL100 Hermes验收模型".to_owned(),
            context_window_tokens: 65_536,
            max_output_tokens: 4_096,
            input_modalities: vec![ExternalAgentInputModality::Text],
            supports_tools: true,
            supports_reasoning: false,
            revision: "hermes-e2e-route-v1".to_owned(),
        })
        .expect("Hermes-capable model profile"),
        HermesAgentPaths {
            home_directory: fake_home.clone(),
            config_path: hermes_directory.join("config.yaml"),
            environment_path: hermes_directory.join(".env"),
            hermes_directory: hermes_directory.clone(),
            validation_root: app_data.join("temporary/hermes-validation"),
            binary_candidates: vec![binary.clone()],
        },
        gateway_base_url,
    );
    let plan = manager
        .plan_configuration()
        .expect("Hermes configuration plan and official config-show validation");
    manager
        .apply_configuration(&plan.plan_id)
        .expect("confirmed Hermes configuration");

    let usage_writer = UsageWriter::start(database.clone());
    let backend = BackendConfig::new(
        "official-hermes-mock-backend",
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

    let command_output = tokio::task::spawn_blocking(move || {
        run_official_hermes(&binary, &fake_home, &project, &hermes_directory)
    })
    .await
    .expect("Hermes inference task")
    .expect("run official Hermes CLI");
    let stdout = String::from_utf8_lossy(&command_output.stdout);
    let stderr = String::from_utf8_lossy(&command_output.stderr);
    assert!(
        command_output.status.success(),
        "Hermes exited unsuccessfully\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains(EXPECTED_REPLY),
        "Hermes output did not contain the mock response\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    usage_writer
        .flush(Duration::from_secs(3))
        .expect("flush Hermes usage");
    assert!(
        database
            .usage_request_count_for_client("hermes-agent")
            .expect("Hermes usage count")
            >= 1
    );
    assert_eq!(
        database
            .usage_request_count_for_client("hal100-agent")
            .expect("built-in Agent usage count"),
        0,
        "external Hermes must never be attributed to HAL100's built-in Agent runtime"
    );
    let requests = captured_requests.lock().expect("captured requests");
    assert!(requests.iter().any(|request| {
        request["model"] == "hal100-active"
            && request["messages"]
                .as_array()
                .is_some_and(|messages| !messages.is_empty())
    }));

    gateway_task.abort();
    backend_task.abort();
}

fn install_official_hermes(package: &str, directory: &Path) -> std::io::Result<PathBuf> {
    let venv = directory.join("venv");
    fs::create_dir_all(directory)?;
    let create = Command::new("uv")
        .args(["venv", "--python", "3.12"])
        .arg(&venv)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;
    if !create.status.success() {
        return Err(std::io::Error::other(format!(
            "Hermes virtual environment creation failed: {}",
            String::from_utf8_lossy(&create.stderr)
        )));
    }
    let python = venv.join("bin/python");
    let install = Command::new("uv")
        .args(["pip", "install", "--python"])
        .arg(&python)
        .arg(package)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;
    if !install.status.success() {
        return Err(std::io::Error::other(format!(
            "Hermes package installation failed: {}",
            String::from_utf8_lossy(&install.stderr)
        )));
    }
    let binary = venv.join("bin/hermes");
    if !binary.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Hermes package did not expose its official CLI",
        ));
    }
    Ok(binary)
}

fn run_official_hermes(
    binary: &Path,
    fake_home: &Path,
    project: &Path,
    hermes_directory: &Path,
) -> std::io::Result<Output> {
    fs::create_dir_all(fake_home.join("tmp"))?;
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut command = Command::new(binary);
    command
        .args([
            "-p",
            "default",
            "-z",
            "Reply with exactly HAL100_HERMES_E2E_OK.",
            "--provider",
            "custom:hal100",
            "--model",
            "hal100-active",
            "--ignore-rules",
        ])
        .current_dir(project)
        .env_clear()
        .env("PATH", path)
        .env("HOME", fake_home)
        .env("HERMES_REAL_HOME", fake_home)
        .env("HERMES_HOME", hermes_directory)
        .env("TMPDIR", fake_home.join("tmp"))
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .env("CI", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    run_with_timeout(command, Duration::from_secs(180))
}

fn selected_hermes_package() -> String {
    let package =
        std::env::var("HAL100_HERMES_TEST_PACKAGE").unwrap_or_else(|_| HERMES_PACKAGE.to_owned());
    let Some(version) = package.strip_prefix("hermes-agent==") else {
        panic!("HAL100_HERMES_TEST_PACKAGE must be an exact hermes-agent version");
    };
    assert!(
        !version.is_empty()
            && version
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.'),
        "HAL100_HERMES_TEST_PACKAGE must be an exact stable version"
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
                    "Hermes timed out; stderr: {}",
                    String::from_utf8_lossy(&output.stderr)
                ),
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[derive(Clone)]
struct MockBackendState {
    requests: Arc<Mutex<Vec<Value>>>,
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
        .expect("request capture")
        .push(request.clone());
    if request["stream"].as_bool().unwrap_or(false) {
        return sse_response(&[
            "data: {\"id\":\"chatcmpl-hal100-hermes\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"hal100-active\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"HAL100_HERMES_E2E_OK\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-hal100-hermes\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"hal100-active\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":13,\"completion_tokens\":4,\"total_tokens\":17}}\n\n",
            "data: [DONE]\n\n",
        ]);
    }
    Json(json!({
        "id": "chatcmpl-hal100-hermes",
        "object": "chat.completion",
        "model": "hal100-active",
        "choices": [{"index": 0, "message": {"role": "assistant", "content": EXPECTED_REPLY}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 13, "completion_tokens": 4, "total_tokens": 17}
    }))
    .into_response()
}

fn sse_response(chunks: &[&'static str]) -> Response {
    let chunks = chunks
        .iter()
        .map(|chunk| Bytes::from_static(chunk.as_bytes()))
        .collect::<Vec<_>>();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(stream::iter(
            chunks.into_iter().map(Ok::<_, Infallible>),
        )))
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
        let path = std::env::temp_dir().join(format!("hal100-real-hermes-{}", Uuid::new_v4()));
        fs::create_dir(&path).expect("create isolated test directory");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
