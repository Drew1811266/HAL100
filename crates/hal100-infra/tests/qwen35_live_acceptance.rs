use std::{
    error::Error,
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use hal100_infra::{
    CredentialRegistry, Database, GatewayState, LlamaCppManager, ModelDownloadManager,
    RemoteModelCatalog, UsageWriter, serve_gateway, stored_client_credential,
};
use hal100_platform::NativeSystemProbe;
use hal100_protocol::{
    DownloadSource, EngineInstallState, EngineRuntimeState, LocalModelSummary, ModelDownloadState,
};
use serde_json::{Value, json};

const CONFIRMATION_ENV: &str = "HAL100_QWEN35_DEPLOY_CONFIRMED";
const DATA_DIR_ENV: &str = "HAL100_ACCEPTANCE_DATA_DIR";
const REPOSITORY: &str = "unsloth/Qwen3.5-2B-GGUF";
const REMOTE_FILE: &str = "Qwen3.5-2B-Q4_K_M.gguf";
const EXPECTED_SIZE_BYTES: u64 = 1_280_835_840;
const EXPECTED_SHA256: &str = "aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223";
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const POLL_INTERVAL: Duration = Duration::from_millis(750);

type AcceptanceResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "downloads and runs the confirmed Qwen3.5-2B artifact in the real HAL100 data directory"]
async fn confirmed_qwen35_deployment_produces_exact_gateway_usage() -> AcceptanceResult<()> {
    require_explicit_confirmation()?;
    let data_dir = PathBuf::from(std::env::var(DATA_DIR_ENV)?);
    std::fs::create_dir_all(&data_dir)?;
    let model_storage = data_dir.join("models");
    std::fs::create_dir_all(&model_storage)?;

    let database = Arc::new(Database::open(data_dir.join("hal100.sqlite"))?);
    let client_key = format!("hal100_qwen35_acceptance_{}", uuid::Uuid::new_v4().simple());
    let credential = stored_client_credential(
        "qwen35-live-acceptance-key",
        "qwen35-live-acceptance",
        "Qwen3.5 实机验收",
        &client_key,
    )?;
    database.upsert_client_credential(&credential, now_ms())?;
    let usage_writer = UsageWriter::start(database.clone());
    let gateway = GatewayState::new(
        None,
        CredentialRegistry::new(database.load_client_credentials()?),
        usage_writer.clone(),
    )?;

    let engine = Arc::new(LlamaCppManager::new(
        database.clone(),
        gateway.clone(),
        data_dir.join("engines").join("llama.cpp"),
    )?);
    let engine_status = engine.status()?;
    if engine_status.install_state != EngineInstallState::Installed {
        let plan = engine.plan_install()?;
        eprintln!(
            "HAL100_ACCEPTANCE_ENGINE_PLAN version={} bytes={} publisher={}",
            plan.version, plan.archive_size_bytes, plan.publisher
        );
        let installed = engine.apply_install(&plan.plan_id).await?;
        ensure(
            installed.install_state == EngineInstallState::Installed,
            "llama.cpp did not reach the installed state",
        )?;
    }
    let installed = engine.status()?;
    eprintln!(
        "HAL100_ACCEPTANCE_ENGINE_READY version={} state={:?}",
        installed.version, installed.install_state
    );

    let model = ensure_model(database.clone(), &model_storage).await?;
    ensure(
        model.size_bytes == EXPECTED_SIZE_BYTES,
        "indexed model size changed",
    )?;
    ensure(
        model.repository.as_deref() == Some(REPOSITORY),
        "indexed model repository changed",
    )?;
    ensure(model.file_name == REMOTE_FILE, "indexed model file changed")?;
    eprintln!(
        "HAL100_ACCEPTANCE_MODEL_READY id={} bytes={} path={}",
        model.id, model.size_bytes, model.path
    );

    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let gateway_address = listener.local_addr()?;
    let gateway_task = tokio::spawn(serve_gateway(listener, gateway.clone()));

    let running = engine.start_model(&model.id).await?;
    ensure(
        running.runtime_state == EngineRuntimeState::Running,
        "llama.cpp did not reach the running state",
    )?;
    let backend_port = running
        .port
        .ok_or_else(|| io::Error::other("running engine did not expose its backend port"))?;
    eprintln!(
        "HAL100_ACCEPTANCE_RUNTIME_READY backend_port={} gateway={}",
        backend_port, gateway_address
    );

    let inference_result = run_gateway_inference(
        gateway_address,
        &client_key,
        database.clone(),
        &usage_writer,
    )
    .await;
    let stop_result = engine.stop().await;
    gateway_task.abort();

    let evidence = inference_result?;
    let stopped = stop_result?;
    ensure(
        stopped.runtime_state == EngineRuntimeState::Stopped,
        "llama.cpp did not stop after acceptance",
    )?;

    println!("{}", serde_json::to_string_pretty(&evidence)?);
    Ok(())
}

