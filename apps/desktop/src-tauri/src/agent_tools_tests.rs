use std::{
    fs, future,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, Response, StatusCode, header},
    routing::get,
};
use hal100_infra::{
    CredentialRegistry, Database, EnvironmentDiagnostics, GatewayState, LlamaCppManager,
    ModelDownloadError, ModelDownloadManager, ModelRemovalManager, OpenCodeManager, OpenCodePaths,
    RemoteModelCatalog, RemoteModelCatalogError, UsageWriter,
};
use hal100_protocol::{
    AGENT_RPC_MAX_TOOL_RESULT_BYTES, DownloadSource, MODEL_CATALOG_SEARCH_TOOL,
    MODEL_REPOSITORY_INSPECTION_TOOL, ModelDownloadState, PLAN_MODEL_DOWNLOAD_TOOL,
    ToolCallRequestPayload, ToolCallResultStatus,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::*;

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "hal100-agent-tools-test-{}",
            Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&path).expect("create test directory");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn request(id: &str, tool_name: &str, arguments: Value) -> ToolCallRequestPayload {
    ToolCallRequestPayload {
        run_id: "agent-model-download-run".to_owned(),
        tool_call_id: id.to_owned(),
        tool_name: tool_name.to_owned(),
        arguments,
    }
}

#[test]
fn cancellable_remote_wait_stops_a_hung_future_promptly() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .build()
        .expect("test runtime");
    let cancellation = Arc::new(AtomicBool::new(false));
    let cancellation_for_thread = cancellation.clone();
    let canceller = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        cancellation_for_thread.store(true, Ordering::Release);
    });

    let started = Instant::now();
    let result = block_on_cancellable(runtime.handle(), &cancellation, future::pending::<()>());

    canceller.join().expect("canceller");
    assert!(matches!(result, Err(AgentToolExecutionError::Cancelled)));
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn tool_success_payloads_have_a_budget_below_the_rpc_frame_limit() {
    let request = request(
        "tool-result-budget",
        SYSTEM_SUMMARY_TOOL,
        json!({ "detail": "summary" }),
    );
    assert!(success(&request, json!({ "status": "ok" })).is_ok());

    let oversized = "x".repeat(AGENT_RPC_MAX_TOOL_RESULT_BYTES);
    assert!(matches!(
        success(&request, json!({ "value": oversized })),
        Err(AgentToolExecutionError::ResultTooLarge)
    ));
}

