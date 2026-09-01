use std::{
    collections::HashMap,
    convert::Infallible,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use futures_util::{StreamExt, stream};
use hal100_core::{SecretStore, SecretStoreError, SecretStoreOperation};
use hal100_infra::{
    ActiveGatewayRoute, BackendConfig, BackendManager, BoundedEngineHttpClient, CredentialRegistry,
    Database, EngineInspector, ExternalEngineInspectionFuture, ExternalEngineQualificationFuture,
    ExternalInferenceEngineAdapter, ExternalInferenceEngineRegistry, GatewayRouteSwitchError,
    GatewayState, InferenceEngineAcceptanceEvidence, InferenceEngineAcceptanceEvidenceError,
    InferenceEngineAcceptanceHostAttestation, InferenceEngineAcceptanceResilience,
    InferenceEngineAcceptanceRun, InferenceEngineAcceptanceRunOutcome,
    InferenceEngineAcceptanceStability, LlamaCppManager, OpenAiQualificationOptions,
    OpenAiStabilityObservation, RuntimeActivationJournalRepository, RuntimeActivationPhase,
    RuntimeProfileManager, StoredRuntimeActivationJournal, UsageWriter, VerifiedEngineTarget,
    gateway_router, qualify_openai_runtime_stability, stored_client_credential,
    write_acceptance_run_exclusive,
};
use hal100_platform::NativeSystemProbe;
use hal100_protocol::{
    BackendAuthMethod, BackendDraft, BackendKind, EngineAdapterId, ExternalRuntimeProfileDraft,
    HostCapabilitySnapshot, InferenceAccelerator, InferenceArchitecture, InferenceDeployment,
    InferenceEngineManifest, InferenceEngineSupportEvidenceKind,
    InferenceEngineSupportEvidenceSummary, InferenceEngineSupportStatus, InferencePlatform,
    RuntimeProfileSupportCell,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Resolve one real-acceptance entry point strictly from the adapter manifest and the native
/// compilation identity. Every live test uses this selector so platform cells cannot drift into
/// the manifest without having an executable evidence path. External live acceptance remains
/// local-only; a future remote cell needs a distinct target and evidence contract.
#[allow(dead_code)]
pub fn select_declared_acceptance_cell(
    manifest: &InferenceEngineManifest,
    os: &str,
    architecture: &str,
    accelerator: &str,
) -> Option<RuntimeProfileSupportCell> {
    let cell = RuntimeProfileSupportCell {
        platform: InferencePlatform::from_storage_key(os)?,
        architecture: InferenceArchitecture::from_storage_key(architecture)?,
        accelerator: InferenceAccelerator::from_storage_key(accelerator)?,
        deployment: InferenceDeployment::Local,
    };
    manifest
        .support_units
        .iter()
        .any(|unit| {
            unit.platform == cell.platform
                && unit.architecture == cell.architecture
                && unit.accelerator == cell.accelerator
                && unit.deployment == cell.deployment
        })
        .then_some(cell)
}

/// Resolve and attest the real host before any external request is sent.
///
/// Live acceptance used to defer the native host probe until after protocol and stability
/// traffic. A mistyped accelerator or a runner on the wrong platform could therefore exercise a
/// real model before being rejected. Keep declaration resolution and native attestation as one
/// preflight step so every engine fails closed before inspection, qualification or inference.
#[allow(dead_code)]
pub fn prepare_real_acceptance_host(
    manifest: &InferenceEngineManifest,
    accelerator: &str,
    label: &str,
) -> (RuntimeProfileSupportCell, HostCapabilitySnapshot) {
    let cell = select_declared_acceptance_cell(
        manifest,
        std::env::consts::OS,
        std::env::consts::ARCH,
        accelerator,
    )
    .unwrap_or_else(|| {
        panic!(
            "unsupported {label} acceptance cell: {}/{}/{}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            accelerator
        )
    });
    let host =
        probe_real_acceptance_host(cell.platform, cell.architecture, cell.accelerator, label);
    (cell, host)
}

#[allow(dead_code)]
#[derive(Default)]
struct AcceptanceSecretStore {
    values: Mutex<HashMap<String, Vec<u8>>>,
}

/// Test-only manifest wrapper used while collecting a real acceptance run for a currently
/// `connected` cell. It promotes exactly the requested host cell inside this one in-memory
/// registry so the production profile gate can exercise the save/activate/verify path. The
/// checked-in adapter manifest, support matrix and acceptance ledger are never modified.
struct AcceptanceSupportCellAdapter {
    delegate: Arc<dyn ExternalInferenceEngineAdapter>,
    manifest: InferenceEngineManifest,
}

impl EngineInspector for AcceptanceSupportCellAdapter {
    fn manifest(&self) -> InferenceEngineManifest {
        self.manifest.clone()
    }

    fn protocol_capability_hash(&self) -> Option<String> {
        self.delegate.protocol_capability_hash()
    }

    fn default_target(&self) -> Option<VerifiedEngineTarget> {
        self.delegate.default_target()
    }

    fn inspect<'a>(
        &'a self,
        target: &'a VerifiedEngineTarget,
    ) -> ExternalEngineInspectionFuture<'a> {
        self.delegate.inspect(target)
    }

    fn qualify<'a>(
        &'a self,
        target: &'a VerifiedEngineTarget,
        model_id: &'a str,
    ) -> ExternalEngineQualificationFuture<'a> {
        self.delegate.qualify(target, model_id)
    }
}

