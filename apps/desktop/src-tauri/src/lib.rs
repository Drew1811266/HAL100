mod agent_action;
mod agent_coordinator;
mod agent_ecosystem;
mod agent_kernel;
mod agent_provider;
mod agent_service;
mod agent_tools;

use std::{
    net::{SocketAddr, TcpListener},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use hal100_core::AppCore;
use hal100_infra::{
    BackendConfig, BackendManager, CredentialRegistry, DEFAULT_GATEWAY_ADDRESS, Database,
    ExternalModelProfileRegistry, GatewayRoutingSnapshot, GatewayState, GenericClientManager,
    GgufImportManager, HermesAgentIntegrationAdapter, HermesAgentPaths, LlamaCppManager,
    LocalBackendDiscoveryService, LoggingGuard, ModelDownloadManager, ModelRemovalManager,
    OpenClawIntegrationAdapter, OpenClawPaths, OpenCodeManager, OpenCodePaths,
    PiCodingAgentIntegrationAdapter, PiCodingAgentPaths, RemoteModelCatalog, UsageWriter,
    init_structured_logging, serve_gateway, stored_client_credential,
};
use hal100_platform::{MacOsKeychainSecretStore, MacOsSystemProbe};
use hal100_protocol::{
    AgentEcosystemCatalog, AppOverview, AuditLog, BackendCatalog, BackendDraft, BackendProbeResult,
    BackendRouteDraft, DataCleanupPreview, DataCleanupResult, DesktopSettings, DownloadSource,
    EngineInstallPlan, EngineRemovePlan, ExternalAgentConfigurationPlan,
    ExternalAgentConfigurationResult, ExternalAgentDetection, ExternalAgentDisconnectPlan,
    ExternalAgentDisconnectResult, ExternalAgentGatewayProtocol, GenericClientCatalog,
    GenericClientCredential, GgufImportPlan, GgufImportResult, HardwareProfile, LlamaCppStatus,
    LocalBackendDiscovery, ModelDownloadPlan, ModelDownloadSnapshot, ModelLibrary,
    ModelRemovalKind, ModelRemovalPlan, ModelRemovalResult, ModelTestResult, OnboardingCompletion,
    OpenCodeApplyResult, OpenCodeConfigPlan, OpenCodeDetection, OpenCodeProjectDiagnosis,
    RemoteModelRepository, RemoteModelSearchResults, RetentionSettingsDraft, ServiceState,
    UsageDashboard,
};
use tauri::{
    Manager, State,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_autostart::ManagerExt as AutostartManagerExt;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use crate::agent_service::AgentService;

const TRAY_OPEN_MENU_ID: &str = "open";
const TRAY_QUIT_MENU_ID: &str = "quit";
const DEV_HIDE_WINDOW_ARGUMENT: &str = "--hal100-dev-hide-window";

struct CoreState {
    core: Arc<AppCore<MacOsSystemProbe>>,
}

struct DatabaseState {
    database: Arc<Database>,
    gateway_base_url: String,
}

struct LoggingState {
    #[allow(dead_code)]
    guard: LoggingGuard,
}

struct UsageWriterState {
    #[allow(dead_code)]
    writer: UsageWriter,
}

struct GatewayRuntimeState {
    task: tauri::async_runtime::JoinHandle<()>,
}

struct GatewayControlState {
    gateway: GatewayState,
}

struct BackendManagementState {
    manager: Arc<BackendManager>,
}

struct BackendDiscoveryState {
    service: Arc<LocalBackendDiscoveryService>,
}

struct GenericClientState {
    manager: Arc<GenericClientManager>,
}

struct OpenCodeState {
    manager: Arc<OpenCodeManager>,
}

struct PiCodingAgentState {
    manager: Arc<PiCodingAgentIntegrationAdapter>,
}

struct OpenClawState {
    manager: Arc<OpenClawIntegrationAdapter>,
}

struct HermesAgentState {
    manager: Arc<HermesAgentIntegrationAdapter>,
}

struct ModelManagementState {
    database: Arc<Database>,
    import_manager: Arc<GgufImportManager>,
    model_storage_path: PathBuf,
    download_manager: Arc<ModelDownloadManager>,
    removal_manager: Arc<ModelRemovalManager>,
}

struct RemoteCatalogState {
    catalog: Arc<RemoteModelCatalog>,
}

struct EngineState {
    manager: Arc<LlamaCppManager>,
}

struct AgentState {
    service: Arc<AgentService>,
}

struct ModelTestState {
    client: reqwest::Client,
    gateway_address: SocketAddr,
    client_key: String,
}

impl Drop for GatewayRuntimeState {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn configured_gateway_address() -> Result<SocketAddr, std::io::Error> {
    let Some(raw_port) = std::env::var("HAL100_DEV_GATEWAY_PORT").ok() else {
        return Ok(DEFAULT_GATEWAY_ADDRESS);
    };
    let port = raw_port.parse::<u16>().map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "HAL100_DEV_GATEWAY_PORT must be a non-zero u16",
        )
    })?;
    if port == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "HAL100_DEV_GATEWAY_PORT must be a non-zero u16",
        ));
    }
    Ok(SocketAddr::new(DEFAULT_GATEWAY_ADDRESS.ip(), port))
}

fn bind_gateway_listener(address: SocketAddr) -> Result<TcpListener, std::io::Error> {
    TcpListener::bind(address)
}

fn unix_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}

async fn require_native_confirmation(
    app: tauri::AppHandle,
    title: impl Into<String>,
    message: impl Into<String>,
    dangerous: bool,
) -> Result<(), String> {
    let title = title.into();
    let message = message.into();
    let confirmed = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .message(message)
            .title(title)
            .kind(if dangerous {
                MessageDialogKind::Warning
            } else {
                MessageDialogKind::Info
            })
            .buttons(MessageDialogButtons::OkCancelCustom(
                "确认执行".to_owned(),
                "取消".to_owned(),
            ))
            .blocking_show()
    })
    .await
    .map_err(|error| format!("原生确认窗口异常结束：{error}"))?;
    confirmed
        .then_some(())
        .ok_or_else(|| "用户已取消操作".to_owned())
}

fn show_or_create_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }

    let Some(config) = app
        .config()
        .app
        .windows
        .iter()
        .find(|config| config.label == "main")
    else {
        return;
    };

    if let Ok(window) = tauri::WebviewWindowBuilder::from_config(app, config)
        .and_then(tauri::WebviewWindowBuilder::build)
    {
        let _ = window.set_focus();
    }
}

fn dev_hide_window_requested(arguments: &[String]) -> bool {
    cfg!(debug_assertions)
        && arguments
            .iter()
            .any(|argument| argument == DEV_HIDE_WINDOW_ARGUMENT)
}

fn handle_single_instance(app: &tauri::AppHandle, arguments: &[String]) {
    if dev_hide_window_requested(arguments) {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.hide();
            tracing::info!("development_window_hidden_for_stability");
        }
        return;
    }
    show_or_create_main_window(app);
}