#[test]
fn remote_failures_keep_safe_actionable_codes() {
    assert_eq!(
        AgentToolExecutionError::RemoteCatalog(RemoteModelCatalogError::Network(
            "sensitive upstream detail".to_owned()
        ))
        .code(),
        "catalog_network_error"
    );
    assert_eq!(
        AgentToolExecutionError::ModelDownload(ModelDownloadError::InsufficientStorage {
            required_bytes: 2,
            available_bytes: 1,
        })
        .code(),
        "insufficient_storage"
    );
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
fn agent_model_discovery_plan_and_confirm_use_one_deterministic_download_manager() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("test runtime");
    let payload = gguf_payload();
    let hash = Sha256::digest(&payload)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let app = Router::new()
        .route("/catalog/models", get(hf_search))
        .route("/catalog/models/acme/model", get(hf_repository))
        .route(
            "/download/acme/model/resolve/revision/model-Q4_K_M.gguf",
            get(download),
        )
        .with_state((payload.clone(), hash));
    let (address, server) = runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock listener");
        let address = listener.local_addr().expect("mock address");
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        (address, server)
    });

    let temp = TestDirectory::new();
    let model_storage_path = temp.0.join("models");
    fs::create_dir_all(&model_storage_path).expect("model storage");
    let database = Arc::new(Database::open(temp.0.join("hal100.sqlite")).expect("database"));
    database
        .set_default_download_source(DownloadSource::HuggingFace, now_ms())
        .expect("default source");
    let credentials = CredentialRegistry::new(Vec::new());
    let gateway = GatewayState::new(
        None,
        credentials.clone(),
        UsageWriter::start(database.clone()),
    )
    .expect("gateway");
    let engine = Arc::new(
        LlamaCppManager::new(
            database.clone(),
            gateway.clone(),
            temp.0.join("engines/llama.cpp"),
        )
        .expect("engine"),
    );
    let open_code = Arc::new(OpenCodeManager::with_gateway_base_url(
        database.clone(),
        credentials,
        OpenCodePaths::for_macos(&temp.0.join("home"), &temp.0),
        "http://127.0.0.1:10100/v1".to_owned(),
    ));
    let catalog = Arc::new(
        RemoteModelCatalog::with_endpoints(
            &format!("http://{address}/catalog/"),
            &format!("http://{address}/unused-modelscope-open/"),
            &format!("http://{address}/unused-modelscope-legacy/"),
        )
        .expect("catalog"),
    );
    let downloads = Arc::new(
        ModelDownloadManager::with_download_endpoints(
            database.clone(),
            catalog.clone(),
            model_storage_path.clone(),
            &format!("http://{address}/download/"),
            &format!("http://{address}/unused-modelscope-download/"),
        )
        .expect("downloads"),
    );
    let removals = Arc::new(ModelRemovalManager::new(
        database.clone(),
        model_storage_path.clone(),
    ));
    let diagnostics = Arc::new(EnvironmentDiagnostics::new(
        database.clone(),
        engine.clone(),
        open_code.clone(),
        gateway.clone(),
    ));
    let action_plans = AgentActionPlanStore::new();
    let executor = AgentToolExecutor::new(
        model_storage_path,
        database.clone(),
        engine,
        open_code,
        removals,
        diagnostics,
        catalog,
        downloads.clone(),
        gateway,
        action_plans.clone(),
    );
    let cancellation = Arc::new(AtomicBool::new(false));
    let mut run = executor.start_run(
        "agent-model-download-run".to_owned(),
        runtime.handle().clone(),
        cancellation,
    );

    let search = run
        .handle(&request(
            "tool-search",
            MODEL_CATALOG_SEARCH_TOOL,
            json!({ "query": "acme gguf" }),
        ))
        .expect("search execution");
    assert_eq!(search.status, ToolCallResultStatus::Success);
    assert_eq!(
        search.output.as_ref().expect("search output")["items"][0]["repository"],
        "acme/model"
    );

    let forged_repository = run
        .handle(&request(
            "tool-forged-repository",
            MODEL_REPOSITORY_INSPECTION_TOOL,
            json!({ "repository": "other/model" }),
        ))
        .expect("forged repository result");
    assert_eq!(forged_repository.status, ToolCallResultStatus::Error);
    assert_eq!(
        forged_repository.error.expect("forged error").code,
        "repository_not_in_search_results"
    );

    let repository = run
        .handle(&request(
            "tool-repository",
            MODEL_REPOSITORY_INSPECTION_TOOL,
            json!({ "repository": "acme/model" }),
        ))
        .expect("repository execution");
    assert_eq!(repository.status, ToolCallResultStatus::Success);
    assert_eq!(
        repository.output.as_ref().expect("repository output")["files"][0]["path"],
        "model-Q4_K_M.gguf"
    );

    let forged_file = run
        .handle(&request(
            "tool-forged-file",
            PLAN_MODEL_DOWNLOAD_TOOL,
            json!({ "remotePath": "other.gguf" }),
        ))
        .expect("forged file result");
    assert_eq!(forged_file.status, ToolCallResultStatus::Error);
    assert_eq!(
        forged_file.error.expect("forged error").code,
        "remote_file_not_in_repository_snapshot"
    );

    let planned = run
        .handle(&request(
            "tool-download-plan",
            PLAN_MODEL_DOWNLOAD_TOOL,
            json!({ "remotePath": "model-Q4_K_M.gguf" }),
        ))
        .expect("download plan execution");
    assert_eq!(planned.status, ToolCallResultStatus::Success);
    let outer_plan_id = planned.output.expect("download plan output")["planId"]
        .as_str()
        .expect("outer plan id")
        .to_owned();
    let pending = action_plans
        .take(&outer_plan_id, now_ms())
        .expect("native confirmation consumes outer plan");
    let download_plan_id = match pending.executor {
        AgentActionExecutor::DownloadModel { download_plan_id } => download_plan_id,
        _ => panic!("expected deterministic download executor"),
    };
    let snapshot = runtime
        .block_on(async { executor.start_model_download(&download_plan_id) })
        .expect("start confirmed download");
    let completed = runtime.block_on(async {
        for _ in 0..100 {
            let current = downloads
                .download(&snapshot.download_id)
                .expect("download snapshot");
            if current.state == ModelDownloadState::Ready {
                return current;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        downloads
            .download(&snapshot.download_id)
            .expect("final download snapshot")
    });
    assert_eq!(completed.state, ModelDownloadState::Ready);
    assert_eq!(database.local_models().expect("model index").len(), 1);
    assert!(Path::new(&database.local_models().unwrap()[0].path).is_file());
    server.abort();
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
async fn hf_search() -> Json<Value> {
    Json(json!([{
        "id": "acme/model",
        "downloads": 10,
        "likes": 2,
        "gated": false,
        "private": false,
        "tags": ["gguf", "license:mit"],
        "usedStorage": 64
    }]))
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
async fn hf_repository(State((payload, hash)): State<(Vec<u8>, String)>) -> Json<Value> {
    Json(json!({
        "id": "acme/model",
        "sha": "revision",
        "gated": false,
        "private": false,
        "tags": ["gguf", "license:mit"],
        "siblings": [{
            "rfilename": "model-Q4_K_M.gguf",
            "size": payload.len(),
            "lfs": { "size": payload.len(), "sha256": hash }
        }]
    }))
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
async fn download(
    State((payload, _)): State<(Vec<u8>, String)>,
    headers: HeaderMap,
) -> Response<Body> {
    if let Some(start) = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("bytes="))
        .and_then(|value| value.strip_suffix('-'))
        .and_then(|value| value.parse::<usize>().ok())
    {
        return Response::builder()
            .status(StatusCode::PARTIAL_CONTENT)
            .header(
                header::CONTENT_RANGE,
                HeaderValue::from_str(&format!(
                    "bytes {start}-{}/{}",
                    payload.len() - 1,
                    payload.len()
                ))
                .expect("content range"),
            )
            .body(Body::from(payload[start..].to_vec()))
            .expect("range response");
    }
    Response::new(Body::from(payload))
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn gguf_payload() -> Vec<u8> {
    let mut output = Vec::new();
    output.extend_from_slice(b"GGUF");
    output.extend_from_slice(&3_u32.to_le_bytes());
    output.extend_from_slice(&42_u64.to_le_bytes());
    output.extend_from_slice(&7_u64.to_le_bytes());
    output.extend_from_slice(b"agent model download fixture");
    output
}
