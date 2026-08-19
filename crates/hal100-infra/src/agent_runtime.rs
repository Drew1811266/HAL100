use std::{
    fs::OpenOptions,
    io::Write,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use hal100_protocol::{AgentComponentState, AgentStatus, LocalModelState};
use reqwest::Client;
use thiserror::Error;
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use crate::{
    BackendConfig, Database, DatabaseError, EngineManagerError, GatewayRouteError, GatewayState,
    LlamaCppManager,
};

pub const AGENT_MODEL_ALIAS: &str = "hal100-agent";
const AGENT_BACKEND_ID: &str = "hal100-agent-runtime";
const AGENT_MODEL_REPOSITORY: &str = "unsloth/Qwen3.5-2B-GGUF";
const AGENT_MODEL_REVISION: &str = "f6d5376be1edb4d416d56da11e5397a961aca8ae";
const AGENT_MODEL_FILE: &str = "Qwen3.5-2B-Q4_K_M.gguf";
const AGENT_MODEL_SIZE_BYTES: u64 = 1_280_835_840;
pub const AGENT_MODEL_ID: &str =
    "managed-aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223";
const AGENT_START_TIMEOUT: Duration = Duration::from_secs(90);
const AGENT_CONTEXT_SIZE: &str = "6144";
const PI_VERSION: &str = "0.84.2";
pub const AGENT_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Error)]
pub enum AgentRuntimeError {
    #[error(transparent)]
    Database(#[from] DatabaseError),
    #[error(transparent)]
    Engine(#[from] EngineManagerError),
    #[error(transparent)]
    GatewayRoute(#[from] GatewayRouteError),
    #[error("HAL100 Agent 模型尚未准备好")]
    ModelNotPrepared,
    #[error("无法为 HAL100 Agent 分配安全回环端口")]
    PortUnavailable,
    #[error("HAL100 Agent 模型进程无法启动")]
    StartFailed,
    #[error("HAL100 Agent 模型进程在就绪前退出")]
    ExitedBeforeReady,
    #[error("等待 HAL100 Agent 模型就绪超时")]
    StartTimeout,
    #[error("HAL100 Agent 模型启动已取消")]
    Cancelled,
    #[error("HAL100 Agent 运行时状态锁已损坏")]
    LockPoisoned,
    #[error("HAL100 Agent 运行时文件操作失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("HAL100 Agent 健康检查失败：{0}")]
    Health(String),
}

pub struct AgentModelRuntime {
    database: Arc<Database>,
    engine: Arc<LlamaCppManager>,
    gateway: GatewayState,
    client: Client,
    lifecycle: AsyncMutex<()>,
    runtime: Mutex<RuntimeState>,
}

struct RuntimeState {
    state: AgentComponentState,
    session: Option<RuntimeSession>,
    last_error_code: Option<String>,
}

struct RuntimeSession {
    child: Child,
    port: u16,
}

impl AgentModelRuntime {
    pub fn new(
        database: Arc<Database>,
        engine: Arc<LlamaCppManager>,
        gateway: GatewayState,
    ) -> Result<Self, AgentRuntimeError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .no_proxy()
            .build()
            .map_err(|error| AgentRuntimeError::Health(error.to_string()))?;
        Ok(Self {
            database,
            engine,
            gateway,
            client,
            lifecycle: AsyncMutex::new(()),
            runtime: Mutex::new(RuntimeState {
                state: AgentComponentState::Stopped,
                session: None,
                last_error_code: None,
            }),
        })
    }

    pub fn status(&self) -> Result<AgentStatus, AgentRuntimeError> {
        self.reconcile_exited_process()?;
        let model = self.agent_model()?;
        let runtime = self
            .runtime
            .lock()
            .map_err(|_| AgentRuntimeError::LockPoisoned)?;
        Ok(AgentStatus {
            kernel_state: AgentComponentState::Stopped,
            model_runtime_state: if model.is_some() {
                runtime.state
            } else {
                AgentComponentState::Unavailable
            },
            pi_version: PI_VERSION.to_owned(),
            model_name: "Qwen3.5-2B Q4_K_M".to_owned(),
            model_prepared: model.is_some(),
            model_size_bytes: AGENT_MODEL_SIZE_BYTES,
            idle_timeout_seconds: AGENT_IDLE_TIMEOUT.as_secs() as u32,
            active_run_id: None,
            cancellation_requested: false,
            last_error_code: runtime.last_error_code.clone(),
        })
    }

    pub async fn ensure_started(&self) -> Result<AgentStatus, AgentRuntimeError> {
        self.ensure_started_inner(None).await
    }

    pub async fn ensure_started_cancellable(
        &self,
        cancellation: Arc<AtomicBool>,
    ) -> Result<AgentStatus, AgentRuntimeError> {
        self.ensure_started_inner(Some(cancellation)).await
    }

    async fn ensure_started_inner(
        &self,
        cancellation: Option<Arc<AtomicBool>>,
    ) -> Result<AgentStatus, AgentRuntimeError> {
        let _guard = self.lifecycle.lock().await;
        ensure_not_cancelled(cancellation.as_deref())?;
        self.reconcile_exited_process()?;
        if self
            .runtime
            .lock()
            .map_err(|_| AgentRuntimeError::LockPoisoned)?
            .session
            .is_some()
        {
            return self.status();
        }
        let model = self
            .agent_model()?
            .ok_or(AgentRuntimeError::ModelNotPrepared)?;
        let verified_assets = if let Some(cancellation) = cancellation.clone() {
            self.engine
                .verified_runtime_assets_cancellable(&model.id, cancellation)
                .await
        } else {
            self.engine.verified_runtime_assets(&model.id).await
        };
        let (binary, verified_model) = match verified_assets {
            Ok(assets) => assets,
            Err(EngineManagerError::OperationCancelled) => {
                return Err(AgentRuntimeError::Cancelled);
            }
            Err(error) => return Err(error.into()),
        };
        ensure_not_cancelled(cancellation.as_deref())?;
        if verified_model.repository.as_deref() != Some(AGENT_MODEL_REPOSITORY)
            || verified_model.revision.as_deref() != Some(AGENT_MODEL_REVISION)
            || verified_model.file_name != AGENT_MODEL_FILE
            || verified_model.size_bytes != AGENT_MODEL_SIZE_BYTES
        {
            return Err(AgentRuntimeError::ModelNotPrepared);
        }
        let port = reserve_loopback_port()?;
        let api_key = format!(
            "hal100_agent_backend_{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        let binary_directory = binary.parent().ok_or(AgentRuntimeError::StartFailed)?;
        let api_key_file = EphemeralSecretFile::create(
            binary_directory.join(format!(".agent-session-{}.key", Uuid::new_v4().simple())),
            api_key.as_bytes(),
        )?;
        {
            let mut runtime = self
                .runtime
                .lock()
                .map_err(|_| AgentRuntimeError::LockPoisoned)?;
            runtime.state = AgentComponentState::Starting;
            runtime.last_error_code = None;
        }
        let child = Command::new(&binary)
            .arg("--model")
            .arg(&verified_model.path)
            .arg("--alias")
            .arg(AGENT_MODEL_ALIAS)
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .arg("--ctx-size")
            .arg(AGENT_CONTEXT_SIZE)
            .arg("--parallel")
            .arg("1")
            .arg("--reasoning")
            .arg("off")
            .arg("--api-key-file")
            .arg(api_key_file.path())
            .current_dir(binary_directory)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| AgentRuntimeError::StartFailed)?;
        {
            let mut runtime = self
                .runtime
                .lock()
                .map_err(|_| AgentRuntimeError::LockPoisoned)?;
            runtime.session = Some(RuntimeSession { child, port });
        }
        if let Err(error) = self
            .wait_until_ready(port, &api_key, cancellation.as_deref())
            .await
        {
            self.stop_process(if matches!(error, AgentRuntimeError::Cancelled) {
                "start_cancelled"
            } else {
                "start_failed"
            })?;
            return Err(error);
        }
        if let Err(error) = ensure_not_cancelled(cancellation.as_deref()) {
            self.stop_process("start_cancelled")?;
            return Err(error);
        }

        let backend = BackendConfig::new(
            AGENT_BACKEND_ID,
            &format!("http://127.0.0.1:{port}/v1/"),
            Some(api_key),
        )
        .map_err(|_| AgentRuntimeError::StartFailed)?;
        if let Err(error) = self.gateway.upsert_routed_backend(backend) {
            self.stop_process("gateway_route_failed")?;
            return Err(error.into());
        }
        if let Err(error) =
            self.gateway
                .set_model_route(AGENT_MODEL_ALIAS, AGENT_BACKEND_ID, AGENT_MODEL_ALIAS)
        {
            let _ = self.gateway.remove_routed_backend(AGENT_BACKEND_ID);
            self.stop_process("gateway_route_failed")?;
            return Err(error.into());
        }
        if let Err(error) = ensure_not_cancelled(cancellation.as_deref()) {
            let _ = self.remove_gateway_route();
            self.stop_process("start_cancelled")?;
            return Err(error);
        }
        self.runtime
            .lock()
            .map_err(|_| AgentRuntimeError::LockPoisoned)?
            .state = AgentComponentState::Running;
        self.database.insert_audit_event(
            "agent_runtime_started",
            "agent_runtime",
            AGENT_MODEL_ALIAS,
            "{\"model\":\"Qwen3.5-2B Q4_K_M\"}",
            now_ms(),
        )?;
        self.status()
    }

    pub async fn stop(&self) -> Result<AgentStatus, AgentRuntimeError> {
        let _guard = self.lifecycle.lock().await;
        self.remove_gateway_route()?;
        if self.stop_process("stopped")? {
            self.database.insert_audit_event(
                "agent_runtime_stopped",
                "agent_runtime",
                AGENT_MODEL_ALIAS,
                "{}",
                now_ms(),
            )?;
        }
        self.status()
    }

    pub fn port(&self) -> Result<Option<u16>, AgentRuntimeError> {
        self.reconcile_exited_process()?;
        Ok(self
            .runtime
            .lock()
            .map_err(|_| AgentRuntimeError::LockPoisoned)?
            .session
            .as_ref()
            .map(|session| session.port))
    }

    fn agent_model(&self) -> Result<Option<hal100_protocol::LocalModelSummary>, DatabaseError> {
        self.database.refresh_local_model_states()?;
        Ok(self.database.local_models()?.into_iter().find(|model| {
            model.id == AGENT_MODEL_ID
                && model.state == LocalModelState::Ready
                && model.repository.as_deref() == Some(AGENT_MODEL_REPOSITORY)
                && model.revision.as_deref() == Some(AGENT_MODEL_REVISION)
                && model.file_name == AGENT_MODEL_FILE
                && model.size_bytes == AGENT_MODEL_SIZE_BYTES
        }))
    }

    async fn wait_until_ready(
        &self,
        port: u16,
        api_key: &str,
        cancellation: Option<&AtomicBool>,
    ) -> Result<(), AgentRuntimeError> {
        let started = Instant::now();
        let models_url = format!("http://127.0.0.1:{port}/v1/models");
        loop {
            ensure_not_cancelled(cancellation)?;
            {
                let mut runtime = self
                    .runtime
                    .lock()
                    .map_err(|_| AgentRuntimeError::LockPoisoned)?;
                let session = runtime
                    .session
                    .as_mut()
                    .ok_or(AgentRuntimeError::StartFailed)?;
                if session.child.try_wait()?.is_some() {
                    runtime.session = None;
                    runtime.state = AgentComponentState::Error;
                    runtime.last_error_code = Some("exited_before_ready".to_owned());
                    return Err(AgentRuntimeError::ExitedBeforeReady);
                }
            }
            let request = self.client.get(&models_url).bearer_auth(api_key).send();
            let response = if let Some(cancellation) = cancellation {
                tokio::select! {
                    response = request => response,
                    () = wait_for_cancellation(cancellation) => {
                        return Err(AgentRuntimeError::Cancelled);
                    }
                }
            } else {
                request.await
            };
            if response.is_ok_and(|response| response.status().is_success()) {
                return Ok(());
            }
            if started.elapsed() >= AGENT_START_TIMEOUT {
                return Err(AgentRuntimeError::StartTimeout);
            }
            for _ in 0..4 {
                tokio::time::sleep(Duration::from_millis(50)).await;
                ensure_not_cancelled(cancellation)?;
            }
        }
    }

    fn reconcile_exited_process(&self) -> Result<(), AgentRuntimeError> {
        let exited = {
            let mut runtime = self
                .runtime
                .lock()
                .map_err(|_| AgentRuntimeError::LockPoisoned)?;
            runtime
                .session
                .as_mut()
                .and_then(|session| session.child.try_wait().ok().flatten())
                .is_some()
        };
        if exited {
            let _ = self.remove_gateway_route();
            let mut runtime = self
                .runtime
                .lock()
                .map_err(|_| AgentRuntimeError::LockPoisoned)?;
            runtime.session = None;
            runtime.state = AgentComponentState::Error;
            runtime.last_error_code = Some("process_exited".to_owned());
        }
        Ok(())
    }

    fn remove_gateway_route(&self) -> Result<(), AgentRuntimeError> {
        let _ = self.gateway.remove_model_route(AGENT_MODEL_ALIAS)?;
        let _ = self.gateway.remove_routed_backend(AGENT_BACKEND_ID)?;
        Ok(())
    }

    fn stop_process(&self, reason: &str) -> Result<bool, AgentRuntimeError> {
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| AgentRuntimeError::LockPoisoned)?;
        let had_session = runtime.session.is_some();
        if let Some(mut session) = runtime.session.take() {
            let _ = session.child.kill();
            let _ = session.child.wait();
        }
        runtime.state = AgentComponentState::Stopped;
        runtime.last_error_code = (reason != "stopped").then(|| reason.to_owned());
        Ok(had_session)
    }
}

impl Drop for AgentModelRuntime {
    fn drop(&mut self) {
        let _ = self.gateway.remove_model_route(AGENT_MODEL_ALIAS);
        let _ = self.gateway.remove_routed_backend(AGENT_BACKEND_ID);
        if let Ok(runtime) = self.runtime.get_mut()
            && let Some(mut session) = runtime.session.take()
        {
            let _ = session.child.kill();
            let _ = session.child.wait();
        }
    }
}

struct EphemeralSecretFile(PathBuf);

impl EphemeralSecretFile {
    fn create(path: PathBuf, contents: &[u8]) -> Result<Self, AgentRuntimeError> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        file.write_all(contents)?;
        file.sync_all()?;
        Ok(Self(path))
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for EphemeralSecretFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn reserve_loopback_port() -> Result<u16, AgentRuntimeError> {
    let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .map_err(|_| AgentRuntimeError::PortUnavailable)?;
    listener
        .local_addr()
        .map(|address| address.port())
        .map_err(|_| AgentRuntimeError::PortUnavailable)
}

fn ensure_not_cancelled(cancellation: Option<&AtomicBool>) -> Result<(), AgentRuntimeError> {
    if cancellation.is_some_and(|cancellation| cancellation.load(Ordering::Acquire)) {
        Err(AgentRuntimeError::Cancelled)
    } else {
        Ok(())
    }
}

async fn wait_for_cancellation(cancellation: &AtomicBool) {
    while !cancellation.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}