impl ExternalInferenceEngineAdapter for AcceptanceSupportCellAdapter {}

fn registry_for_acceptance_lifecycle(
    adapter_id: &EngineAdapterId,
    host: &HostCapabilitySnapshot,
    accelerator: InferenceAccelerator,
) -> Arc<ExternalInferenceEngineRegistry> {
    let standard =
        ExternalInferenceEngineRegistry::standard().expect("standard external engine registry");
    let delegate = standard
        .adapter_by_id(adapter_id)
        .expect("acceptance adapter must be registered");
    let mut manifest = delegate.manifest();
    let mut matched = false;
    for unit in &mut manifest.support_units {
        if unit.platform == host.platform
            && unit.architecture == host.architecture
            && unit.accelerator == accelerator
            && unit.deployment == InferenceDeployment::Local
        {
            unit.status = InferenceEngineSupportStatus::VerifiedExternal;
            unit.evidence = Some(InferenceEngineSupportEvidenceSummary::for_status(
                InferenceEngineSupportStatus::VerifiedExternal,
            ));
            matched = true;
        }
    }
    assert!(
        matched,
        "acceptance host must match one declared support cell for the adapter"
    );
    Arc::new(
        ExternalInferenceEngineRegistry::new(vec![Arc::new(AcceptanceSupportCellAdapter {
            delegate,
            manifest,
        })])
        .expect("acceptance lifecycle registry"),
    )
}

impl SecretStore for AcceptanceSecretStore {
    fn read(&self, credential_id: &str) -> Result<Option<Vec<u8>>, SecretStoreError> {
        Ok(self
            .values
            .lock()
            .map_err(|_| SecretStoreError::new(SecretStoreOperation::Read))?
            .get(credential_id)
            .cloned())
    }

    fn write(&self, credential_id: &str, secret: &[u8]) -> Result<(), SecretStoreError> {
        self.values
            .lock()
            .map_err(|_| SecretStoreError::new(SecretStoreOperation::Write))?
            .insert(credential_id.to_owned(), secret.to_vec());
        Ok(())
    }

    fn delete(&self, credential_id: &str) -> Result<(), SecretStoreError> {
        self.values
            .lock()
            .map_err(|_| SecretStoreError::new(SecretStoreOperation::Delete))?
            .remove(credential_id);
        Ok(())
    }
}

/// Run the same save → revalidate → activate → verify loop used by the desktop runtime.
///
/// This helper is intentionally test-only: it never starts an external engine, downloads a
/// model, reads process arguments, or accepts a non-loopback endpoint. The caller must have
/// already performed the adapter's real inspection and protocol qualification.
#[allow(dead_code)]
pub struct ExternalProfileLifecycleInput<'a> {
    pub adapter_id: &'a EngineAdapterId,
    pub api_root: &'a str,
    pub model_id: &'a str,
    pub expected_evidence: hal100_protocol::RuntimeProfileEvidence,
    pub host: HostCapabilitySnapshot,
    pub accelerator: InferenceAccelerator,
    pub api_key: Option<&'a str>,
    pub label: &'a str,
}

