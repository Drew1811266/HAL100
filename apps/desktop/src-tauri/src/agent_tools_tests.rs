#[cfg(unix)]
use std::{fs, path::PathBuf};
use std::{
    future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, Response, StatusCode, header},
    routing::get,
};
#[cfg(unix)]
use hal100_core::ExternalAgentIntegrationId;
#[cfg(unix)]
use hal100_infra::{
    CredentialRegistry, Database, EnvironmentDiagnostics, ExternalModelProfileRegistry,
    GatewayState, HermesAgentIntegrationAdapter, HermesAgentPaths, LlamaCppManager,
    ManagedExternalAgentDeploymentManager, ModelDownloadManager, ModelRemovalManager,
    OpenClawIntegrationAdapter, OpenClawPaths, OpenCodeManager, OpenCodePaths,
    PiCodingAgentIntegrationAdapter, PiCodingAgentPaths, RemoteModelCatalog, UsageWriter,
};
use hal100_infra::{ExternalInferenceEngineRegistry, ModelDownloadError, RemoteModelCatalogError};
use hal100_protocol::{
    AGENT_RPC_MAX_TOOL_RESULT_BYTES, HostCapabilitySnapshot, InferenceAccelerator,
    InferenceArchitecture, InferencePlatform, RuntimeProfileActivationPlan,
    RuntimeProfileAdapterBinding, RuntimeProfileCatalog, RuntimeProfileEvidence,
    RuntimeProfileEvidenceKind, RuntimeProfileModelDigestKind, RuntimeProfileReadiness,
    RuntimeProfileSummary, ToolCallRequestPayload,
};
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use hal100_protocol::{
    DownloadSource, MODEL_CATALOG_SEARCH_TOOL, MODEL_REPOSITORY_INSPECTION_TOOL,
    ModelDownloadState, PLAN_MODEL_DOWNLOAD_TOOL,
};
#[cfg(unix)]
use hal100_protocol::{
    EXTERNAL_AGENT_STATUS_TOOL, OPERATIONAL_HEALTH_OBSERVATION_TOOL, OPERATIONAL_HISTORY_TOOL,
    PLAN_EXTERNAL_AGENT_CONFIGURATION_TOOL, PLAN_EXTERNAL_AGENT_INSTALLATION_TOOL,
    ToolCallResultStatus,
};
use serde_json::{Value, json};
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use sha2::{Digest, Sha256};
#[cfg(unix)]
use uuid::Uuid;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use std::path::Path;

use super::*;

#[cfg(unix)]
struct TestDirectory(PathBuf);

#[cfg(unix)]
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

#[cfg(unix)]
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
    assert_eq!(
        AgentToolExecutionError::RuntimeProfile(RuntimeProfileManagerError::ProfileChanged).code(),
        "runtime_profile_changed"
    );
    assert_eq!(
        AgentToolExecutionError::RuntimeProfile(RuntimeProfileManagerError::ProfileNeedsRepair)
            .code(),
        "runtime_profile_needs_repair"
    );
    assert_eq!(
        AgentToolExecutionError::RuntimeProfile(RuntimeProfileManagerError::SupportCellNotProven)
            .code(),
        "runtime_profile_runtime_device_unproven"
    );
    assert_eq!(
        AgentToolExecutionError::RuntimeProfile(RuntimeProfileManagerError::ExternalEngine(
            hal100_infra::ExternalEngineAdapterError::Unreachable,
        ))
        .code(),
        "runtime_profile_engine_unreachable"
    );
}