async fn ensure_model(
    database: Arc<Database>,
    model_storage: &Path,
) -> AcceptanceResult<LocalModelSummary> {
    if let Some(model) = matching_model(&database)? {
        return Ok(model);
    }

    let catalog = Arc::new(RemoteModelCatalog::new()?);
    let manager = Arc::new(ModelDownloadManager::new(
        database.clone(),
        catalog,
        model_storage.to_owned(),
    )?);
    let available = NativeSystemProbe.model_storage_available_bytes(model_storage)?;

    let matching_download = database.downloads()?.into_iter().find(|download| {
        download.source == DownloadSource::HuggingFace
            && download.repository == REPOSITORY
            && download.file_name == REMOTE_FILE
            && matches!(
                download.state,
                ModelDownloadState::Paused
                    | ModelDownloadState::Failed
                    | ModelDownloadState::Cancelled
            )
    });
    let download_id = if let Some(download) = matching_download {
        eprintln!(
            "HAL100_ACCEPTANCE_DOWNLOAD_RESUME id={} bytes={}/{}",
            download.id, download.downloaded_bytes, download.expected_size_bytes
        );
        manager
            .resume_download(&download.id, available)
            .await?
            .download_id
    } else {
        let plan = manager
            .plan_download(
                DownloadSource::HuggingFace,
                REPOSITORY,
                REMOTE_FILE,
                available,
            )
            .await?;
        ensure(
            plan.file.size_bytes == EXPECTED_SIZE_BYTES,
            "remote model size differs from the confirmed plan",
        )?;
        ensure(
            plan.file.sha256.as_deref() == Some(EXPECTED_SHA256),
            "remote model SHA-256 differs from the confirmed plan",
        )?;
        eprintln!(
            "HAL100_ACCEPTANCE_MODEL_PLAN repository={} revision={} file={} bytes={} sha256={}",
            plan.repository,
            plan.file.revision,
            plan.file.path,
            plan.file.size_bytes,
            plan.file.sha256.as_deref().unwrap_or("missing")
        );
        manager
            .start_download(&plan.plan_id, available)?
            .download_id
    };

    let started = Instant::now();
    let mut last_reported_bytes = u64::MAX;
    loop {
        if started.elapsed() > DOWNLOAD_TIMEOUT {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Qwen3.5 model download timed out",
            )
            .into());
        }
        let snapshot = manager.download(&download_id)?;
        if snapshot.downloaded_bytes != last_reported_bytes {
            let percent = if snapshot.expected_size_bytes == 0 {
                0.0
            } else {
                snapshot.downloaded_bytes as f64 * 100.0 / snapshot.expected_size_bytes as f64
            };
            eprintln!(
                "HAL100_ACCEPTANCE_DOWNLOAD state={:?} bytes={}/{} percent={percent:.1}",
                snapshot.state, snapshot.downloaded_bytes, snapshot.expected_size_bytes
            );
            last_reported_bytes = snapshot.downloaded_bytes;
        }
        match snapshot.state {
            ModelDownloadState::Ready => break,
            ModelDownloadState::Failed | ModelDownloadState::Cancelled => {
                return Err(io::Error::other(format!(
                    "Qwen3.5 model download stopped: state={:?} error={:?}",
                    snapshot.state, snapshot.error_code
                ))
                .into());
            }
            _ => tokio::time::sleep(POLL_INTERVAL).await,
        }
    }

    matching_model(&database)?.ok_or_else(|| {
        io::Error::other("download reached ready but the model index is missing").into()
    })
}