#[allow(dead_code)]
pub async fn verify_external_profile_lifecycle(input: ExternalProfileLifecycleInput<'_>) {
    let ExternalProfileLifecycleInput {
        adapter_id,
        api_root,
        model_id,
        expected_evidence,
        host,
        accelerator,
        api_key,
        label,
    } = input;
    let database = Arc::new(Database::open_in_memory().expect("acceptance database"));
    let client_key = "hal100_acceptance_0123456789abcdef";
    let credential = stored_client_credential(
        "acceptance-client",
        "acceptance",
        "Acceptance Gateway probe",
        client_key,
    )
    .expect("acceptance Gateway credential");
    database
        .upsert_client_credential(&credential, 1)
        .expect("persist acceptance Gateway credential");
    let gateway = GatewayState::new(
        None,
        CredentialRegistry::new(
            database
                .load_client_credentials()
                .expect("load acceptance Gateway credential"),
        ),
        UsageWriter::start(database.clone()),
    )
    .expect("acceptance gateway");
    let backend_manager = Arc::new(BackendManager::new(
        database.clone(),
        gateway.clone(),
        Arc::new(AcceptanceSecretStore::default()),
    ));
    let backend_catalog = backend_manager
        .save_backend(BackendDraft {
            id: None,
            display_name: format!("{label} real acceptance"),
            kind: BackendKind::ExternalOpenAi,
            engine: Some(adapter_id.engine),
            adapter_variant: Some(adapter_id.variant.clone()),
            api_root: api_root.to_owned(),
            auth_method: if api_key.is_some() {
                BackendAuthMethod::Bearer
            } else {
                BackendAuthMethod::None
            },
            api_key: api_key.map(str::to_owned),
        })
        .await
        .expect("persist exact external backend");
    let backend_id = backend_catalog.backends[0].id.clone();
    let engine_root: PathBuf = std::env::temp_dir().join(format!(
        "hal100-inference-acceptance-{}",
        Uuid::new_v4().simple()
    ));
    let managed_engine = Arc::new(
        LlamaCppManager::new(database.clone(), gateway.clone(), engine_root.clone())
            .expect("managed engine boundary"),
    );
    let registry = registry_for_acceptance_lifecycle(adapter_id, &host, accelerator);
    let support_cell = hal100_protocol::RuntimeProfileSupportCell {
        platform: host.platform,
        architecture: host.architecture,
        accelerator,
        deployment: hal100_protocol::InferenceDeployment::Local,
    };
    let manager = RuntimeProfileManager::with_external_context(
        database,
        managed_engine,
        host,
        backend_manager,
        gateway.clone(),
        registry,
    );
    let catalog = manager
        .save_external(ExternalRuntimeProfileDraft {
            name: format!("{label} real acceptance"),
            description: "fixed platform service with live protocol qualification".to_owned(),
            backend_id,
            model_id: model_id.to_owned(),
            expected_evidence,
            support_cell: Some(support_cell),
        })
        .await
        .expect("save qualified runtime profile");
    let profile = catalog.profiles.first().expect("saved profile");
    let plan = manager
        .plan_activation_verified(&profile.id)
        .await
        .expect("plan verified activation");
    let result = manager
        .apply_activation(&plan.plan_id)
        .await
        .expect("activate and post-verify external profile");
    assert_eq!(result.active_model_id, model_id);
    assert_eq!(
        result.ownership,
        hal100_protocol::InferenceEngineOwnership::External
    );
    assert!(
        manager
            .verify_active_profile(&profile.id)
            .await
            .expect("verify active external profile")
    );
    let gateway_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("acceptance Gateway listener");
    let gateway_address = gateway_listener
        .local_addr()
        .expect("acceptance Gateway address");
    let gateway_task = tokio::spawn(async move {
        axum::serve(gateway_listener, gateway_router(gateway))
            .await
            .expect("acceptance Gateway server");
    });
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("acceptance Gateway client")
        .post(format!("http://{gateway_address}/v1/chat/completions"))
        .bearer_auth(client_key)
        .json(&json!({
            "model": "hal100-active",
            "messages": [{
                "role": "user",
                "content": "Call hal100_protocol_probe exactly once with a short value."
            }],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "hal100_protocol_probe",
                    "description": "HAL100 bounded protocol qualification probe",
                    "parameters": {
                        "type": "object",
                        "properties": {"value": {"type": "string"}},
                        "required": ["value"],
                        "additionalProperties": false
                    }
                }
            }],
            "tool_choice": {
                "type": "function",
                "function": {"name": "hal100_protocol_probe"}
            },
            "parallel_tool_calls": false,
            "temperature": 0,
            "max_tokens": 64,
            "stream": false
        }))
        .send()
        .await
        .expect("acceptance Gateway tool response");
    assert!(response.status().is_success());
    let response = response
        .json::<Value>()
        .await
        .expect("acceptance Gateway tool JSON");
    let tool_calls = response["choices"][0]["message"]["tool_calls"]
        .as_array()
        .expect("acceptance Gateway tool calls");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(
        tool_calls[0]["function"]["name"].as_str(),
        Some("hal100_protocol_probe")
    );
    let arguments = tool_calls[0]["function"]["arguments"]
        .as_str()
        .expect("OpenAI string tool arguments after Gateway compatibility");
    assert!(serde_json::from_str::<Value>(arguments).is_ok_and(|arguments| arguments.is_object()));
    gateway_task.abort();
    let _ = std::fs::remove_dir_all(engine_root);
}