#[test]
fn agent_runtime_catalog_keeps_managed_and_external_profiles_without_sensitive_identity() {
    let common = |id: &str,
                  ownership: InferenceEngineOwnership,
                  backend_id: Option<&str>,
                  context_window_tokens: Option<u32>| RuntimeProfileSummary {
        id: id.to_owned(),
        name: id.to_owned(),
        description: "verified fixture".to_owned(),
        spec_version: 3,
        ownership,
        backend_id: backend_id.map(str::to_owned),
        backend_api_root: backend_id.map(|_| "http://127.0.0.1:11434/v1/".to_owned()),
        model_id: format!("model-{id}"),
        model_display_name: format!("Model {id}"),
        model_digest_kind: if ownership == InferenceEngineOwnership::Managed {
            RuntimeProfileModelDigestKind::Sha256
        } else {
            RuntimeProfileModelDigestKind::OllamaDigest
        },
        engine: if ownership == InferenceEngineOwnership::Managed {
            "llama_cpp".to_owned()
        } else {
            "ollama".to_owned()
        },
        engine_version: "test".to_owned(),
        capacity_tier: context_window_tokens.map(|_| "standard".to_owned()),
        context_window_tokens,
        capacity_revision: context_window_tokens.map(|_| "test".to_owned()),
        adapter_binding: RuntimeProfileAdapterBinding {
            variant: if ownership == InferenceEngineOwnership::Managed {
                "hal100-managed-metal".to_owned()
            } else {
                "official-loopback-api".to_owned()
            },
            contract_revision: hal100_protocol::ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
            backend_config_revision: backend_id.map(|_| 1),
            origin_fingerprint: backend_id.map(|_| "a".repeat(64)),
            protocol_capability_hash: Some("b".repeat(64)),
            support_cell: None,
        },
        evidence: RuntimeProfileEvidence {
            kind: RuntimeProfileEvidenceKind::ContentDigest,
            algorithm: if ownership == InferenceEngineOwnership::Managed {
                "sha256".to_owned()
            } else {
                "ollama-digest".to_owned()
            },
            value: "c".repeat(64),
        },
        reviewed_performance: (ownership == InferenceEngineOwnership::External).then_some(
            hal100_protocol::RuntimeProfileReviewedPerformance {
                workload_revision: "openai-short-chat-v1".to_owned(),
                attempts: 20,
                concurrency: 4,
                p95_latency_ms: 90,
                max_latency_ms: 100,
                total_prompt_tokens: 40,
                total_completion_tokens: 20,
                wall_time_ms: 500,
                sample_completion_tokens_per_second_milli: 40_000,
                reviewed_at_ms: 1,
            },
        ),
        readiness: RuntimeProfileReadiness::Ready,
        issues: Vec::new(),
        verified_at_ms: 1,
        last_activated_at_ms: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    };
    let profiles = build_agent_runtime_profiles(RuntimeProfileCatalog {
        profiles: vec![
            common(
                "managed",
                InferenceEngineOwnership::Managed,
                None,
                Some(32_768),
            ),
            common(
                "external",
                InferenceEngineOwnership::External,
                Some("saved-ollama"),
                None,
            ),
        ],
        active_profile_id: Some("external".to_owned()),
        can_save_current: false,
    });

    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].context_window_tokens, Some(32_768));
    assert_eq!(profiles[1].ownership, InferenceEngineOwnership::External);
    assert_eq!(profiles[1].backend_id.as_deref(), Some("saved-ollama"));
    assert_eq!(profiles[1].adapter_variant, "official-loopback-api");
    assert_eq!(
        profiles[1].adapter_contract_revision,
        hal100_protocol::ENGINE_ADAPTER_CONTRACT_REVISION
    );
    assert_eq!(profiles[1].context_window_tokens, None);
    assert_eq!(
        profiles[1]
            .reviewed_performance
            .as_ref()
            .map(|performance| performance.p95_latency_ms),
        Some(90)
    );
    assert!(profiles[1].active);
    let rendered = serde_json::to_string(&profiles).expect("Agent profile catalog JSON");
    assert!(rendered.contains("reviewedPerformance"));
    assert!(!rendered.contains("127.0.0.1"));
    assert!(!rendered.to_ascii_lowercase().contains("digest"));
}