fn install_tray(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItemBuilder::with_id(TRAY_OPEN_MENU_ID, "打开 HAL100").build(app)?;
    let quit = MenuItemBuilder::with_id(TRAY_QUIT_MENU_ID, "退出 HAL100").build(app)?;
    let menu = MenuBuilder::new(app)
        .item(&open)
        .separator()
        .item(&quit)
        .build()?;
    let icon = app
        .default_window_icon()
        .expect("HAL100 must include a default window icon")
        .clone();

    TrayIconBuilder::with_id("hal100")
        .icon(icon)
        .tooltip("HAL100")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            TRAY_OPEN_MENU_ID => show_or_create_main_window(app),
            TRAY_QUIT_MENU_ID => {
                tracing::info!(event = "explicit_quit", "desktop_runtime_stopping");
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_or_create_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

fn should_prevent_exit(code: Option<i32>) -> bool {
    code.is_none()
}

fn should_hide_window_on_close(label: &str) -> bool {
    label == "main"
}

#[tauri::command]
fn get_app_overview(state: State<'_, CoreState>) -> AppOverview {
    state.core.overview(env!("CARGO_PKG_VERSION"))
}

#[tauri::command]
fn get_agent_status(state: State<'_, AgentState>) -> Result<hal100_protocol::AgentStatus, String> {
    state.service.status().map_err(|error| error.to_string())
}

#[tauri::command]
fn get_environment_diagnostics(
    state: State<'_, AgentState>,
) -> Result<hal100_protocol::EnvironmentDiagnosticReport, String> {
    state
        .service
        .environment_diagnostics()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn preview_agent_cloud_run(
    request: hal100_protocol::AgentPromptRequest,
    state: State<'_, AgentState>,
) -> Result<hal100_protocol::AgentCloudRunPreview, String> {
    state
        .service
        .preview_cloud_run(&request)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_agent_cloud_session(
    state: State<'_, AgentState>,
) -> Result<hal100_protocol::AgentCloudSessionStatus, String> {
    state
        .service
        .cloud_session_status()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn preview_agent_cloud_session(
    target: hal100_protocol::AgentCloudTarget,
    state: State<'_, AgentState>,
) -> Result<hal100_protocol::AgentCloudSessionPreview, String> {
    state
        .service
        .preview_cloud_session(&target)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn start_agent_cloud_session(
    app: tauri::AppHandle,
    target: hal100_protocol::AgentCloudTarget,
    state: State<'_, AgentState>,
) -> Result<hal100_protocol::AgentCloudSessionStatus, String> {
    let preview = state
        .service
        .preview_cloud_session(&target)
        .map_err(|error| error.to_string())?;
    require_native_confirmation(
        app,
        "确认当前 Agent 会话使用云端",
        format!(
            "后端：{}\n目标：{}\n模型：{}\n\n{}\n\n该授权只保存在当前 HAL100 进程内存中；明确退出或重启应用后恢复本地 Qwen 默认。",
            preview.backend_name,
            preview.api_root,
            preview.model,
            preview.confirmation_summary,
        ),
        false,
    )
    .await?;
    state
        .service
        .start_cloud_session(target)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn stop_agent_cloud_session(
    state: State<'_, AgentState>,
) -> Result<hal100_protocol::AgentCloudSessionStatus, String> {
    state
        .service
        .stop_cloud_session()
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn run_agent_prompt(
    app: tauri::AppHandle,
    request: hal100_protocol::AgentPromptRequest,
    state: State<'_, AgentState>,
) -> Result<hal100_protocol::AgentRunResult, String> {
    if request.cloud_target.is_some() {
        let preview = state
            .service
            .preview_cloud_run(&request)
            .map_err(|error| error.to_string())?;
        require_native_confirmation(
            app,
            "确认本次使用云端 Agent",
            format!(
                "后端：{}\n目标：{}\n模型：{}\n任务文字：{} 字节\n\n{}\n\n仅本次任务使用云端；失败时不会改用本地模型。",
                preview.backend_name,
                preview.api_root,
                preview.model,
                preview.prompt_bytes,
                preview.confirmation_summary,
            ),
            false,
        )
        .await?;
    }
    state
        .service
        .run_prompt(request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn stop_agent_runtime(
    state: State<'_, AgentState>,
) -> Result<hal100_protocol::AgentStatus, String> {
    state
        .service
        .stop_runtime()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn cancel_agent_run(state: State<'_, AgentState>) -> Result<hal100_protocol::AgentStatus, String> {
    state
        .service
        .cancel_active_run()
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn apply_agent_action_plan(
    app: tauri::AppHandle,
    plan_id: String,
    state: State<'_, AgentState>,
) -> Result<hal100_protocol::AgentActionResult, String> {
    let plan = state
        .service
        .action_plan(&plan_id)
        .map_err(|error| error.to_string())?;
    let (title, safety_note) = match plan.action_kind {
        hal100_protocol::AgentActionKind::StartOrSwitchModel => (
            "确认 Agent 的模型启动或切换计划",
            "确认后 Rust Core 会重新校验模型文件，并等待已有请求安全排空；不会强制切换。",
        ),
        hal100_protocol::AgentActionKind::DownloadModel => (
            "确认 Agent 的模型下载计划",
            "确认后 Rust Core 会再次检查可用空间，并按固定来源、仓库、修订、文件和 SHA-256 启动下载；完成校验前不会进入模型库。",
        ),
        hal100_protocol::AgentActionKind::InstallLlamaCpp => (
            "确认 Agent 的 llama.cpp 安装计划",
            "确认后 Rust Core 才会下载固定官方构建，并校验归档与二进制 SHA-256。",
        ),
        hal100_protocol::AgentActionKind::RemoveLlamaCpp => (
            "确认 Agent 的 llama.cpp 卸载计划",
            "确认后 Rust Core 会停止托管进程并只删除 HAL100 引擎目录；不会删除任何模型。",
        ),
        hal100_protocol::AgentActionKind::RemoveModel => (
            "确认 Agent 的模型移除计划",
            "确认后 Rust Core 会重新校验活动模型、所有权和路径：托管文件只移到系统废纸篓，外部文件不会改动。",
        ),
        hal100_protocol::AgentActionKind::ConfigureOpenCode => (
            "确认 Agent 的 OpenCode 配置计划",
            "确认后 Rust Core 会再次检查文件快照、Provider 所有权和符号链接，并在需要时创建备份。",
        ),
    };
    let details = plan
        .details
        .iter()
        .map(|detail| format!("• {detail}"))
        .collect::<Vec<_>>()
        .join("\n");
    let current = plan.current_state.as_deref().unwrap_or("当前状态未知");
    if let Err(error) = require_native_confirmation(
        app,
        title,
        format!(
            "目标：{}\n{}\n\n{}\n\n{}。\n\n{}\nAgent 本身不能执行这项操作。",
            plan.target_name, current, details, plan.action_summary, safety_note
        ),
        matches!(
            plan.action_kind,
            hal100_protocol::AgentActionKind::RemoveModel
                | hal100_protocol::AgentActionKind::RemoveLlamaCpp
        ),
    )
    .await
    {
        state
            .service
            .discard_action_plan(&plan_id, "native_confirmation_cancelled");
        return Err(error);
    }
    state
        .service
        .apply_action_plan(&plan_id)
        .await
        .map_err(|error| error.to_string())
}

fn set_autostart_enabled(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    if enabled {
        manager.enable()
    } else {
        manager.disable()
    }
    .map_err(|error| format!("更新随系统登录启动失败：{error}"))
}

#[tauri::command]
async fn get_desktop_settings(
    app: tauri::AppHandle,
    state: State<'_, DatabaseState>,
) -> Result<DesktopSettings, String> {
    let launch_at_login_enabled = app
        .autolaunch()
        .is_enabled()
        .map_err(|error| format!("读取随系统登录启动状态失败：{error}"))?;
    let database = state.database.clone();
    let gateway_base_url = state.gateway_base_url.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (onboarding_completed, onboarding_step, launch_at_login_asked) =
            database.onboarding_state()?;
        let retention = database.retention_settings()?;
        Ok::<_, hal100_infra::DatabaseError>(DesktopSettings {
            onboarding_completed,
            onboarding_step,
            launch_at_login_asked,
            launch_at_login_enabled,
            usage_retention_days: retention.usage_retention_days,
            audit_retention_days: retention.audit_retention_days,
            gateway_base_url,
            close_behavior: "隐藏窗口并保持后台运行".to_owned(),
        })
    })
    .await
    .map_err(|error| format!("读取桌面设置任务异常结束：{error}"))?
    .map_err(|error| error.to_string())
}

#[tauri::command]
async fn set_onboarding_step(step: u8, state: State<'_, DatabaseState>) -> Result<(), String> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.set_onboarding_step(step, unix_time_ms()))
        .await
        .map_err(|error| format!("保存首次启动进度任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn complete_onboarding(
    app: tauri::AppHandle,
    completion: OnboardingCompletion,
    state: State<'_, DatabaseState>,
) -> Result<DesktopSettings, String> {
    let previous = app
        .autolaunch()
        .is_enabled()
        .map_err(|error| format!("读取随系统登录启动状态失败：{error}"))?;
    set_autostart_enabled(&app, completion.launch_at_login)?;
    let database = state.database.clone();
    let saved =
        tauri::async_runtime::spawn_blocking(move || database.complete_onboarding(unix_time_ms()))
            .await
            .map_err(|error| format!("完成首次启动任务异常结束：{error}"))?;
    if let Err(error) = saved {
        let _ = set_autostart_enabled(&app, previous);
        return Err(error.to_string());
    }
    get_desktop_settings(app, state).await
}

#[tauri::command]
async fn set_launch_at_login(
    app: tauri::AppHandle,
    enabled: bool,
    state: State<'_, DatabaseState>,
) -> Result<DesktopSettings, String> {
    let previous = app
        .autolaunch()
        .is_enabled()
        .map_err(|error| format!("读取随系统登录启动状态失败：{error}"))?;
    set_autostart_enabled(&app, enabled)?;
    let database = state.database.clone();
    let saved = tauri::async_runtime::spawn_blocking(move || {
        database.mark_launch_at_login_asked(enabled, unix_time_ms())
    })
    .await
    .map_err(|error| format!("保存登录启动设置任务异常结束：{error}"))?;
    if let Err(error) = saved {
        let _ = set_autostart_enabled(&app, previous);
        return Err(error.to_string());
    }
    get_desktop_settings(app, state).await
}

#[tauri::command]
async fn update_retention_settings(
    draft: RetentionSettingsDraft,
    state: State<'_, DatabaseState>,
) -> Result<RetentionSettingsDraft, String> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || {
        database.set_retention_settings(draft, unix_time_ms())
    })
    .await
    .map_err(|error| format!("保存数据保留设置任务异常结束：{error}"))?
    .map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_data_cleanup_preview(
    state: State<'_, DatabaseState>,
) -> Result<DataCleanupPreview, String> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.data_cleanup_preview(unix_time_ms()))
        .await
        .map_err(|error| format!("读取数据清理预览任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn apply_data_retention(
    app: tauri::AppHandle,
    state: State<'_, DatabaseState>,
) -> Result<DataCleanupResult, String> {
    let cleanup_at_ms = unix_time_ms();
    let database = state.database.clone();
    let preview_database = database.clone();
    let preview = tauri::async_runtime::spawn_blocking(move || {
        preview_database.data_cleanup_preview(cleanup_at_ms)
    })
    .await
    .map_err(|error| format!("读取数据清理预览任务异常结束：{error}"))?
    .map_err(|error| error.to_string())?;
    require_native_confirmation(
        app,
        "按保留策略删除历史数据",
        format!(
            "确认删除 {} 条过期 Token 请求记录和 {} 条过期审计记录？此操作不可撤销，不会删除模型、后端或凭据。",
            preview.usage_request_count, preview.audit_event_count
        ),
        true,
    )
    .await?;
    tauri::async_runtime::spawn_blocking(move || database.apply_data_retention(cleanup_at_ms))
        .await
        .map_err(|error| format!("执行数据清理任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_audit_log(state: State<'_, DatabaseState>) -> Result<AuditLog, String> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.audit_log(200))
        .await
        .map_err(|error| format!("读取审计记录任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_gateway_routing_snapshot(state: State<'_, GatewayControlState>) -> GatewayRoutingSnapshot {
    state.gateway.routing_snapshot()
}

#[tauri::command]
fn get_backend_catalog(state: State<'_, BackendManagementState>) -> Result<BackendCatalog, String> {
    state.manager.catalog().map_err(|error| error.to_string())
}

#[tauri::command]
fn get_generic_client_catalog(
    state: State<'_, GenericClientState>,
) -> Result<GenericClientCatalog, String> {
    state.manager.catalog().map_err(|error| error.to_string())
}

#[tauri::command]
async fn create_generic_client(
    display_name: String,
    state: State<'_, GenericClientState>,
) -> Result<GenericClientCredential, String> {
    state
        .manager
        .create(&display_name)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn revoke_generic_client(
    app: tauri::AppHandle,
    client_app_id: String,
    state: State<'_, GenericClientState>,
) -> Result<GenericClientCatalog, String> {
    let display_name = state
        .manager
        .catalog()
        .map_err(|error| error.to_string())?
        .clients
        .into_iter()
        .find(|client| client.client_app_id == client_app_id)
        .map(|client| client.display_name)
        .ok_or_else(|| "通用客户端凭据不存在".to_owned())?;
    require_native_confirmation(
        app,
        "撤销通用客户端凭据",
        format!(
            "确认撤销“{display_name}”的本地客户端 Key？使用它的软件将立即无法访问 HAL100 Gateway；历史 Token 记录不会删除。"
        ),
        true,
    )
    .await?;
    state
        .manager
        .revoke(&client_app_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn save_external_backend(
    draft: BackendDraft,
    state: State<'_, BackendManagementState>,
) -> Result<BackendCatalog, String> {
    state
        .manager
        .save_backend(draft)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn activate_external_backend(
    backend_id: String,
    state: State<'_, BackendManagementState>,
) -> Result<BackendCatalog, String> {
    state
        .manager
        .activate_backend(&backend_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn force_activate_external_backend(
    app: tauri::AppHandle,
    backend_id: String,
    state: State<'_, BackendManagementState>,
) -> Result<BackendCatalog, String> {
    require_native_confirmation(
        app,
        "强制切换活动后端",
        "确认立即强制切换？HAL100 会取消旧活动后端上的所有未完成请求，并将其 Usage 标记为 forced_route_switch。",
        true,
    )
    .await?;
    state
        .manager
        .force_activate_backend(&backend_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn probe_external_backend(
    backend_id: String,
    state: State<'_, BackendManagementState>,
) -> Result<BackendProbeResult, String> {
    state
        .manager
        .probe_backend(&backend_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn save_model_route(
    draft: BackendRouteDraft,
    state: State<'_, BackendManagementState>,
) -> Result<BackendCatalog, String> {
    state
        .manager
        .save_route(draft)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn delete_model_route(
    app: tauri::AppHandle,
    alias: String,
    state: State<'_, BackendManagementState>,
) -> Result<BackendCatalog, String> {
    require_native_confirmation(
        app,
        "删除模型别名",
        format!("确认删除模型别名“{alias}”？正在使用该别名的新请求将无法再按此路由。"),
        true,
    )
    .await?;
    state
        .manager
        .delete_route(&alias)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn delete_external_backend(
    app: tauri::AppHandle,
    backend_id: String,
    state: State<'_, BackendManagementState>,
) -> Result<BackendCatalog, String> {
    require_native_confirmation(
        app,
        "删除外部后端",
        "确认删除这个外部后端及其 Keychain 凭据？必须先移除引用它的模型别名，活动后端不能删除。",
        true,
    )
    .await?;
    state
        .manager
        .delete_backend(&backend_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn discover_local_backends(
    state: State<'_, BackendDiscoveryState>,
) -> Result<LocalBackendDiscovery, String> {
    Ok(state.service.discover().await)
}

#[tauri::command]
async fn get_hardware_profile(
    state: State<'_, ModelManagementState>,
) -> Result<HardwareProfile, String> {
    let model_storage_path = state.model_storage_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        MacOsSystemProbe.hardware_profile(&model_storage_path)
    })
    .await
    .map_err(|error| format!("硬件检测任务异常结束：{error}"))?
    .map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_model_library(state: State<'_, ModelManagementState>) -> Result<ModelLibrary, String> {
    let import_manager = state.import_manager.clone();
    let model_storage_path = state.model_storage_path.clone();
    tauri::async_runtime::spawn_blocking(move || import_manager.library(&model_storage_path))
        .await
        .map_err(|error| format!("读取模型库任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn select_and_plan_gguf_import(
    app: tauri::AppHandle,
    state: State<'_, ModelManagementState>,
) -> Result<Option<GgufImportPlan>, String> {
    let selected = app
        .dialog()
        .file()
        .add_filter("GGUF 模型", &["gguf"])
        .blocking_pick_file();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected
        .into_path()
        .map_err(|error| format!("无法读取所选文件路径：{error}"))?;
    state
        .import_manager
        .plan_import(&path)
        .map(Some)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn apply_gguf_import(
    app: tauri::AppHandle,
    plan_id: String,
    state: State<'_, ModelManagementState>,
) -> Result<GgufImportResult, String> {
    require_native_confirmation(
        app,
        "确认导入外部模型",
        "HAL100 将索引刚刚预览的外部 GGUF 文件。不会复制、移动或删除源文件。",
        false,
    )
    .await?;
    let manager = state.import_manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.apply_import(&plan_id))
        .await
        .map_err(|error| format!("GGUF导入任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn plan_model_removal(
    model_id: String,
    state: State<'_, ModelManagementState>,
    engine: State<'_, EngineState>,
) -> Result<ModelRemovalPlan, String> {
    let status = engine.manager.status().map_err(|error| error.to_string())?;
    state
        .removal_manager
        .plan_removal(&model_id, status.active_model_id.as_deref())
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn apply_model_removal(
    app: tauri::AppHandle,
    plan_id: String,
    state: State<'_, ModelManagementState>,
    engine: State<'_, EngineState>,
) -> Result<ModelRemovalResult, String> {
    let plan = state
        .removal_manager
        .plan(&plan_id)
        .map_err(|error| error.to_string())?;
    let (title, consequence) = match plan.removal_kind {
        ModelRemovalKind::MoveManagedFileToTrash => (
            "确认删除 HAL100 托管模型",
            "模型文件会移到 macOS 废纸篓，并从 HAL100 模型库移除。可在废纸篓清空前恢复文件。",
        ),
        ModelRemovalKind::RemoveMissingManagedIndex => (
            "确认清理失效模型索引",
            "HAL100 已确认托管文件不存在；本次只移除失效索引。",
        ),
        ModelRemovalKind::RemoveExternalIndex => (
            "确认移除外部模型索引",
            "本次只从 HAL100 移除索引；外部 GGUF 源文件不会被移动、修改或删除。",
        ),
    };
    if let Err(error) = require_native_confirmation(
        app,
        title,
        format!(
            "模型：{}\n大小：{} 字节\n\n{}\n\n{}",
            plan.display_name, plan.size_bytes, plan.action_summary, consequence
        ),
        true,
    )
    .await
    {
        let _ = state.removal_manager.discard_plan(&plan_id);
        return Err(error);
    }
    let manager = state.removal_manager.clone();
    let model_id = plan.model_id;
    engine
        .manager
        .run_if_model_inactive(&model_id, move || manager.apply_removal(&plan_id, None))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn set_default_download_source(
    source: DownloadSource,
    state: State<'_, ModelManagementState>,
) -> Result<ModelLibrary, String> {
    let database = state.database.clone();
    let import_manager = state.import_manager.clone();
    let model_storage_path = state.model_storage_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        database.set_default_download_source(source, unix_time_ms())?;
        import_manager.library(&model_storage_path)
    })
    .await
    .map_err(|error| format!("保存默认下载源任务异常结束：{error}"))?
    .map_err(|error: hal100_infra::GgufImportError| error.to_string())
}

#[tauri::command]
async fn search_remote_models(
    source: DownloadSource,
    query: String,
    state: State<'_, RemoteCatalogState>,
) -> Result<RemoteModelSearchResults, String> {
    state
        .catalog
        .search(source, &query)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_remote_model_repository(
    source: DownloadSource,
    repository: String,
    state: State<'_, RemoteCatalogState>,
) -> Result<RemoteModelRepository, String> {
    state
        .catalog
        .repository(source, &repository)
        .await
        .map_err(|error| error.to_string())
}

async fn available_model_storage_bytes(path: PathBuf) -> Result<u64, String> {
    tauri::async_runtime::spawn_blocking(move || {
        MacOsSystemProbe.model_storage_available_bytes(&path)
    })
    .await
    .map_err(|error| format!("磁盘空间检测任务异常结束：{error}"))?
    .map_err(|error| error.to_string())
}

#[tauri::command]
async fn plan_model_download(
    source: DownloadSource,
    repository: String,
    remote_path: String,
    state: State<'_, ModelManagementState>,
) -> Result<ModelDownloadPlan, String> {
    let manager = state.download_manager.clone();
    let available = available_model_storage_bytes(state.model_storage_path.clone()).await?;
    manager
        .plan_download(source, &repository, &remote_path, available)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn start_model_download(
    app: tauri::AppHandle,
    plan_id: String,
    state: State<'_, ModelManagementState>,
) -> Result<ModelDownloadSnapshot, String> {
    require_native_confirmation(
        app,
        "确认下载模型",
        "HAL100 将按刚刚预览的来源、大小和 SHA-256 下载模型，并写入托管模型目录。",
        false,
    )
    .await?;
    let manager = state.download_manager.clone();
    let available = available_model_storage_bytes(state.model_storage_path.clone()).await?;
    manager
        .start_download(&plan_id, available)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn resume_model_download(
    download_id: String,
    state: State<'_, ModelManagementState>,
) -> Result<ModelDownloadSnapshot, String> {
    let manager = state.download_manager.clone();
    let available = available_model_storage_bytes(state.model_storage_path.clone()).await?;
    manager
        .resume_download(&download_id, available)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_model_downloads(
    state: State<'_, ModelManagementState>,
) -> Result<Vec<ModelDownloadSnapshot>, String> {
    let manager = state.download_manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.downloads())
        .await
        .map_err(|error| format!("读取下载任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn cancel_model_download(
    download_id: String,
    state: State<'_, ModelManagementState>,
) -> Result<(), String> {
    state
        .download_manager
        .cancel_download(&download_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_llama_cpp_status(state: State<'_, EngineState>) -> Result<LlamaCppStatus, String> {
    state.manager.status().map_err(|error| error.to_string())
}

#[tauri::command]
fn plan_llama_cpp_install(state: State<'_, EngineState>) -> Result<EngineInstallPlan, String> {
    state
        .manager
        .plan_install()
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn apply_llama_cpp_install(
    app: tauri::AppHandle,
    plan_id: String,
    state: State<'_, EngineState>,
) -> Result<LlamaCppStatus, String> {
    require_native_confirmation(
        app,
        "确认安装 llama.cpp",
        "HAL100 将下载并安装刚刚预览的固定版本官方引擎。安装包和可执行文件都会校验 SHA-256。",
        false,
    )
    .await?;
    state
        .manager
        .apply_install(&plan_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn plan_llama_cpp_remove(state: State<'_, EngineState>) -> Result<EngineRemovePlan, String> {
    state
        .manager
        .plan_remove()
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn apply_llama_cpp_remove(
    app: tauri::AppHandle,
    plan_id: String,
    state: State<'_, EngineState>,
) -> Result<LlamaCppStatus, String> {
    require_native_confirmation(
        app,
        "确认卸载 llama.cpp",
        "此操作会停止当前推理进程并删除 HAL100 托管的 llama.cpp 引擎文件，但不会删除模型。",
        true,
    )
    .await?;
    state
        .manager
        .apply_remove(&plan_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn start_llama_cpp_model(
    model_id: String,
    state: State<'_, EngineState>,
) -> Result<LlamaCppStatus, String> {
    state
        .manager
        .start_model(&model_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn force_start_llama_cpp_model(
    app: tauri::AppHandle,
    model_id: String,
    state: State<'_, EngineState>,
) -> Result<LlamaCppStatus, String> {
    require_native_confirmation(
        app,
        "强制切换本地模型",
        "确认立即强制切换模型？当前活动后端上的未完成请求会被取消并标记为 forced_route_switch，随后旧 llama-server 才会停止。",
        true,
    )
    .await?;
    state
        .manager
        .force_start_model(&model_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn stop_llama_cpp(state: State<'_, EngineState>) -> Result<LlamaCppStatus, String> {
    state
        .manager
        .stop()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn force_stop_llama_cpp(
    app: tauri::AppHandle,
    state: State<'_, EngineState>,
) -> Result<LlamaCppStatus, String> {
    require_native_confirmation(
        app,
        "强制停止本地模型",
        "确认立即强制停止？当前托管 llama.cpp 上的未完成请求会被取消并标记为 forced_route_switch。",
        true,
    )
    .await?;
    state
        .manager
        .force_stop()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn test_active_model(
    prompt: String,
    state: State<'_, ModelTestState>,
) -> Result<ModelTestResult, String> {
    let prompt = prompt.trim();
    if prompt.is_empty()
        || prompt.chars().count() > 8_000
        || prompt
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\t'))
    {
        return Err("测试内容必须为 1—8000 个有效字符".to_owned());
    }
    let started = Instant::now();
    let mut response = state
        .client
        .post(format!(
            "http://{}/v1/chat/completions",
            state.gateway_address
        ))
        .bearer_auth(&state.client_key)
        .json(&serde_json::json!({
            "model": "hal100-active",
            "messages": [{"role": "user", "content": prompt}],
            "stream": false,
        }))
        .send()
        .await
        .map_err(|_| "无法连接 HAL100 Gateway".to_owned())?;
    let request_id = response
        .headers()
        .get("x-hal100-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    if !response.status().is_success() {
        return Err(format!("模型请求失败：HTTP {}", response.status().as_u16()));
    }
    if response
        .content_length()
        .is_some_and(|length| length > 2 * 1024 * 1024)
    {
        return Err("模型响应超过 2 MiB 安全上限".to_owned());
    }
    const MAX_MODEL_TEST_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
    let mut body = Vec::with_capacity(
        response
            .content_length()
            .and_then(|length| usize::try_from(length).ok())
            .unwrap_or(8 * 1024)
            .min(MAX_MODEL_TEST_RESPONSE_BYTES),
    );
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "读取模型响应失败".to_owned())?
    {
        if body.len().saturating_add(chunk.len()) > MAX_MODEL_TEST_RESPONSE_BYTES {
            return Err("模型响应超过 2 MiB 安全上限".to_owned());
        }
        body.extend_from_slice(&chunk);
    }
    let value: serde_json::Value =
        serde_json::from_slice(&body).map_err(|_| "模型返回了无法识别的数据".to_owned())?;
    let content = value
        .pointer("/choices/0/message/content")
        .and_then(serde_json::Value::as_str)
        .filter(|content| !content.is_empty())
        .ok_or_else(|| "模型响应没有文本内容".to_owned())?
        .to_owned();
    let usage = value.get("usage");
    let usage_value = |key: &str| usage.and_then(|usage| usage.get(key)?.as_u64());
    Ok(ModelTestResult {
        content,
        model: value
            .get("model")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("hal100-active")
            .to_owned(),
        input_tokens: usage_value("prompt_tokens"),
        cached_tokens: usage
            .and_then(|usage| usage.pointer("/prompt_tokens_details/cached_tokens"))
            .and_then(serde_json::Value::as_u64),
        output_tokens: usage_value("completion_tokens"),
        total_tokens: usage_value("total_tokens"),
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        request_id,
    })
}

#[tauri::command]
async fn get_usage_dashboard(state: State<'_, DatabaseState>) -> Result<UsageDashboard, String> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.usage_dashboard(50))
        .await
        .map_err(|error| format!("读取 Token 统计任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_agent_ecosystem_catalog() -> AgentEcosystemCatalog {
    agent_ecosystem::catalog()
}

#[tauri::command]
async fn get_opencode_detection(
    state: State<'_, OpenCodeState>,
) -> Result<OpenCodeDetection, String> {
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.detect())
        .await
        .map_err(|error| format!("OpenCode检测任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn plan_opencode_configuration(
    state: State<'_, OpenCodeState>,
) -> Result<OpenCodeConfigPlan, String> {
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.plan_configuration())
        .await
        .map_err(|error| format!("OpenCode配置预览任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn apply_opencode_configuration(
    app: tauri::AppHandle,
    plan_id: String,
    state: State<'_, OpenCodeState>,
) -> Result<OpenCodeApplyResult, String> {
    require_native_confirmation(
        app,
        "确认配置 OpenCode",
        "HAL100 将应用刚刚预览的 Provider 变更，并创建独立的本地 Gateway 凭据。已有配置会先备份。",
        false,
    )
    .await?;
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.apply_configuration(&plan_id))
        .await
        .map_err(|error| format!("OpenCode配置应用任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn discard_opencode_configuration_plan(
    plan_id: String,
    state: State<'_, OpenCodeState>,
) -> Result<bool, String> {
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.discard_configuration_plan(&plan_id))
        .await
        .map_err(|error| format!("OpenCode配置计划丢弃任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn plan_opencode_disconnection(
    state: State<'_, OpenCodeState>,
) -> Result<ExternalAgentDisconnectPlan, String> {
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.plan_disconnection())
        .await
        .map_err(|error| format!("OpenCode断开预览任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn discard_opencode_disconnection_plan(
    plan_id: String,
    state: State<'_, OpenCodeState>,
) -> Result<bool, String> {
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.discard_disconnection_plan(&plan_id))
        .await
        .map_err(|error| format!("OpenCode断开计划丢弃任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn apply_opencode_disconnection(
    app: tauri::AppHandle,
    plan_id: String,
    state: State<'_, OpenCodeState>,
) -> Result<ExternalAgentDisconnectResult, String> {
    require_native_confirmation(
        app,
        "确认断开 OpenCode",
        "HAL100 将只移除自己管理的 Provider 分片，吊销 OpenCode 专属 Gateway Key，并保留配置备份。",
        true,
    )
    .await?;
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.apply_disconnection(&plan_id))
        .await
        .map_err(|error| format!("OpenCode断开任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn diagnose_opencode_project(
    project_path: String,
    state: State<'_, OpenCodeState>,
) -> Result<OpenCodeProjectDiagnosis, String> {
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager.diagnose_project(std::path::Path::new(&project_path))
    })
    .await
    .map_err(|error| format!("OpenCode项目配置诊断任务异常结束：{error}"))?
    .map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_pi_coding_agent_detection(
    state: State<'_, PiCodingAgentState>,
) -> Result<ExternalAgentDetection, String> {
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.detect())
        .await
        .map_err(|error| format!("Pi Coding Agent检测任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn plan_pi_coding_agent_configuration(
    state: State<'_, PiCodingAgentState>,
) -> Result<ExternalAgentConfigurationPlan, String> {
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.plan_configuration())
        .await
        .map_err(|error| format!("Pi Coding Agent配置预览任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn apply_pi_coding_agent_configuration(
    app: tauri::AppHandle,
    plan_id: String,
    state: State<'_, PiCodingAgentState>,
) -> Result<ExternalAgentConfigurationResult, String> {
    require_native_confirmation(
        app,
        "确认配置 Pi Coding Agent",
        "HAL100 将只写入 models.json 的 providers.hal100 分片，并创建 Pi 专属本地 Gateway 凭据。已有配置会先备份。",
        false,
    )
    .await?;
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.apply_configuration(&plan_id))
        .await
        .map_err(|error| format!("Pi Coding Agent配置应用任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn discard_pi_coding_agent_configuration_plan(
    plan_id: String,
    state: State<'_, PiCodingAgentState>,
) -> Result<bool, String> {
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.discard_configuration_plan(&plan_id))
        .await
        .map_err(|error| format!("Pi Coding Agent配置计划丢弃任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn plan_pi_coding_agent_disconnection(
    state: State<'_, PiCodingAgentState>,
) -> Result<ExternalAgentDisconnectPlan, String> {
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.plan_disconnection())
        .await
        .map_err(|error| format!("Pi Coding Agent断开预览任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn discard_pi_coding_agent_disconnection_plan(
    plan_id: String,
    state: State<'_, PiCodingAgentState>,
) -> Result<bool, String> {
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.discard_disconnection_plan(&plan_id))
        .await
        .map_err(|error| format!("Pi Coding Agent断开计划丢弃任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn apply_pi_coding_agent_disconnection(
    app: tauri::AppHandle,
    plan_id: String,
    state: State<'_, PiCodingAgentState>,
) -> Result<ExternalAgentDisconnectResult, String> {
    require_native_confirmation(
        app,
        "确认断开 Pi Coding Agent",
        "HAL100 将只移除自己管理的 providers.hal100 分片，吊销 Pi 专属 Gateway Key，并保留配置备份。",
        true,
    )
    .await?;
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.apply_disconnection(&plan_id))
        .await
        .map_err(|error| format!("Pi Coding Agent断开任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_openclaw_detection(
    state: State<'_, OpenClawState>,
) -> Result<ExternalAgentDetection, String> {
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.detect())
        .await
        .map_err(|error| format!("OpenClaw检测任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn plan_openclaw_configuration(
    protocol: ExternalAgentGatewayProtocol,
    state: State<'_, OpenClawState>,
) -> Result<ExternalAgentConfigurationPlan, String> {
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.plan_configuration(protocol))
        .await
        .map_err(|error| format!("OpenClaw配置预览任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn apply_openclaw_configuration(
    app: tauri::AppHandle,
    plan_id: String,
    state: State<'_, OpenClawState>,
) -> Result<ExternalAgentConfigurationResult, String> {
    require_native_confirmation(
        app,
        "确认配置 OpenClaw",
        "HAL100 将通过 OpenClaw 官方配置工具写入两个专属分片，并创建 OpenClaw 专属本地 Gateway 凭据。已有配置会先备份；默认模型不会变化。",
        false,
    )
    .await?;
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.apply_configuration(&plan_id))
        .await
        .map_err(|error| format!("OpenClaw配置应用任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn discard_openclaw_configuration_plan(
    plan_id: String,
    state: State<'_, OpenClawState>,
) -> Result<bool, String> {
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.discard_configuration_plan(&plan_id))
        .await
        .map_err(|error| format!("OpenClaw配置计划丢弃任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn plan_openclaw_disconnection(
    state: State<'_, OpenClawState>,
) -> Result<ExternalAgentDisconnectPlan, String> {
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.plan_disconnection())
        .await
        .map_err(|error| format!("OpenClaw断开预览任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn discard_openclaw_disconnection_plan(
    plan_id: String,
    state: State<'_, OpenClawState>,
) -> Result<bool, String> {
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.discard_disconnection_plan(&plan_id))
        .await
        .map_err(|error| format!("OpenClaw断开计划丢弃任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn apply_openclaw_disconnection(
    app: tauri::AppHandle,
    plan_id: String,
    state: State<'_, OpenClawState>,
) -> Result<ExternalAgentDisconnectResult, String> {
    require_native_confirmation(
        app,
        "确认断开 OpenClaw",
        "HAL100 将通过 OpenClaw 官方配置工具只移除自己管理的模型与 SecretRef 分片，吊销专属 Gateway Key，并保留配置备份。",
        true,
    )
    .await?;
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.apply_disconnection(&plan_id))
        .await
        .map_err(|error| format!("OpenClaw断开任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_hermes_agent_detection(
    state: State<'_, HermesAgentState>,
) -> Result<ExternalAgentDetection, String> {
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.detect())
        .await
        .map_err(|error| format!("Hermes Agent检测任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn plan_hermes_agent_configuration(
    state: State<'_, HermesAgentState>,
) -> Result<ExternalAgentConfigurationPlan, String> {
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.plan_configuration())
        .await
        .map_err(|error| format!("Hermes Agent配置预览任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn apply_hermes_agent_configuration(
    app: tauri::AppHandle,
    plan_id: String,
    state: State<'_, HermesAgentState>,
) -> Result<ExternalAgentConfigurationResult, String> {
    require_native_confirmation(
        app,
        "确认配置 Hermes Agent",
        "HAL100 将只写入 default Profile 的 providers.hal100 与 .env 专属变量。YAML 会先备份，其他 Profile、默认模型和运行中的服务不会变化。",
        false,
    )
    .await?;
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.apply_configuration(&plan_id))
        .await
        .map_err(|error| format!("Hermes Agent配置应用任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn discard_hermes_agent_configuration_plan(
    plan_id: String,
    state: State<'_, HermesAgentState>,
) -> Result<bool, String> {
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.discard_configuration_plan(&plan_id))
        .await
        .map_err(|error| format!("Hermes Agent配置计划丢弃任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn plan_hermes_agent_disconnection(
    state: State<'_, HermesAgentState>,
) -> Result<ExternalAgentDisconnectPlan, String> {
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.plan_disconnection())
        .await
        .map_err(|error| format!("Hermes Agent断开预览任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn discard_hermes_agent_disconnection_plan(
    plan_id: String,
    state: State<'_, HermesAgentState>,
) -> Result<bool, String> {
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.discard_disconnection_plan(&plan_id))
        .await
        .map_err(|error| format!("Hermes Agent断开计划丢弃任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn apply_hermes_agent_disconnection(
    app: tauri::AppHandle,
    plan_id: String,
    state: State<'_, HermesAgentState>,
) -> Result<ExternalAgentDisconnectResult, String> {
    require_native_confirmation(
        app,
        "确认断开 Hermes Agent",
        "HAL100 将只移除 default Profile 中自己管理的 Provider 与 .env 变量，吊销 Hermes 专属 Gateway Key，并保留不含密钥的 YAML 备份。",
        true,
    )
    .await?;
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.apply_disconnection(&plan_id))
        .await
        .map_err(|error| format!("Hermes Agent断开任务异常结束：{error}"))?
        .map_err(|error| error.to_string())
}

#[cfg(feature = "benchmark-hooks")]
fn schedule_benchmark_window_close(app: &tauri::AppHandle) {
    let Ok(raw_delay) = std::env::var("HAL100_BENCHMARK_CLOSE_AFTER_MS") else {
        return;
    };
    let Ok(delay_ms) = raw_delay.parse::<u64>() else {
        return;
    };

    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.close();
        }
    });
}

#[cfg(feature = "benchmark-hooks")]
fn schedule_benchmark_exit(app: &tauri::AppHandle) {
    let Ok(raw_delay) = std::env::var("HAL100_BENCHMARK_EXIT_AFTER_MS") else {
        return;
    };
    let Ok(delay_ms) = raw_delay.parse::<u64>() else {
        return;
    };

    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        app.exit(0);
    });
}

#[cfg(feature = "benchmark-hooks")]
struct BenchmarkWindowCycleConfig {
    cycles: u32,
    interval: std::time::Duration,
    settle_after_cycles: std::time::Duration,
    exit_when_complete: bool,
    destroy_between_cycles: bool,
}

#[cfg(feature = "benchmark-hooks")]
fn benchmark_window_cycle_config() -> Option<BenchmarkWindowCycleConfig> {
    let cycles = std::env::var("HAL100_BENCHMARK_WINDOW_CYCLES")
        .ok()?
        .parse::<u32>()
        .ok()
        .filter(|cycles| *cycles > 0)?;
    let interval_ms = std::env::var("HAL100_BENCHMARK_WINDOW_INTERVAL_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(120);
    let settle_after_cycles_ms = std::env::var("HAL100_BENCHMARK_SETTLE_AFTER_CYCLES_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(3_000);

    Some(BenchmarkWindowCycleConfig {
        cycles,
        interval: std::time::Duration::from_millis(interval_ms),
        settle_after_cycles: std::time::Duration::from_millis(settle_after_cycles_ms),
        exit_when_complete: std::env::var("HAL100_BENCHMARK_EXIT_WHEN_CYCLES_COMPLETE")
            .is_ok_and(|value| value == "1"),
        destroy_between_cycles: std::env::var("HAL100_BENCHMARK_WINDOW_CYCLE_MODE")
            .is_ok_and(|value| value == "destroy"),
    })
}

#[cfg(feature = "benchmark-hooks")]
fn run_on_main_thread_and_wait(
    app: &tauri::AppHandle,
    task: impl FnOnce() + Send + 'static,
) -> bool {
    let (completed_tx, completed_rx) = std::sync::mpsc::sync_channel(1);
    if app
        .run_on_main_thread(move || {
            task();
            let _ = completed_tx.send(());
        })
        .is_err()
    {
        return false;
    }

    completed_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .is_ok()
}

#[cfg(feature = "benchmark-hooks")]
fn schedule_benchmark_window_cycles(app: &tauri::AppHandle) {
    let Some(config) = benchmark_window_cycle_config() else {
        return;
    };

    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(config.interval);
        let mut completed_cycles = 0;

        for _ in 0..config.cycles {
            let close_app = app.clone();
            let destroy_between_cycles = config.destroy_between_cycles;
            if !run_on_main_thread_and_wait(&app, move || {
                if let Some(window) = close_app.get_webview_window("main") {
                    if destroy_between_cycles {
                        let _ = window.destroy();
                    } else {
                        let _ = window.close();
                    }
                }
            }) {
                tracing::warn!(completed_cycles, "benchmark_window_cycle_interrupted");
                break;
            }

            std::thread::sleep(config.interval);
            let show_app = app.clone();
            if !run_on_main_thread_and_wait(&app, move || {
                show_or_create_main_window(&show_app);
            }) {
                tracing::warn!(completed_cycles, "benchmark_window_cycle_interrupted");
                break;
            }

            completed_cycles += 1;
            std::thread::sleep(config.interval);
        }

        let close_app = app.clone();
        let _ = run_on_main_thread_and_wait(&app, move || {
            if let Some(window) = close_app.get_webview_window("main") {
                let _ = window.close();
            }
        });
        tracing::info!(
            requested_cycles = config.cycles,
            completed_cycles,
            mode = if config.destroy_between_cycles {
                "destroy"
            } else {
                "production"
            },
            "benchmark_window_cycles_completed"
        );

        std::thread::sleep(config.settle_after_cycles);
        if config.exit_when_complete {
            app.exit(0);
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let core = Arc::new(AppCore::new(MacOsSystemProbe));

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            handle_single_instance(app, &args);
        }))
        .manage(CoreState { core: core.clone() })
        .invoke_handler(tauri::generate_handler![
            get_app_overview,
            get_agent_status,
            get_environment_diagnostics,
            preview_agent_cloud_run,
            get_agent_cloud_session,
            preview_agent_cloud_session,
            start_agent_cloud_session,
            stop_agent_cloud_session,
            run_agent_prompt,
            cancel_agent_run,
            apply_agent_action_plan,
            stop_agent_runtime,
            get_desktop_settings,
            set_onboarding_step,
            complete_onboarding,
            set_launch_at_login,
            update_retention_settings,
            get_data_cleanup_preview,
            apply_data_retention,
            get_audit_log,
            get_gateway_routing_snapshot,
            get_backend_catalog,
            get_generic_client_catalog,
            create_generic_client,
            revoke_generic_client,
            save_external_backend,
            activate_external_backend,
            force_activate_external_backend,
            probe_external_backend,
            save_model_route,
            delete_model_route,
            delete_external_backend,
            discover_local_backends,
            get_hardware_profile,
            get_model_library,
            set_default_download_source,
            search_remote_models,
            get_remote_model_repository,
            plan_model_download,
            start_model_download,
            resume_model_download,
            get_model_downloads,
            cancel_model_download,
            plan_model_removal,
            apply_model_removal,
            get_llama_cpp_status,
            plan_llama_cpp_install,
            apply_llama_cpp_install,
            plan_llama_cpp_remove,
            apply_llama_cpp_remove,
            start_llama_cpp_model,
            force_start_llama_cpp_model,
            stop_llama_cpp,
            force_stop_llama_cpp,
            test_active_model,
            get_usage_dashboard,
            get_agent_ecosystem_catalog,
            select_and_plan_gguf_import,
            apply_gguf_import,
            get_opencode_detection,
            plan_opencode_configuration,
            apply_opencode_configuration,
            discard_opencode_configuration_plan,
            plan_opencode_disconnection,
            discard_opencode_disconnection_plan,
            apply_opencode_disconnection,
            diagnose_opencode_project,
            get_pi_coding_agent_detection,
            plan_pi_coding_agent_configuration,
            apply_pi_coding_agent_configuration,
            discard_pi_coding_agent_configuration_plan,
            plan_pi_coding_agent_disconnection,
            discard_pi_coding_agent_disconnection_plan,
            apply_pi_coding_agent_disconnection,
            get_openclaw_detection,
            plan_openclaw_configuration,
            apply_openclaw_configuration,
            discard_openclaw_configuration_plan,
            plan_openclaw_disconnection,
            discard_openclaw_disconnection_plan,
            apply_openclaw_disconnection,
            get_hermes_agent_detection,
            plan_hermes_agent_configuration,
            apply_hermes_agent_configuration,
            discard_hermes_agent_configuration_plan,
            plan_hermes_agent_disconnection,
            discard_hermes_agent_disconnection_plan,
            apply_hermes_agent_disconnection
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && should_hide_window_on_close(window.label())
            {
                api.prevent_close();
                let _ = window.hide();
                tracing::debug!(window = window.label(), "window_hidden_for_background");
            }
        })
        .setup(move |app| {
            let log_dir = app.path().app_log_dir()?;
            let logging_guard = init_structured_logging(log_dir)?;
            app.manage(LoggingState {
                guard: logging_guard,
            });
            tracing::info!(
                version = env!("CARGO_PKG_VERSION"),
                platform = "macos",
                architecture = "aarch64",
                "desktop_runtime_started"
            );

            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&data_dir, std::fs::Permissions::from_mode(0o700))?;
            }
            let model_storage_path = data_dir.join("models");
            std::fs::create_dir_all(&model_storage_path)?;
            let database = Arc::new(Database::open(data_dir.join("hal100.sqlite"))?);
            let schema_version = database.schema_version()?;
            tracing::info!(schema_version, "database_ready");

            if let Ok(client_key) = std::env::var("HAL100_DEV_CLIENT_KEY") {
                let credential = stored_client_credential(
                    "development-key",
                    "development-client",
                    "HAL100 development client",
                    &client_key,
                )?;
                database.upsert_client_credential(&credential, unix_time_ms())?;
            }
            let model_test_key = format!("hal100_model_test_{}", uuid::Uuid::new_v4().simple());
            let model_test_credential = stored_client_credential(
                "hal100-model-test-key",
                "hal100-model-test",
                "HAL100 模型测试",
                &model_test_key,
            )?;
            database.upsert_client_credential(&model_test_credential, unix_time_ms())?;
            let credentials = CredentialRegistry::new(database.load_client_credentials()?);
            let gateway_address = configured_gateway_address()?;
            let gateway_base_url = format!("http://{gateway_address}/v1");
            let generic_client_manager = Arc::new(GenericClientManager::with_gateway_base_url(
                database.clone(),
                credentials.clone(),
                gateway_base_url.clone(),
            ));
            let home_directory = app.path().home_dir()?;
            let open_code_manager = Arc::new(OpenCodeManager::with_gateway_base_url(
                database.clone(),
                credentials.clone(),
                OpenCodePaths::for_macos(&home_directory, &data_dir),
                gateway_base_url.clone(),
            ));
            let external_model_profiles =
                ExternalModelProfileRegistry::conservative_managed_route();
            let pi_coding_agent_manager =
                Arc::new(PiCodingAgentIntegrationAdapter::with_gateway_base_url(
                    database.clone(),
                    credentials.clone(),
                    external_model_profiles.clone(),
                    PiCodingAgentPaths::for_macos(&home_directory, &data_dir),
                    gateway_base_url.clone(),
                ));
            let openclaw_manager = Arc::new(OpenClawIntegrationAdapter::with_gateway_base_url(
                database.clone(),
                credentials.clone(),
                external_model_profiles.clone(),
                OpenClawPaths::for_macos(&home_directory, &data_dir),
                gateway_base_url.clone(),
            ));
            let hermes_agent_manager =
                Arc::new(HermesAgentIntegrationAdapter::with_gateway_base_url(
                    database.clone(),
                    credentials.clone(),
                    external_model_profiles,
                    HermesAgentPaths::for_macos(&home_directory, &data_dir),
                    gateway_base_url.clone(),
                ));
            let backend = std::env::var("HAL100_DEV_BACKEND_URL")
                .ok()
                .map(|url| {
                    let api_key = std::env::var("HAL100_DEV_BACKEND_API_KEY")
                        .ok()
                        .filter(|key| !key.is_empty());
                    BackendConfig::new("development-backend", &url, api_key)
                })
                .transpose()?;
            let usage_writer = UsageWriter::start(database.clone());
            let gateway = GatewayState::new(backend, credentials.clone(), usage_writer.clone())?;
            let engine_gateway = gateway.clone();
            let agent_gateway = gateway.clone();
            let gateway_control = gateway.clone();
            let backend_manager = Arc::new(BackendManager::new(
                database.clone(),
                gateway_control.clone(),
                Arc::new(MacOsKeychainSecretStore::default()),
            ));
            let restore_report = backend_manager.restore()?;
            tracing::info!(
                loaded_backends = restore_report.loaded_backends,
                loaded_routes = restore_report.loaded_routes,
                skipped_backends = restore_report.skipped_backend_ids.len(),
                skipped_routes = restore_report.skipped_route_aliases.len(),
                active_backend_restored = restore_report.active_backend_restored,
                "gateway_routes_restored"
            );
            let backend_configured = gateway.has_backend();
            let credentials_configured = gateway.has_client_credentials();
            let listener = bind_gateway_listener(gateway_address)?;
            let model_test_client = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(120))
                .no_proxy()
                .build()?;
            let gateway_core = core.clone();
            let gateway_task = tauri::async_runtime::spawn(async move {
                if let Err(error) = serve_gateway(listener, gateway).await {
                    gateway_core.set_gateway_state(ServiceState::Error);
                    tracing::error!(
                        error_code = "gateway_server_stopped",
                        error = %error,
                        "gateway_server_stopped"
                    );
                }
            });
            core.set_gateway_state(ServiceState::Running);
            tracing::info!(
                address = %gateway_address,
                backend_configured,
                credentials_configured,
                "gateway_ready"
            );
            app.manage(DatabaseState {
                database: database.clone(),
                gateway_base_url: gateway_base_url.clone(),
            });
            let import_manager = Arc::new(GgufImportManager::new(database.clone()));
            let remote_catalog = Arc::new(RemoteModelCatalog::new()?);
            let backend_discovery = Arc::new(LocalBackendDiscoveryService::new()?);
            let download_manager = Arc::new(ModelDownloadManager::new(
                database.clone(),
                remote_catalog.clone(),
                model_storage_path.clone(),
            )?);
            let removal_manager = Arc::new(ModelRemovalManager::new(
                database.clone(),
                model_storage_path.clone(),
            ));
            let llama_cpp_manager = Arc::new(LlamaCppManager::new(
                database.clone(),
                engine_gateway,
                data_dir.join("engines").join("llama.cpp"),
            )?);
            let agent_runtime = Arc::new(hal100_infra::AgentModelRuntime::new(
                database.clone(),
                llama_cpp_manager.clone(),
                agent_gateway,
            )?);
            let agent_service = Arc::new(AgentService::new(
                agent_runtime,
                llama_cpp_manager.clone(),
                open_code_manager.clone(),
                removal_manager.clone(),
                remote_catalog.clone(),
                download_manager.clone(),
                gateway_control.clone(),
                database.clone(),
                credentials.clone(),
                gateway_base_url,
                model_storage_path.clone(),
                &data_dir,
            )?);
            app.manage(ModelManagementState {
                database: database.clone(),
                import_manager,
                model_storage_path,
                download_manager,
                removal_manager,
            });
            app.manage(RemoteCatalogState {
                catalog: remote_catalog,
            });
            app.manage(BackendDiscoveryState {
                service: backend_discovery,
            });
            app.manage(EngineState {
                manager: llama_cpp_manager,
            });
            app.manage(AgentState {
                service: agent_service,
            });
            app.manage(ModelTestState {
                client: model_test_client,
                gateway_address,
                client_key: model_test_key,
            });
            app.manage(OpenCodeState {
                manager: open_code_manager,
            });
            app.manage(PiCodingAgentState {
                manager: pi_coding_agent_manager,
            });
            app.manage(OpenClawState {
                manager: openclaw_manager,
            });
            app.manage(HermesAgentState {
                manager: hermes_agent_manager,
            });
            app.manage(UsageWriterState {
                writer: usage_writer,
            });
            app.manage(GatewayControlState {
                gateway: gateway_control,
            });
            app.manage(BackendManagementState {
                manager: backend_manager,
            });
            app.manage(GenericClientState {
                manager: generic_client_manager,
            });
            app.manage(GatewayRuntimeState { task: gateway_task });
            install_tray(app)?;
            tracing::info!("tray_ready");
            #[cfg(feature = "benchmark-hooks")]
            {
                schedule_benchmark_window_close(app.handle());
                schedule_benchmark_exit(app.handle());
                schedule_benchmark_window_cycles(app.handle());
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("HAL100 desktop runtime failed")
        .run(|app, event| match event {
            tauri::RunEvent::ExitRequested { code, api, .. } if should_prevent_exit(code) => {
                tracing::debug!("background_runtime_retained");
                api.prevent_exit();
            }
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } => show_or_create_main_window(app),
            _ => {}
        });
}

#[cfg(test)]
mod tests {
    use super::{
        DEV_HIDE_WINDOW_ARGUMENT, bind_gateway_listener, dev_hide_window_requested,
        should_hide_window_on_close, should_prevent_exit,
    };
    use std::{io::ErrorKind, net::TcpStream};

    #[test]
    fn user_window_exit_request_keeps_background_runtime_alive() {
        assert!(should_prevent_exit(None));
    }

    #[test]
    fn explicit_programmatic_exit_is_allowed() {
        assert!(!should_prevent_exit(Some(0)));
    }

    #[test]
    fn only_the_main_window_is_reused_when_closed() {
        assert!(should_hide_window_on_close("main"));
        assert!(!should_hide_window_on_close("diagnostics"));
    }

    #[test]
    #[cfg(debug_assertions)]
    fn development_single_instance_argument_requests_a_hidden_main_window() {
        assert!(dev_hide_window_requested(&[
            "/workspace/target/debug/hal100-desktop".to_owned(),
            DEV_HIDE_WINDOW_ARGUMENT.to_owned(),
        ]));
        assert!(!dev_hide_window_requested(&[
            "/workspace/target/debug/hal100-desktop".to_owned()
        ]));
    }

    #[test]
    fn main_window_capability_keeps_native_titlebar_dragging_enabled() {
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json"))
                .expect("main window capability must remain valid JSON");
        let permissions = capability["permissions"]
            .as_array()
            .expect("main window capability must declare permissions");

        assert!(
            permissions.iter().any(|permission| {
                permission.as_str() == Some("core:window:allow-start-dragging")
            })
        );
        assert!(permissions.iter().any(|permission| {
            permission.as_str() == Some("core:window:allow-internal-toggle-maximize")
        }));
    }

    #[test]
    fn occupied_gateway_port_fails_without_displacing_the_existing_owner() {
        let owner =
            std::net::TcpListener::bind("127.0.0.1:0").expect("bind the existing loopback service");
        let address = owner.local_addr().expect("existing service address");

        let error = bind_gateway_listener(address).expect_err("occupied port must fail closed");
        assert_eq!(error.kind(), ErrorKind::AddrInUse);

        let client = TcpStream::connect(address).expect("existing service remains reachable");
        let (_accepted, peer) = owner
            .accept()
            .expect("existing service still accepts clients");
        assert_eq!(peer.ip(), client.local_addr().expect("client address").ip());
    }
}