/// Execute the shared bounded repeated/concurrent request probe for a fixed external target.
///
/// This helper is intentionally test-only and accepts only a `VerifiedEngineTarget` constructed
/// by Rust. It returns aggregate evidence suitable for the acceptance artifact and never exposes
/// prompts, responses, credentials or endpoint details.
#[allow(dead_code)]
pub async fn verify_external_stability(
    target: &VerifiedEngineTarget,
    model_id: &str,
    label: &str,
) -> OpenAiStabilityObservation {
    verify_external_stability_with_options(
        target,
        model_id,
        label,
        OpenAiQualificationOptions::default(),
    )
    .await
}

#[allow(dead_code)]
pub async fn verify_external_stability_with_options(
    target: &VerifiedEngineTarget,
    model_id: &str,
    label: &str,
    options: OpenAiQualificationOptions,
) -> OpenAiStabilityObservation {
    let http = BoundedEngineHttpClient::new("acceptance-stability").expect("stability HTTP client");
    let observation = qualify_openai_runtime_stability(&http, target, model_id, &options)
        .await
        .unwrap_or_else(|error| panic!("{label} bounded stability probe failed: {error}"));
    assert_eq!(observation.attempts, 20);
    assert_eq!(observation.concurrency, 4);
    println!(
        "{label} bounded stability probe passed: attempts={} concurrency={} max_latency_ms={}",
        observation.attempts, observation.concurrency, observation.max_latency_ms
    );
    observation
}

/// Exercise the shared HAL100 control-plane interruption and recovery contracts.
///
/// These checks intentionally use a deterministic local stream fixture instead of claiming that
/// an external engine exposes a controllable lifecycle. They prove the properties that every
/// formally supported target relies on: a forced route switch cancels the old upstream generation,
/// a timed-out safe switch leaves the previous route active, and a restart recovers a durable
/// activation journal before new work is accepted. The returned value contains only booleans and
/// is suitable for the redacted acceptance artifact.
#[allow(dead_code)]
pub async fn verify_control_plane_resilience() -> InferenceEngineAcceptanceResilience {
    let (cancellation_verified, failed_switch_rollback_verified) =
        verify_gateway_interruption_paths().await;
    let restart_compensation_verified = verify_restart_compensation().await;
    let resilience = InferenceEngineAcceptanceResilience {
        cancellation_verified,
        failed_switch_rollback_verified,
        restart_compensation_verified,
    };
    assert!(
        resilience.all_passed(),
        "shared control-plane resilience contract must pass before acceptance evidence is emitted"
    );
    resilience
}

const RESILIENCE_CLIENT_KEY: &str = "hal100_resilience_0123456789abcdef";

#[derive(Clone)]
struct ResilienceBackendState {
    stream_drops: Arc<AtomicUsize>,
}

struct ResilienceDropGuard(Arc<AtomicUsize>);

impl Drop for ResilienceDropGuard {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::AcqRel);
    }
}

async fn resilience_chat(
    State(state): State<ResilienceBackendState>,
    Json(request): Json<Value>,
) -> Response {
    if request
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return resilience_stream(state.stream_drops);
    }
    Json(json!({
        "id": "resilience-complete",
        "object": "chat.completion",
        "model": request.get("model").and_then(Value::as_str).unwrap_or("resilience-model"),
        "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
    }))
    .into_response()
}

fn resilience_stream(stream_drops: Arc<AtomicUsize>) -> Response {
    let body = Body::from_stream(stream::unfold(
        (0_u8, ResilienceDropGuard(stream_drops)),
        |(index, guard)| async move {
            match index {
                0 => Some((
                    Ok::<_, Infallible>(Bytes::from_static(
                        b"data: {\"choices\":[{\"delta\":{\"content\":\"first\"}}]}\n\n",
                    )),
                    (1, guard),
                )),
                1 => {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    Some((
                        Ok::<_, Infallible>(Bytes::from_static(b"data: [DONE]\n\n")),
                        (2, guard),
                    ))
                }
                _ => None,
            }
        },
    ));
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(body)
        .expect("resilience SSE response")
}