#[test]
fn agent_runtime_engine_capabilities_project_the_full_registry_without_endpoints_or_digests() {
    let registry = ExternalInferenceEngineRegistry::standard()
        .expect("standard external engine registry")
        .manifest_registry();
    let host = HostCapabilitySnapshot {
        platform: InferencePlatform::MacOs,
        architecture: InferenceArchitecture::Aarch64,
        cpu_brand: "Apple".to_owned(),
        device_model: "Apple M1".to_owned(),
        total_memory_bytes: 16 * 1024 * 1024 * 1024,
        physical_cpu_cores: 8,
        logical_cpu_cores: 8,
        accelerators: vec![InferenceAccelerator::Metal],
        model_storage_path: "/private/should-not-leak".to_owned(),
        model_storage_available_bytes: 1,
        probe_revision: "test".to_owned(),
    };
    let capabilities = build_agent_engine_capabilities(registry, &host, &[]);

    assert_eq!(capabilities.len(), 13);
    assert_eq!(
        capabilities
            .iter()
            .map(|capability| capability.engine)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        8,
        "adapter variants must not be conflated with engine kinds"
    );
    assert_eq!(
        capabilities
            .iter()
            .filter(|capability| capability.engine.storage_key() == "openvino")
            .count(),
        3
    );
    assert_eq!(
        capabilities
            .iter()
            .filter(|capability| capability.engine.storage_key() == "mlc-llm")
            .map(|capability| capability.adapter_variant.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([
            "official-openai-cuda",
            "official-openai-metal",
            "official-openai-rocm",
            "official-openai-vulkan",
        ])
    );
    assert!(capabilities.iter().any(|capability| capability.compatible));
    let ollama = capabilities
        .iter()
        .find(|capability| capability.engine.storage_key() == "ollama")
        .expect("Ollama capability");
    assert_eq!(ollama.adapter_variant, "official-loopback-api");
    assert_eq!(ollama.engine_key, "ollama");
    assert_eq!(ollama.saved_profile_count, 0);
    assert!(
        ollama
            .support_cells
            .iter()
            .any(|cell| cell.accelerator == InferenceAccelerator::Metal)
    );
    let vllm = capabilities
        .iter()
        .find(|capability| capability.engine.storage_key() == "vllm")
        .expect("vLLM capability");
    assert!(!vllm.compatible);

    let rendered = serde_json::to_string(&capabilities).expect("Agent capability JSON");
    for forbidden in [
        "apiRoot",
        "digest",
        "credential",
        "apiKey",
        "command",
        "path",
        "private/should-not-leak",
    ] {
        assert!(
            !rendered
                .to_ascii_lowercase()
                .contains(&forbidden.to_ascii_lowercase())
        );
    }
}

#[test]
fn external_runtime_profile_plan_keeps_live_verification_and_unknown_capacity_explicit() {
    let pending = build_runtime_profile_activation_agent_plan(
        "agent-run-external-profile",
        RuntimeProfileActivationPlan {
            plan_id: "rust-plan-external".to_owned(),
            expires_at_ms: 50_000,
            profile_id: "runtime-profile-external".to_owned(),
            profile_name: "外部代码助手".to_owned(),
            model_id: "qwen3:8b".to_owned(),
            model_display_name: "qwen3:8b".to_owned(),
            ownership: InferenceEngineOwnership::External,
            backend_id: Some("saved-ollama".to_owned()),
            engine: "ollama".to_owned(),
            engine_version: "0.12.6".to_owned(),
            support_cell: None,
            context_window_tokens: None,
            current_backend_id: None,
            current_model_id: None,
            current_model_name: None,
            issues: Vec::new(),
            action_summary: "切换到外部方案".to_owned(),
            requires_confirmation: true,
        },
    )
    .expect("external Agent plan");

    assert!(matches!(
        pending.executor,
        AgentActionExecutor::ActivateRuntimeProfile {
            ref profile_id,
            ref activation_plan_id,
        } if profile_id == "runtime-profile-external"
            && activation_plan_id == "rust-plan-external"
    ));
    assert!(pending.plan.requires_native_confirmation);
    assert_eq!(
        pending.plan.current_state.as_deref(),
        Some("当前没有活动模型")
    );
    assert!(
        pending
            .plan
            .details
            .iter()
            .any(|detail| detail == "上下文窗口：由外部引擎决定")
    );
    assert!(
        pending
            .plan
            .details
            .iter()
            .any(|detail| detail.contains("用户外部后端 saved-ollama"))
    );
    assert!(
        pending
            .plan
            .details
            .iter()
            .any(|detail| detail.contains("Rust 实时复检"))
    );
    let rendered = serde_json::to_string(&pending.plan).expect("Agent plan JSON");
    assert!(!rendered.contains("127.0.0.1"));
    assert!(!rendered.to_ascii_lowercase().contains("digest"));
}

#[test]
fn model_stop_plan_binds_only_the_exact_current_rust_runtime_state() {
    let status = LlamaCppStatus {
        version: "b10218".to_owned(),
        install_state: EngineInstallState::Installed,
        runtime_state: hal100_protocol::EngineRuntimeState::Running,
        active_model_id: Some("model-a".to_owned()),
        active_model_name: Some("Qwen fixture".to_owned()),
        port: Some(12345),
        last_error_code: None,
    };
    let pending = build_model_stop_plan_from_status("run-stop", "model-a", &status)
        .expect("exact current model stop plan");
    assert_eq!(pending.plan.action_kind, AgentActionKind::StopModel);
    assert_eq!(pending.plan.target_id, "model-a");
    assert!(pending.plan.requires_native_confirmation);
    assert!(matches!(
        pending.executor,
        AgentActionExecutor::StopModel { ref model_id } if model_id == "model-a"
    ));

    assert!(build_model_stop_plan_from_status("run-stop", "model-b", &status).is_none());
    let mut stopped = status;
    stopped.runtime_state = hal100_protocol::EngineRuntimeState::Stopped;
    stopped.active_model_id = None;
    assert!(build_model_stop_plan_from_status("run-stop", "model-a", &stopped).is_none());
}

#[cfg(unix)]
#[test]
fn external_agent_tools_bind_the_prompt_target_and_never_return_local_paths() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("test runtime");
    let temp = TestDirectory::new();
    let home = temp.0.join("home");
    let model_storage_path = temp.0.join("models");
    fs::create_dir_all(&model_storage_path).expect("model storage");
    let database = Arc::new(Database::open(temp.0.join("hal100.sqlite")).expect("database"));
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
    let mut open_code_paths = OpenCodePaths::for_macos(&home, &temp.0);
    open_code_paths.binary_candidates.clear();
    let open_code = Arc::new(OpenCodeManager::with_gateway_base_url(
        database.clone(),
        credentials.clone(),
        open_code_paths,
        "http://127.0.0.1:10100/v1".to_owned(),
    ));
    let profiles = ExternalModelProfileRegistry::conservative_managed_route();
    let mut pi_paths = PiCodingAgentPaths::for_macos(&home, &temp.0);
    pi_paths.binary_candidates.clear();
    let pi_coding_agent = Arc::new(PiCodingAgentIntegrationAdapter::with_gateway_base_url(
        database.clone(),
        credentials.clone(),
        profiles.clone(),
        pi_paths,
        "http://127.0.0.1:10100/v1".to_owned(),
    ));
    let mut openclaw_paths = OpenClawPaths::for_macos(&home, &temp.0);
    openclaw_paths.binary_candidates.clear();
    let openclaw = Arc::new(OpenClawIntegrationAdapter::with_gateway_base_url(
        database.clone(),
        credentials.clone(),
        profiles.clone(),
        openclaw_paths,
        "http://127.0.0.1:10100/v1".to_owned(),
    ));
    let mut hermes_paths = HermesAgentPaths::for_macos(&home, &temp.0);
    hermes_paths.binary_candidates.clear();
    let hermes_agent = Arc::new(HermesAgentIntegrationAdapter::with_gateway_base_url(
        database.clone(),
        credentials,
        profiles,
        hermes_paths,
        "http://127.0.0.1:10100/v1".to_owned(),
    ));
    let removals = Arc::new(ModelRemovalManager::new(
        database.clone(),
        model_storage_path.clone(),
    ));
    let diagnostics = Arc::new(EnvironmentDiagnostics::new(
        database.clone(),
        engine.clone(),
        open_code.clone(),
        pi_coding_agent.clone(),
        openclaw.clone(),
        hermes_agent.clone(),
        gateway.clone(),
    ));
    let catalog = Arc::new(RemoteModelCatalog::new().expect("remote catalog"));
    let downloads = Arc::new(
        ModelDownloadManager::new(
            database.clone(),
            catalog.clone(),
            model_storage_path.clone(),
        )
        .expect("download manager"),
    );
    database
        .insert_audit_event(
            "agent_action_failed",
            "agent_action_plan",
            "/Users/private/secret-target",
            &json!({
                "errorCode": "configuration_validation_failed",
                "action": "configure_external_agent",
                "reason": "adapter_rejected",
                "prompt": "secret prompt must not escape"
            })
            .to_string(),
            now_ms(),
        )
        .expect("audit fixture");
    let fake_npm = temp.0.join("bin/npm");
    fs::create_dir_all(fake_npm.parent().expect("fake npm parent")).expect("fake npm directory");
    fs::write(
        &fake_npm,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf '11.5.2\n'
  exit 0
fi
if [ "$1" = "view" ]; then
  printf '%s\n' '{"name":"@earendil-works/pi-coding-agent","version":"0.84.2","bin":{"pi":"dist/cli.js"},"dist":{"integrity":"sha512-l4E+B7hgXKWddRo8bC/eSue2aWZjEgJ9xIpf5p0Og+lq8a2TArCwJ0HCoCPCgaBP/tN4zbYH/wOwvx9pJpeLCA=="}}'
  exit 0
fi
exit 9
"#,
    )
    .expect("fake npm");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake_npm, fs::Permissions::from_mode(0o700))
            .expect("fake npm executable");
    }
    let managed_deployment = Arc::new(ManagedExternalAgentDeploymentManager::new(
        database.clone(),
        temp.0.join("managed-external-agents"),
        vec![fake_npm],
    ));
    let runtime_profiles = Arc::new(RuntimeProfileManager::new(database.clone(), engine.clone()));
    let executor = AgentToolExecutor::new(
        model_storage_path,
        database.clone(),
        engine,
        open_code,
        pi_coding_agent,
        openclaw,
        hermes_agent,
        removals,
        diagnostics,
        catalog,
        downloads,
        managed_deployment,
        gateway,
        AgentActionPlanStore::new(),
        runtime_profiles,
    );
    let mut run = executor.start_run(
        "agent-model-download-run".to_owned(),
        Some(ExternalAgentIntegrationId::PiCodingAgent),
        None,
        runtime.handle().clone(),
        Arc::new(AtomicBool::new(false)),
    );

    let status = run
        .handle(&request(
            "tool-external-status",
            EXTERNAL_AGENT_STATUS_TOOL,
            json!({ "integrationId": "pi-coding-agent" }),
        ))
        .expect("status execution");
    assert_eq!(status.status, ToolCallResultStatus::Success);
    let output = status.output.expect("sanitized status");
    assert_eq!(output["integrationId"], "pi-coding-agent");
    assert_eq!(output["installed"], false);
    assert!(output.get("binaryPath").is_none());
    assert!(output.get("configPath").is_none());
    assert!(output.get("warnings").is_none());
    assert!(
        !output
            .to_string()
            .contains(temp.0.to_string_lossy().as_ref())
    );

    let mismatch = run
        .handle(&request(
            "tool-external-mismatch",
            EXTERNAL_AGENT_STATUS_TOOL,
            json!({ "integrationId": "openclaw" }),
        ))
        .expect("mismatch response");
    assert_eq!(mismatch.status, ToolCallResultStatus::Error);
    assert_eq!(
        mismatch.error.expect("mismatch error").code,
        "external_agent_target_mismatch"
    );

    let unavailable = run
        .handle(&request(
            "tool-external-plan",
            PLAN_EXTERNAL_AGENT_CONFIGURATION_TOOL,
            json!({ "integrationId": "pi-coding-agent" }),
        ))
        .expect("plan response");
    assert_eq!(unavailable.status, ToolCallResultStatus::Error);
    assert!(run.action_plans().is_empty());

    let history = run
        .handle(&request(
            "tool-operational-history",
            OPERATIONAL_HISTORY_TOOL,
            json!({ "target": "recent" }),
        ))
        .expect("operational history");
    assert_eq!(history.status, ToolCallResultStatus::Success);
    let history = history.output.expect("sanitized operational history");
    assert_eq!(history["returnedEventCount"], 1);
    assert_eq!(
        history["events"][0]["errorCode"],
        "configuration_validation_failed"
    );
    let serialized = history.to_string();
    assert!(!serialized.contains("secret-target"));
    assert!(!serialized.contains("secret prompt"));
    assert!(!serialized.contains("targetId"));

    let observation = run
        .handle(&request(
            "tool-operational-observation",
            OPERATIONAL_HEALTH_OBSERVATION_TOOL,
            json!({ "target": "deployment", "sampleCount": 3 }),
        ))
        .expect("bounded operational observation");
    assert_eq!(observation.status, ToolCallResultStatus::Success);
    let observation = observation.output.expect("sanitized observation");
    assert_eq!(observation["sampleCount"], 3);
    assert_eq!(observation["samples"].as_array().map(Vec::len), Some(3));
    assert_eq!(observation["stable"], true);
    assert!(
        observation["windowMs"]
            .as_u64()
            .is_some_and(|value| value >= 100)
    );
    let serialized = observation.to_string();
    assert!(!serialized.contains("backendId"));
    assert!(!serialized.contains("binaryPath"));
    assert!(!serialized.contains("configPath"));
    assert!(!serialized.contains(temp.0.to_string_lossy().as_ref()));

    let install = run
        .handle(&request(
            "tool-external-install",
            PLAN_EXTERNAL_AGENT_INSTALLATION_TOOL,
            json!({ "integrationId": "pi-coding-agent" }),
        ))
        .expect("private installation plan");
    assert_eq!(install.status, ToolCallResultStatus::Success);
    let plan = install.output.expect("sanitized installation plan");
    assert_eq!(plan["actionKind"], "installExternalAgent");
    assert_eq!(plan["targetId"], "pi-coding-agent");
    assert!(!plan.to_string().contains(temp.0.to_string_lossy().as_ref()));
    assert_eq!(run.action_plans().len(), 1);
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
        credentials.clone(),
        OpenCodePaths::for_macos(&temp.0.join("home"), &temp.0),
        "http://127.0.0.1:10100/v1".to_owned(),
    ));
    let profiles = ExternalModelProfileRegistry::conservative_managed_route();
    let pi_coding_agent = Arc::new(PiCodingAgentIntegrationAdapter::with_gateway_base_url(
        database.clone(),
        credentials.clone(),
        profiles.clone(),
        PiCodingAgentPaths::for_macos(&temp.0.join("home"), &temp.0),
        "http://127.0.0.1:10100/v1".to_owned(),
    ));
    let openclaw = Arc::new(OpenClawIntegrationAdapter::with_gateway_base_url(
        database.clone(),
        credentials.clone(),
        profiles.clone(),
        OpenClawPaths::for_macos(&temp.0.join("home"), &temp.0),
        "http://127.0.0.1:10100/v1".to_owned(),
    ));
    let hermes_agent = Arc::new(HermesAgentIntegrationAdapter::with_gateway_base_url(
        database.clone(),
        credentials,
        profiles,
        HermesAgentPaths::for_macos(&temp.0.join("home"), &temp.0),
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
        pi_coding_agent.clone(),
        openclaw.clone(),
        hermes_agent.clone(),
        gateway.clone(),
    ));
    let action_plans = AgentActionPlanStore::new();
    let runtime_profiles = Arc::new(RuntimeProfileManager::new(database.clone(), engine.clone()));
    let executor = AgentToolExecutor::new(
        model_storage_path,
        database.clone(),
        engine,
        open_code,
        pi_coding_agent,
        openclaw,
        hermes_agent,
        removals,
        diagnostics,
        catalog,
        downloads.clone(),
        Arc::new(ManagedExternalAgentDeploymentManager::new(
            database.clone(),
            temp.0.join("managed-external-agents"),
            Vec::new(),
        )),
        gateway,
        action_plans.clone(),
        runtime_profiles,
    );
    let cancellation = Arc::new(AtomicBool::new(false));
    let mut run = executor.start_run(
        "agent-model-download-run".to_owned(),
        None,
        None,
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
