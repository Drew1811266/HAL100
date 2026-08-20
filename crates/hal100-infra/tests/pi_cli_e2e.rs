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
    PiCodingAgentIntegrationAdapter, PiCodingAgentPaths, UsageWriter, gateway_router,
};
use serde_json::{Value, json};
use uuid::Uuid;

const BACKEND_KEY: &str = "pi-real-cli-backend-key";
const PI_PACKAGE: &str = "@earendil-works/pi-coding-agent@0.84.2";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "downloads/runs the pinned official Pi Coding Agent CLI in an isolated temporary HOME"]
async fn official_pi_cli_uses_managed_provider_and_records_isolated_usage() {
    let pi_package = selected_pi_package();
    let temp = TestDirectory::new();
    let project = temp.0.join("project");
    let fake_home = temp.0.join("home");
    let app_data = temp.0.join("hal100-data");
    let fake_binary = temp.0.join("fake-bin/pi");
    fs::create_dir_all(&project).expect("project directory");
    fs::create_dir_all(&fake_home).expect("fake HOME");
    fs::create_dir_all(fake_binary.parent().expect("fake binary parent"))
        .expect("fake binary directory");
    fs::write(&fake_binary, "#!/bin/sh\nprintf '0.84.2\\n'\n").expect("fake detector CLI");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake_binary, fs::Permissions::from_mode(0o700))
            .expect("fake detector executable");
    }

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
    let agent_directory = fake_home.join(".pi/agent");
    let manager = PiCodingAgentIntegrationAdapter::with_gateway_base_url(
        database.clone(),
        credentials.clone(),
        ExternalModelProfileRegistry::conservative_managed_route(),
        PiCodingAgentPaths {
            home_directory: fake_home.clone(),
            config_path: agent_directory.join("models.json"),
            agent_directory: agent_directory.clone(),
            credential_path: app_data.join("credentials/pi-coding-agent-gateway.key"),
            binary_candidates: vec![fake_binary],
        },
        gateway_base_url,
    );
    let plan = manager.plan_configuration().expect("Pi configuration plan");
    manager
        .apply_configuration(&plan.plan_id)
        .expect("confirmed Pi configuration");

    let usage_writer = UsageWriter::start(database.clone());
    let backend = BackendConfig::new(
        "official-pi-mock-backend",
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
        run_official_pi(&pi_package, &fake_home, &project, &agent_directory)
    })
    .await
    .expect("Pi blocking task")
    .expect("run official Pi CLI");
    let stdout = String::from_utf8_lossy(&command_output.stdout);
    let stderr = String::from_utf8_lossy(&command_output.stderr);
    assert!(
        command_output.status.success(),
        "Pi exited unsuccessfully\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("HAL100_PI_E2E_OK"),
        "Pi output did not contain the mock response\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    usage_writer
        .flush(Duration::from_secs(2))
        .expect("flush Pi usage");
    assert!(
        database
            .usage_request_count_for_client("pi-coding-agent")
            .expect("Pi usage count")
            >= 1
    );
    assert_eq!(
        database
            .usage_request_count_for_client("hal100-agent")
            .expect("built-in Agent usage count"),
        0,
        "external Pi must never be attributed to HAL100's built-in Agent runtime"
    );
    let requests = captured_requests.lock().expect("captured requests");
    assert!(requests.iter().any(|request| {
        request["model"] == "hal100-active"
            && request["stream"] == true
            && request["messages"]
                .as_array()
                .is_some_and(|messages| !messages.is_empty())
    }));

    gateway_task.abort();
    backend_task.abort();
}

fn run_official_pi(
    pi_package: &str,
    fake_home: &Path,
    project: &Path,
    agent_directory: &Path,
) -> std::io::Result<Output> {
    fs::create_dir_all(fake_home.join("tmp"))?;
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut command = Command::new("pnpm");
    command
        .args([
            "dlx",
            pi_package,
            "--mode",
            "json",
            "--no-session",
            "--provider",
            "hal100",
            "--model",
            "hal100-active",
            "--no-tools",
            "Reply with exactly HAL100_PI_E2E_OK.",
        ])
        .current_dir(project)
        .env_clear()
        .env("PATH", path)
        .env("HOME", fake_home)
        .env("TMPDIR", fake_home.join("tmp"))
        .env("PI_CODING_AGENT_DIR", agent_directory)
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("NO_COLOR", "1")
        .env("CI", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    run_with_timeout(command, Duration::from_secs(90))
}

fn selected_pi_package() -> String {
    let package = std::env::var("HAL100_PI_TEST_PACKAGE").unwrap_or_else(|_| PI_PACKAGE.to_owned());
    let Some(version) = package.strip_prefix("@earendil-works/pi-coding-agent@") else {
        panic!("HAL100_PI_TEST_PACKAGE must be an exact @earendil-works/pi-coding-agent version");
    };
    assert!(
        !version.is_empty()
            && version
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.'),
        "HAL100_PI_TEST_PACKAGE must be an exact stable version"
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
                    "Pi timed out; stderr: {}",
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
        .push(request);
    let chunks = [
        "data: {\"id\":\"chatcmpl-hal100-pi\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"hal100-active\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"HAL100_PI_E2E_OK\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"chatcmpl-hal100-pi\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"hal100-active\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"id\":\"chatcmpl-hal100-pi\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"hal100-active\",\"choices\":[],\"usage\":{\"prompt_tokens\":13,\"completion_tokens\":4,\"total_tokens\":17}}\n\n",
        "data: [DONE]\n\n",
    ];
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(stream::iter(chunks.map(|chunk| {
            Ok::<_, Infallible>(Bytes::from_static(chunk.as_bytes()))
        }))))
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
        let path = std::env::temp_dir().join(format!("hal100-real-pi-{}", Uuid::new_v4()));
        fs::create_dir(&path).expect("create isolated test directory");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
