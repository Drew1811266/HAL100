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
    BackendConfig, CredentialRegistry, Database, GatewayState, OpenCodeManager, OpenCodePaths,
    UsageWriter, gateway_router,
};
use serde_json::{Value, json};
use uuid::Uuid;

const BACKEND_KEY: &str = "opencode-real-cli-backend-key";
const OPENCODE_PACKAGE: &str = "opencode-ai@1.18.11";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "downloads/runs the pinned official OpenCode CLI in an isolated temporary HOME"]
async fn official_opencode_cli_uses_managed_provider_and_records_usage() {
    let opencode_package = selected_opencode_package();
    let temp = TestDirectory::new();
    let project = temp.0.join("project");
    let fake_home = temp.0.join("home");
    let app_data = temp.0.join("hal100-data");
    fs::create_dir_all(&project).expect("project directory");
    fs::create_dir_all(&fake_home).expect("fake HOME");

    let captured_requests = Arc::new(Mutex::new(Vec::new()));
    let backend_state = MockBackendState {
        requests: captured_requests.clone(),
    };
    let backend_router = Router::new()
        .route("/v1/models", get(mock_models))
        .route("/v1/chat/completions", post(mock_chat))
        .with_state(backend_state);
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
    let manager = OpenCodeManager::with_gateway_base_url(
        database.clone(),
        credentials.clone(),
        OpenCodePaths {
            home_directory: fake_home.clone(),
            config_path: fake_home.join(".config/opencode/opencode.json"),
            alternate_config_path: fake_home.join(".config/opencode/opencode.jsonc"),
            credential_path: app_data.join("credentials/opencode-gateway.key"),
            binary_candidates: Vec::new(),
        },
        gateway_base_url,
    );
    let plan = manager.plan_configuration().expect("OpenCode plan");
    manager
        .apply_configuration(&plan.plan_id)
        .expect("confirmed OpenCode configuration");

    let usage_writer = UsageWriter::start(database.clone());
    let backend = BackendConfig::new(
        "official-opencode-mock-backend",
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

    let config_path = fake_home.join(".config/opencode/opencode.json");
    let command_output = tokio::task::spawn_blocking(move || {
        run_official_opencode(&opencode_package, &fake_home, &project, &config_path)
    })
    .await
    .expect("OpenCode blocking task")
    .expect("run official OpenCode CLI");
    let stdout = String::from_utf8_lossy(&command_output.stdout);
    let stderr = String::from_utf8_lossy(&command_output.stderr);
    assert!(
        command_output.status.success(),
        "OpenCode exited unsuccessfully\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("HAL100_E2E_OK"),
        "OpenCode output did not contain the mock response\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    usage_writer
        .flush(Duration::from_secs(2))
        .expect("flush OpenCode usage");
    assert!(
        database
            .usage_request_count_for_client("opencode")
            .expect("OpenCode usage count")
            >= 1
    );
    let requests = captured_requests.lock().expect("captured requests");
    assert!(!requests.is_empty(), "OpenCode made no Chat request");
    assert!(requests.iter().any(|request| {
        request["model"] == "hal100-active"
            && request["stream"] == true
            && request["tools"]
                .as_array()
                .is_some_and(|tools| !tools.is_empty())
    }));

    gateway_task.abort();
    backend_task.abort();
}

fn run_official_opencode(
    opencode_package: &str,
    fake_home: &Path,
    project: &Path,
    config_path: &Path,
) -> std::io::Result<Output> {
    fs::create_dir_all(fake_home.join("tmp"))?;
    let first = run_with_timeout(
        official_opencode_command(opencode_package, fake_home, project, config_path),
        Duration::from_secs(90),
    )?;
    let completed_first_run_migration = first.status.success()
        && !String::from_utf8_lossy(&first.stdout).contains("HAL100_E2E_OK")
        && String::from_utf8_lossy(&first.stderr).contains("Database migration complete.");
    if completed_first_run_migration {
        return run_with_timeout(
            official_opencode_command(opencode_package, fake_home, project, config_path),
            Duration::from_secs(90),
        );
    }
    Ok(first)
}

fn official_opencode_command(
    opencode_package: &str,
    fake_home: &Path,
    project: &Path,
    config_path: &Path,
) -> Command {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut command = Command::new("pnpm");
    command
        .args([
            "dlx",
            opencode_package,
            "--pure",
            "run",
            "--model",
            "hal100/hal100-active",
            "--format",
            "json",
            "--dir",
        ])
        .arg(project)
        .arg("Reply with exactly HAL100_E2E_OK. Do not call any tools.")
        .current_dir(project)
        .env_clear()
        .env("PATH", path)
        .env("HOME", fake_home)
        .env("TMPDIR", fake_home.join("tmp"))
        .env("XDG_CONFIG_HOME", fake_home.join(".config"))
        .env("XDG_DATA_HOME", fake_home.join(".local/share"))
        .env("XDG_CACHE_HOME", fake_home.join(".cache"))
        .env("OPENCODE_CONFIG", config_path)
        .env("OPENCODE_DISABLE_AUTOUPDATE", "true")
        .env("OPENCODE_DISABLE_MODELS_FETCH", "true")
        .env("OPENCODE_DISABLE_DEFAULT_PLUGINS", "true")
        .env("OPENCODE_DISABLE_LSP_DOWNLOAD", "true")
        .env("OPENCODE_DISABLE_CLAUDE_CODE", "true")
        .env("OPENCODE_DISABLE_CLAUDE_CODE_PROMPT", "true")
        .env("OPENCODE_DISABLE_CLAUDE_CODE_SKILLS", "true")
        .env("OPENCODE_EXPERIMENTAL_DISABLE_FILEWATCHER", "true")
        .env("OPENCODE_CLIENT", "hal100-e2e")
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("NO_COLOR", "1")
        .env("CI", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn selected_opencode_package() -> String {
    let package = std::env::var("HAL100_OPENCODE_TEST_PACKAGE")
        .unwrap_or_else(|_| OPENCODE_PACKAGE.to_owned());
    let Some(version) = package.strip_prefix("opencode-ai@") else {
        panic!("HAL100_OPENCODE_TEST_PACKAGE must be an exact opencode-ai version");
    };
    assert!(
        !version.is_empty()
            && version
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.'),
        "HAL100_OPENCODE_TEST_PACKAGE must be an exact stable version"
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
                    "OpenCode timed out; stderr: {}",
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
        "data: {\"id\":\"chatcmpl-hal100\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"hal100-active\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"HAL100_E2E_OK\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"chatcmpl-hal100\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"hal100-active\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"id\":\"chatcmpl-hal100\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"hal100-active\",\"choices\":[],\"usage\":{\"prompt_tokens\":13,\"completion_tokens\":4,\"total_tokens\":17}}\n\n",
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
        let path = std::env::temp_dir().join(format!("hal100-real-opencode-{}", Uuid::new_v4()));
        fs::create_dir(&path).expect("create isolated test directory");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