async fn wait_for_stream_drops(counter: &AtomicUsize, expected: usize) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while counter.load(Ordering::Acquire) < expected {
        assert!(
            Instant::now() < deadline,
            "resilience stream fixture did not observe upstream cancellation"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn verify_gateway_interruption_paths() -> (bool, bool) {
    let stream_drops = Arc::new(AtomicUsize::new(0));
    let backend_router = Router::new()
        .route("/v1/chat/completions", post(resilience_chat))
        .with_state(ResilienceBackendState {
            stream_drops: stream_drops.clone(),
        });
    let backend_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("resilience backend listener");
    let backend_address = backend_listener
        .local_addr()
        .expect("resilience backend address");
    let backend_task = tokio::spawn(async move {
        axum::serve(backend_listener, backend_router)
            .await
            .expect("resilience backend server");
    });

    let database = Arc::new(Database::open_in_memory().expect("resilience database"));
    let credential = stored_client_credential(
        "resilience-client",
        "resilience",
        "Resilience probe",
        RESILIENCE_CLIENT_KEY,
    )
    .expect("resilience client credential");
    database
        .upsert_client_credential(&credential, 1)
        .expect("persist resilience credential");
    let credentials = CredentialRegistry::new(
        database
            .load_client_credentials()
            .expect("load resilience credential"),
    );
    let usage_writer = UsageWriter::start(database.clone());
    let backend_root = format!("http://{backend_address}/v1");
    let original_backend = BackendConfig::new("resilience-original", &backend_root, None)
        .expect("resilience original backend");
    let gateway_state = GatewayState::new(
        Some(original_backend.clone()),
        credentials,
        usage_writer.clone(),
    )
    .expect("resilience gateway state");
    let gateway_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("resilience gateway listener");
    let gateway_address = gateway_listener
        .local_addr()
        .expect("resilience gateway address");
    let gateway_task = {
        let state = gateway_state.clone();
        tokio::spawn(async move {
            axum::serve(gateway_listener, gateway_router(state))
                .await
                .expect("resilience gateway server");
        })
    };
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("resilience client");
    let gateway_root = format!("http://{gateway_address}");

    let response = client
        .post(format!("{gateway_root}/v1/chat/completions"))
        .bearer_auth(RESILIENCE_CLIENT_KEY)
        .json(&json!({
            "model": "resilience-cancel-model",
            "messages": [{"role": "user", "content": "cancel"}],
            "stream": true
        }))
        .send()
        .await
        .expect("resilience cancellation response");
    let request_id = response
        .headers()
        .get("x-hal100-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .expect("resilience request id");
    let mut body = response.bytes_stream();
    assert!(body.next().await.is_some_and(|item| item.is_ok()));
    let replacement = BackendConfig::new("resilience-replacement", &backend_root, None)
        .expect("resilience replacement backend");
    gateway_state
        .force_replace_backend(Some(replacement))
        .await
        .expect("force resilience route switch");
    let cancellation_verified = body.next().await.is_some_and(|item| item.is_err());
    drop(body);
    wait_for_stream_drops(&stream_drops, 1).await;
    usage_writer
        .flush(Duration::from_secs(1))
        .expect("flush resilience cancellation usage");
    let usage = database
        .usage_request(&request_id)
        .expect("resilience usage record");
    assert_eq!(usage.status, "failed");
    assert_eq!(usage.error_category.as_deref(), Some("forced_route_switch"));

    gateway_state
        .force_replace_backend(Some(original_backend.clone()))
        .await
        .expect("restore original resilience route");
    let response = client
        .post(format!("{gateway_root}/v1/chat/completions"))
        .bearer_auth(RESILIENCE_CLIENT_KEY)
        .json(&json!({
            "model": "resilience-drain-model",
            "messages": [{"role": "user", "content": "drain"}],
            "stream": true
        }))
        .send()
        .await
        .expect("resilience drain response");
    let mut drain_body = response.bytes_stream();
    assert!(drain_body.next().await.is_some_and(|item| item.is_ok()));
    let drain_replacement = BackendConfig::new("resilience-drain-replacement", &backend_root, None)
        .expect("resilience drain replacement backend");
    let switch_result = gateway_state
        .replace_active_route_when_idle(
            Some(ActiveGatewayRoute::passthrough(drain_replacement)),
            Duration::from_millis(25),
        )
        .await;
    let failed_switch_rollback_verified = matches!(
        switch_result,
        Err(GatewayRouteSwitchError::DrainTimeout { .. })
    ) && gateway_state
        .active_route()
        .is_some_and(|route| route.backend().id() == "resilience-original");
    drop(drain_body);
    wait_for_stream_drops(&stream_drops, 2).await;

    gateway_task.abort();
    backend_task.abort();
    (cancellation_verified, failed_switch_rollback_verified)
}

async fn verify_restart_compensation() -> bool {
    let database = Arc::new(Database::open_in_memory().expect("restart compensation database"));
    let gateway = GatewayState::new(
        None,
        CredentialRegistry::new(Vec::new()),
        UsageWriter::start(database.clone()),
    )
    .expect("restart compensation gateway");
    let engine_root = std::env::temp_dir().join(format!(
        "hal100-resilience-restart-{}",
        Uuid::new_v4().simple()
    ));
    let engine = Arc::new(
        LlamaCppManager::new(database.clone(), gateway, engine_root.clone())
            .expect("restart compensation engine"),
    );
    let manager = RuntimeProfileManager::new(database.clone(), engine);
    let repository = RuntimeActivationJournalRepository::new(database.clone());
    let journal = StoredRuntimeActivationJournal {
        id: format!("resilience-journal-{}", Uuid::new_v4().simple()),
        profile_id: "resilience-profile".to_owned(),
        phase: RuntimeActivationPhase::Journaled,
        previous_route: None,
        previous_managed_model_id: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    };
    repository.begin(&journal).expect("begin restart journal");
    assert!(
        repository
            .transition(
                &journal.id,
                RuntimeActivationPhase::Journaled,
                RuntimeActivationPhase::RouteSwitched,
                2,
            )
            .expect("persist switched restart journal")
    );
    let recovered = manager
        .recover_incomplete_activation()
        .await
        .expect("recover restart journal");
    let pending = repository.pending().expect("read restart journal");
    let _ = std::fs::remove_dir_all(engine_root);
    recovered && pending.is_empty()
}

/// Probe the actual host used by a live acceptance run and require the exact declared support
/// coordinates. Acceptance artifacts must never use a synthetic CPU/GPU/memory snapshot: the
/// native probe is the only source of platform, architecture and accelerator evidence that may
/// enter the runtime-profile lifecycle or a reviewed ledger record.
#[allow(dead_code)]
pub fn probe_real_acceptance_host(
    expected_platform: InferencePlatform,
    expected_architecture: InferenceArchitecture,
    required_accelerator: InferenceAccelerator,
    label: &str,
) -> HostCapabilitySnapshot {
    let host = NativeSystemProbe
        .capability_snapshot(std::path::Path::new("."))
        .unwrap_or_else(|error| panic!("{label} native host probe failed: {error}"));
    assert_eq!(
        host.platform, expected_platform,
        "{label} acceptance must run on the declared platform"
    );
    assert_eq!(
        host.architecture, expected_architecture,
        "{label} acceptance must run on the declared architecture"
    );
    assert!(
        host.accelerators.contains(&required_accelerator),
        "{label} native host probe did not expose required accelerator {:?}; candidates={:?}",
        required_accelerator,
        host.accelerators
    );
    host
}

/// Emit a redacted acceptance-run artifact only when explicitly requested by the operator.
///
/// `HAL100_ACCEPTANCE_EVIDENCE_EMIT=1` prints a compact JSON record to stdout for review. Setting
/// both `HAL100_ACCEPTANCE_EVIDENCE_WRITE=1` and `HAL100_ACCEPTANCE_EVIDENCE_OUT=/path/file.json`
/// writes one create-new artifact; an existing file is never overwritten. With neither opt-in,
/// this helper is a no-op, so ordinary ignored tests cannot mutate the workspace.
#[allow(dead_code)]
pub struct AcceptanceRunInput<'a> {
    pub adapter_id: &'a EngineAdapterId,
    pub target: &'a VerifiedEngineTarget,
    pub host: &'a HostCapabilitySnapshot,
    pub accelerator: InferenceAccelerator,
    pub engine_version: Option<&'a str>,
    pub deployment_fingerprint: Option<&'a str>,
    pub model_id: &'a str,
    pub model_evidence: &'a hal100_protocol::RuntimeProfileEvidence,
    pub protocol_capability_hash: &'a str,
    pub test_source: &'a str,
    pub lifecycle_verified: bool,
    pub stability: Option<OpenAiStabilityObservation>,
    /// Optional control-plane resilience evidence. Missing or incomplete checks intentionally
    /// keep the emitted artifact review-only and prevent formal promotion.
    pub resilience: Option<InferenceEngineAcceptanceResilience>,
}

/// Deterministic native-attestation fixture for structural pipeline tests only.
/// Real live-acceptance entry points always use the snapshot returned by `NativeSystemProbe`.
#[allow(dead_code)]
pub fn fixture_host_attestation(
    platform: InferencePlatform,
    architecture: InferenceArchitecture,
    accelerator: InferenceAccelerator,
) -> InferenceEngineAcceptanceHostAttestation {
    let mut accelerators = vec![InferenceAccelerator::Cpu];
    if accelerator != InferenceAccelerator::Cpu {
        accelerators.push(accelerator);
    }
    let host = HostCapabilitySnapshot {
        platform,
        architecture,
        cpu_brand: "HAL100 acceptance fixture CPU".to_owned(),
        device_model: "HAL100AcceptanceFixture".to_owned(),
        total_memory_bytes: 32 * 1024 * 1024 * 1024,
        physical_cpu_cores: 8,
        logical_cpu_cores: 16,
        accelerators,
        model_storage_path: "/redacted-fixture-storage".to_owned(),
        model_storage_available_bytes: 100,
        probe_revision: "host-capabilities-v3".to_owned(),
    };
    InferenceEngineAcceptanceHostAttestation::from_host_snapshot(&host, accelerator)
        .expect("bounded fixture host attestation")
}

#[allow(dead_code)]
pub fn emit_acceptance_run_if_opted_in(
    input: AcceptanceRunInput<'_>,
) -> Result<bool, InferenceEngineAcceptanceEvidenceError> {
    let emit_stdout = std::env::var("HAL100_ACCEPTANCE_EVIDENCE_EMIT").as_deref() == Ok("1");
    let write_file = std::env::var("HAL100_ACCEPTANCE_EVIDENCE_WRITE").as_deref() == Ok("1");
    if !emit_stdout && !write_file {
        return Ok(false);
    }
    if input.target.adapter_id() != input.adapter_id {
        return Err(InferenceEngineAcceptanceEvidenceError::InvalidRun);
    }
    let registry = ExternalInferenceEngineRegistry::standard()
        .map_err(|_| InferenceEngineAcceptanceEvidenceError::InvalidRun)?;
    let adapter = registry
        .adapter_by_id(input.adapter_id)
        .ok_or(InferenceEngineAcceptanceEvidenceError::InvalidRun)?;
    if adapter.protocol_capability_hash().as_deref() != Some(input.protocol_capability_hash) {
        return Err(InferenceEngineAcceptanceEvidenceError::InvalidRun);
    }
    let observed_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .ok_or(InferenceEngineAcceptanceEvidenceError::InvalidRun)?;
    let host_attestation = InferenceEngineAcceptanceHostAttestation::from_host_snapshot(
        input.host,
        input.accelerator,
    )?;
    let host_fingerprint = host_attestation
        .device_class_fingerprint
        .as_deref()
        .ok_or(InferenceEngineAcceptanceEvidenceError::InvalidHostAttestation)?;
    let mut evidence = vec![
        InferenceEngineAcceptanceEvidence {
            kind: InferenceEngineSupportEvidenceKind::OfficialContract,
            source: "contracts/inference-engines/v1-support-matrix.json".to_owned(),
            assertion: "adapter manifest support cell resolved".to_owned(),
        },
        InferenceEngineAcceptanceEvidence {
            kind: InferenceEngineSupportEvidenceKind::ProtocolQualification,
            source: input.test_source.to_owned(),
            assertion: format!(
                "required OpenAI protocol capabilities passed; hash={}",
                input.protocol_capability_hash
            ),
        },
        InferenceEngineAcceptanceEvidence {
            kind: InferenceEngineSupportEvidenceKind::PlatformRuntime,
            source: input.test_source.to_owned(),
            assertion: format!(
                "native host attestation matched the declared support cell; device-class-sha256={host_fingerprint}"
            ),
        },
        InferenceEngineAcceptanceEvidence {
            kind: InferenceEngineSupportEvidenceKind::ModelDeploymentIdentity,
            source: input.test_source.to_owned(),
            assertion: "served model matched the requested model identity".to_owned(),
        },
    ];
    if input.lifecycle_verified {
        evidence.push(InferenceEngineAcceptanceEvidence {
            kind: InferenceEngineSupportEvidenceKind::RuntimeProfileLifecycle,
            source: input.test_source.to_owned(),
            assertion: "save/revalidate/activate/verify runtime profile lifecycle passed"
                .to_owned(),
        });
    }
    if input.stability.is_some() {
        evidence.push(InferenceEngineAcceptanceEvidence {
            kind: InferenceEngineSupportEvidenceKind::Stability,
            source: input.test_source.to_owned(),
            assertion: "bounded repeated/concurrent inference probe passed".to_owned(),
        });
    }
    let engine_version = input
        .engine_version
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let deployment_fingerprint = input
        .deployment_fingerprint
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    if engine_version.is_some() || deployment_fingerprint.is_some() {
        evidence.push(InferenceEngineAcceptanceEvidence {
            kind: InferenceEngineSupportEvidenceKind::EngineIdentity,
            source: input.test_source.to_owned(),
            assertion: "exact engine or deployment identity was observed by the live inspection"
                .to_owned(),
        });
    }
    let model_revision = format!(
        "model-id-sha256:{}",
        Sha256::digest(input.model_id.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    let qualified_model_evidence = deployment_fingerprint.as_ref().map_or_else(
        || input.model_evidence.clone(),
        |fingerprint| hal100_protocol::RuntimeProfileEvidence {
            kind: hal100_protocol::RuntimeProfileEvidenceKind::DeploymentFingerprint,
            algorithm: "engine-deployment-fingerprint-v1".to_owned(),
            value: fingerprint.clone(),
        },
    );
    let run = InferenceEngineAcceptanceRun {
        schema_version: hal100_infra::INFERENCE_ENGINE_ACCEPTANCE_RUN_SCHEMA_VERSION,
        run_id: format!("acceptance-run-{}", Uuid::new_v4().simple()),
        adapter_id: input.adapter_id.clone(),
        instance_id: input.target.key().instance_id().as_str().to_owned(),
        origin_fingerprint: input.target.origin().fingerprint_hex(),
        config_revision: input.target.key().config_revision(),
        protocol_capability_hash: input.protocol_capability_hash.to_owned(),
        platform: input.host.platform,
        architecture: input.host.architecture,
        accelerator: input.accelerator,
        deployment: InferenceDeployment::Local,
        outcome: InferenceEngineAcceptanceRunOutcome::Passed,
        observed_at_ms,
        engine_version,
        deployment_fingerprint,
        model_revision: Some(model_revision),
        host_summary: Some(format!(
            "{}/{}/{}",
            platform_key(input.host.platform),
            architecture_key(input.host.architecture),
            accelerator_key(input.accelerator)
        )),
        host_attestation,
        model_evidence:
            hal100_infra::InferenceEngineAcceptanceModelEvidence::from_runtime_evidence(
                &qualified_model_evidence,
            )?,
        stability: input
            .stability
            .map(|stability| InferenceEngineAcceptanceStability {
                workload_revision: stability.workload_revision.to_owned(),
                attempts: stability.attempts,
                concurrency: stability.concurrency,
                p95_latency_ms: stability.p95_latency_ms,
                max_latency_ms: stability.max_latency_ms,
                total_prompt_tokens: stability.total_prompt_tokens,
                total_completion_tokens: stability.total_completion_tokens,
                wall_time_ms: stability.wall_time_ms,
            }),
        resilience: input.resilience,
        evidence,
    };
    run.validate()?;
    if emit_stdout {
        let json = serde_json::to_string(&run)
            .map_err(|_| InferenceEngineAcceptanceEvidenceError::InvalidRun)?;
        println!("HAL100_ACCEPTANCE_RUN_JSON={json}");
    }
    if write_file {
        let output = std::env::var("HAL100_ACCEPTANCE_EVIDENCE_OUT")
            .map_err(|_| InferenceEngineAcceptanceEvidenceError::RunOutputUnavailable)?;
        write_acceptance_run_exclusive(PathBuf::from(output).as_path(), &run)?;
    }
    Ok(true)
}

#[allow(dead_code)]
const fn platform_key(platform: hal100_protocol::InferencePlatform) -> &'static str {
    match platform {
        hal100_protocol::InferencePlatform::MacOs => "macos",
        hal100_protocol::InferencePlatform::Windows => "windows",
        hal100_protocol::InferencePlatform::Linux => "linux",
    }
}

#[allow(dead_code)]
const fn architecture_key(architecture: hal100_protocol::InferenceArchitecture) -> &'static str {
    match architecture {
        hal100_protocol::InferenceArchitecture::Aarch64 => "aarch64",
        hal100_protocol::InferenceArchitecture::X86_64 => "x86_64",
    }
}

#[allow(dead_code)]
const fn accelerator_key(accelerator: InferenceAccelerator) -> &'static str {
    match accelerator {
        InferenceAccelerator::Cpu => "cpu",
        InferenceAccelerator::Metal => "metal",
        InferenceAccelerator::Cuda => "cuda",
        InferenceAccelerator::Rocm => "rocm",
        InferenceAccelerator::Vulkan => "vulkan",
        InferenceAccelerator::Sycl => "sycl",
        InferenceAccelerator::IntelGpu => "intel_gpu",
        InferenceAccelerator::IntelNpu => "intel_npu",
    }
}