fn matching_model(database: &Database) -> AcceptanceResult<Option<LocalModelSummary>> {
    Ok(database.local_models()?.into_iter().find(|model| {
        model.repository.as_deref() == Some(REPOSITORY)
            && model.file_name == REMOTE_FILE
            && model.size_bytes == EXPECTED_SIZE_BYTES
    }))
}

async fn run_gateway_inference(
    gateway_address: std::net::SocketAddr,
    client_key: &str,
    database: Arc<Database>,
    usage_writer: &UsageWriter,
) -> AcceptanceResult<Value> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(180))
        .no_proxy()
        .build()?;
    let started = Instant::now();
    let response = client
        .post(format!("http://{gateway_address}/v1/chat/completions"))
        .bearer_auth(client_key)
        .json(&json!({
            "model": "hal100-active",
            "messages": [{
                "role": "user",
                "content": "请用一句简短中文回复：HAL100 的 Qwen3.5 本地推理测试成功。"
            }],
            "temperature": 0,
            "max_tokens": 64,
            "stream": false
        }))
        .send()
        .await?;
    let request_id = response
        .headers()
        .get("x-hal100-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(|| io::Error::other("Gateway response is missing x-hal100-request-id"))?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(io::Error::other(format!(
            "Gateway inference failed with HTTP {status}: {body}"
        ))
        .into());
    }
    let value: Value = serde_json::from_str(&body)?;
    let content = value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .filter(|content| !content.trim().is_empty())
        .ok_or_else(|| io::Error::other("model response did not contain text"))?;
    let input_tokens = required_usage(&value, "prompt_tokens")?;
    let output_tokens = required_usage(&value, "completion_tokens")?;
    let total_tokens = required_usage(&value, "total_tokens")?;
    ensure(
        total_tokens == input_tokens.saturating_add(output_tokens),
        "backend usage total does not equal prompt + completion tokens",
    )?;

    usage_writer.flush(Duration::from_secs(2))?;
    let stored = database.usage_request(&request_id)?;
    let stored_input_tokens = i64::try_from(input_tokens)?;
    let stored_output_tokens = i64::try_from(output_tokens)?;
    let stored_total_tokens = i64::try_from(total_tokens)?;
    ensure(
        stored.client_app_id == "qwen35-live-acceptance",
        "Gateway attribution did not use the acceptance client",
    )?;
    ensure(
        stored.input_tokens == Some(stored_input_tokens)
            && stored.output_tokens == Some(stored_output_tokens)
            && stored.total_tokens == Some(stored_total_tokens),
        "SQLite usage does not match the exact backend usage",
    )?;
    ensure(
        stored.usage_accuracy == "exact_backend_response" && stored.status == "succeeded",
        "SQLite usage was not persisted as an exact successful response",
    )?;

    Ok(json!({
        "artifact": {
            "repository": REPOSITORY,
            "file": REMOTE_FILE,
            "sizeBytes": EXPECTED_SIZE_BYTES,
            "sha256": EXPECTED_SHA256
        },
        "gateway": {
            "requestId": request_id,
            "clientAppId": stored.client_app_id,
            "requestedModel": stored.requested_model,
            "resolvedModel": stored.resolved_model,
            "backendId": stored.backend_id,
            "status": stored.status,
            "usageAccuracy": stored.usage_accuracy
        },
        "response": {
            "content": content,
            "model": value.get("model").cloned().unwrap_or(Value::Null),
            "elapsedMs": u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
        },
        "usage": {
            "inputTokens": input_tokens,
            "outputTokens": output_tokens,
            "totalTokens": total_tokens
        }
    }))
}

fn required_usage(value: &Value, field: &str) -> AcceptanceResult<u64> {
    value
        .get("usage")
        .and_then(|usage| usage.get(field))
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            io::Error::other(format!("backend response is missing usage.{field}")).into()
        })
}

fn require_explicit_confirmation() -> AcceptanceResult<()> {
    ensure(
        std::env::var(CONFIRMATION_ENV).as_deref() == Ok("confirmed"),
        "real deployment requires HAL100_QWEN35_DEPLOY_CONFIRMED=confirmed",
    )
}

fn ensure(condition: bool, message: &str) -> AcceptanceResult<()> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message).into())
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}
