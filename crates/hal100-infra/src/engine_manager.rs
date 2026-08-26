use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use flate2::read::GzDecoder;
use futures_util::StreamExt;
use hal100_protocol::{
    EngineInstallPlan, EngineInstallState, EngineRemovePlan, EngineRuntimeState, LlamaCppStatus,
    LocalModelState,
};
use reqwest::{Client, StatusCode, Url, redirect::Policy};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tar::Archive;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use crate::{
    AgentRuntimeCapacityProfile, BackendConfig, Database, DatabaseError, GatewayRouteSwitchError,
    GatewayState,
};

const LLAMA_CPP_VERSION: &str = "b10218";
const LLAMA_CPP_ARCHIVE_NAME: &str = "llama-b10218-bin-macos-arm64.tar.gz";
const LLAMA_CPP_ARCHIVE_BYTES: u64 = 10_938_782;
const LLAMA_CPP_ARCHIVE_SHA256: &str =
    "f3e87f1664c09183a861f16758c55a5adc925672705cd3a47e3dc4444504c914";
const LLAMA_CPP_BINARY_SHA256: &str =
    "ff0e2445d93e2d6305c44cce6386db1020385194261dc184deaf0f37c7148d85";
const LLAMA_CPP_DOWNLOAD_URL: &str = "https://github.com/ggml-org/llama.cpp/releases/download/b10218/llama-b10218-bin-macos-arm64.tar.gz";
const ENGINE_PLAN_TTL_MS: i64 = 5 * 60 * 1_000;
const LLAMA_SERVER_START_TIMEOUT: Duration = Duration::from_secs(90);
const GATEWAY_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);
const LLAMA_SERVER_REASONING_MODE: &str = "off";

#[derive(Debug, Error)]
pub enum EngineManagerError {
    #[error(transparent)]
    Database(#[from] DatabaseError),
    #[error("当前构建只支持 Apple Silicon macOS 的托管 llama.cpp")]
    UnsupportedPlatform,
    #[error("llama.cpp 已经安装")]
    AlreadyInstalled,
    #[error("llama.cpp 尚未安装")]
    NotInstalled,
    #[error("引擎安装计划不存在或已被使用")]
    InstallPlanNotFound,
    #[error("引擎卸载计划不存在或已被使用")]
    RemovePlanNotFound,
    #[error("操作计划已过期，请重新检查")]
    PlanExpired,
    #[error("引擎安装包下载失败：{0}")]
    Network(String),
    #[error("引擎发布服务返回 HTTP {0}")]
    UpstreamStatus(u16),
    #[error("引擎安装包大小与固定发布清单不一致")]
    ArchiveSizeMismatch,
    #[error("引擎安装包 SHA-256 校验失败")]
    ArchiveHashMismatch,
    #[error("引擎安装包包含不安全路径")]
    UnsafeArchive,
    #[error("引擎安装包缺少 llama-server")]
    MissingServerBinary,
    #[error("llama-server 安装校验失败")]
    BinaryVerificationFailed,
    #[error("模型不存在或当前不可用")]
    ModelUnavailable,
    #[error("当前模型正在运行，请先停止或切换模型")]
    ModelIsRunning,
    #[error("模型完整性校验失败，请重新下载或导入")]
    ModelVerificationFailed,
    #[error("无法为本地推理服务分配安全端口")]
    PortUnavailable,
    #[error("llama-server 无法启动")]
    StartFailed,
    #[error("llama-server 在就绪前退出")]
    ExitedBeforeReady,
    #[error("等待 llama-server 就绪超时")]
    StartTimeout,
    #[error("引擎文件操作失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("引擎状态锁已损坏")]
    LockPoisoned,
    #[error("引擎清单无法读取")]
    InvalidManifest,
    #[error("Gateway 后端配置失败")]
    GatewayConfiguration,
    #[error("Gateway 活动请求未能在安全时限内排空")]
    GatewayDrain(#[from] GatewayRouteSwitchError),
    #[error("引擎工作线程异常结束")]
    WorkerFailed,
    #[error("引擎运行时资源校验已取消")]
    OperationCancelled,
}

#[derive(Clone)]
struct EngineRelease {
    version: String,
    archive_name: String,
    archive_size_bytes: u64,
    archive_sha256: [u8; 32],
    binary_sha256: [u8; 32],
    download_url: Url,
}

pub struct LlamaCppManager {
    database: Arc<Database>,
    gateway: GatewayState,
    engine_root: PathBuf,
    release: EngineRelease,
    client: Client,
    capacity: AgentRuntimeCapacityProfile,
    pending_install: Mutex<Option<EngineInstallPlan>>,
    pending_remove: Mutex<Option<EngineRemovePlan>>,
    lifecycle: AsyncMutex<()>,
    runtime: Mutex<RuntimeState>,
}

struct RuntimeState {
    state: EngineRuntimeState,
    session: Option<RuntimeSession>,
    last_error_code: Option<String>,
}

struct RuntimeSession {
    child: Child,
    model_id: String,
    model_name: String,
    previous_backend: Option<BackendConfig>,
    gateway_active: bool,
    port: u16,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallManifest {
    version: String,
    archive_sha256: String,
    binary_sha256: String,
}

impl LlamaCppManager {
    pub fn new(
        database: Arc<Database>,
        gateway: GatewayState,
        engine_root: PathBuf,
    ) -> Result<Self, EngineManagerError> {
        Self::with_capacity(
            database,
            gateway,
            engine_root,
            AgentRuntimeCapacityProfile::baseline(),
        )
    }

    pub fn with_capacity(
        database: Arc<Database>,
        gateway: GatewayState,
        engine_root: PathBuf,
        capacity: AgentRuntimeCapacityProfile,
    ) -> Result<Self, EngineManagerError> {
        if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            return Err(EngineManagerError::UnsupportedPlatform);
        }
        let release = EngineRelease {
            version: LLAMA_CPP_VERSION.to_owned(),
            archive_name: LLAMA_CPP_ARCHIVE_NAME.to_owned(),
            archive_size_bytes: LLAMA_CPP_ARCHIVE_BYTES,
            archive_sha256: decode_sha256(LLAMA_CPP_ARCHIVE_SHA256)
                .expect("pinned llama.cpp SHA-256 is valid"),
            binary_sha256: decode_sha256(LLAMA_CPP_BINARY_SHA256)
                .expect("pinned llama-server SHA-256 is valid"),
            download_url: Url::parse(LLAMA_CPP_DOWNLOAD_URL)
                .expect("pinned llama.cpp URL is valid"),
        };
        Self::with_release_and_capacity(database, gateway, engine_root, release, capacity)
    }

    #[cfg(test)]
    fn with_release(
        database: Arc<Database>,
        gateway: GatewayState,
        engine_root: PathBuf,
        release: EngineRelease,
    ) -> Result<Self, EngineManagerError> {
        Self::with_release_and_capacity(
            database,
            gateway,
            engine_root,
            release,
            AgentRuntimeCapacityProfile::baseline(),
        )
    }

    fn with_release_and_capacity(
        database: Arc<Database>,
        gateway: GatewayState,
        engine_root: PathBuf,
        release: EngineRelease,
        capacity: AgentRuntimeCapacityProfile,
    ) -> Result<Self, EngineManagerError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30))
            .redirect(Policy::limited(5))
            .user_agent(concat!(
                "HAL100/",
                env!("CARGO_PKG_VERSION"),
                " llama-cpp-install"
            ))
            .build()
            .map_err(|error| EngineManagerError::Network(network_error(&error)))?;
        Ok(Self {
            database,
            gateway,
            engine_root,
            release,
            client,
            capacity,
            pending_install: Mutex::new(None),
            pending_remove: Mutex::new(None),
            lifecycle: AsyncMutex::new(()),
            runtime: Mutex::new(RuntimeState {
                state: EngineRuntimeState::Stopped,
                session: None,
                last_error_code: None,
            }),
        })
    }

    pub fn status(&self) -> Result<LlamaCppStatus, EngineManagerError> {
        let install_state = self.install_state();
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| EngineManagerError::LockPoisoned)?;
        let exited = runtime
            .session
            .as_mut()
            .and_then(|session| session.child.try_wait().ok().flatten())
            .is_some();
        if exited {
            let session = runtime.session.take().expect("checked session");
            if session.gateway_active {
                self.gateway.replace_backend(session.previous_backend);
            }
            runtime.state = EngineRuntimeState::Error;
            runtime.last_error_code = Some("process_exited".to_owned());
        }
        let port = runtime.session.as_ref().map(|session| session.port);
        Ok(LlamaCppStatus {
            version: self.release.version.clone(),
            install_state,
            runtime_state: runtime.state,
            active_model_id: runtime
                .session
                .as_ref()
                .map(|session| session.model_id.clone()),
            active_model_name: runtime
                .session
                .as_ref()
                .map(|session| session.model_name.clone()),
            port,
            last_error_code: runtime.last_error_code.clone(),
        })
    }

    pub fn plan_install(&self) -> Result<EngineInstallPlan, EngineManagerError> {
        if self.install_state() == EngineInstallState::Installed {
            return Err(EngineManagerError::AlreadyInstalled);
        }
        let now = now_ms();
        let plan = EngineInstallPlan {
            plan_id: Uuid::new_v4().to_string(),
            expires_at_ms: now.saturating_add(ENGINE_PLAN_TTL_MS),
            engine: "llama.cpp".to_owned(),
            version: self.release.version.clone(),
            archive_size_bytes: self.release.archive_size_bytes,
            publisher: "ggml-org/llama.cpp GitHub Releases".to_owned(),
            action_summary:
                "下载固定版本的 Apple Silicon 官方构建，校验 SHA-256 后安装到 HAL100 托管目录"
                    .to_owned(),
            requires_confirmation: true,
        };
        *self
            .pending_install
            .lock()
            .map_err(|_| EngineManagerError::LockPoisoned)? = Some(plan.clone());
        Ok(plan)
    }

    pub async fn apply_install(&self, plan_id: &str) -> Result<LlamaCppStatus, EngineManagerError> {
        let _lifecycle = self.lifecycle.lock().await;
        let plan = {
            let mut pending = self
                .pending_install
                .lock()
                .map_err(|_| EngineManagerError::LockPoisoned)?;
            if !pending.as_ref().is_some_and(|plan| plan.plan_id == plan_id) {
                return Err(EngineManagerError::InstallPlanNotFound);
            }
            pending.take().expect("matching install plan")
        };
        if now_ms() > plan.expires_at_ms {
            return Err(EngineManagerError::PlanExpired);
        }
        if self.install_state() == EngineInstallState::Installed {
            return Err(EngineManagerError::AlreadyInstalled);
        }

        tokio::fs::create_dir_all(&self.engine_root).await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&self.engine_root, std::fs::Permissions::from_mode(0o700))
                .await?;
        }
        let operation_id = Uuid::new_v4().to_string();
        let archive_path = self
            .engine_root
            .join(format!(".{}-{operation_id}", self.release.archive_name));
        let staging_path = self.engine_root.join(format!(".staging-{operation_id}"));
        let result = self.install_release(&archive_path, &staging_path).await;
        let _ = tokio::fs::remove_file(&archive_path).await;
        let _ = tokio::fs::remove_dir_all(&staging_path).await;
        result?;
        self.database.insert_audit_event(
            "engine_installed",
            "engine",
            "llama.cpp",
            &json!({
                "engine": "llama.cpp",
                "source": "github_release",
                "version": self.release.version,
            })
            .to_string(),
            now_ms(),
        )?;
        self.status()
    }

    pub fn discard_install_plan(&self, plan_id: &str) -> Result<bool, EngineManagerError> {
        let mut pending = self
            .pending_install
            .lock()
            .map_err(|_| EngineManagerError::LockPoisoned)?;
        if pending.as_ref().is_some_and(|plan| plan.plan_id == plan_id) {
            pending.take();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn plan_remove(&self) -> Result<EngineRemovePlan, EngineManagerError> {
        if self.install_state() == EngineInstallState::NotInstalled {
            return Err(EngineManagerError::NotInstalled);
        }
        let now = now_ms();
        let plan = EngineRemovePlan {
            plan_id: Uuid::new_v4().to_string(),
            expires_at_ms: now.saturating_add(ENGINE_PLAN_TTL_MS),
            engine: "llama.cpp".to_owned(),
            version: self.release.version.clone(),
            install_path: self.install_path().display().to_string(),
            action_summary:
                "停止 HAL100 托管的 llama-server，并删除该固定版本的引擎文件；不会删除任何模型"
                    .to_owned(),
            requires_confirmation: true,
        };
        *self
            .pending_remove
            .lock()
            .map_err(|_| EngineManagerError::LockPoisoned)? = Some(plan.clone());
        Ok(plan)
    }

    pub async fn apply_remove(&self, plan_id: &str) -> Result<LlamaCppStatus, EngineManagerError> {
        let _lifecycle = self.lifecycle.lock().await;
        let plan = {
            let mut pending = self
                .pending_remove
                .lock()
                .map_err(|_| EngineManagerError::LockPoisoned)?;
            if !pending.as_ref().is_some_and(|plan| plan.plan_id == plan_id) {
                return Err(EngineManagerError::RemovePlanNotFound);
            }
            pending.take().expect("matching remove plan")
        };
        if now_ms() > plan.expires_at_ms {
            return Err(EngineManagerError::PlanExpired);
        }
        self.stop_internal_when_idle(None).await?;
        let install_path = self.install_path();
        if !install_path.starts_with(&self.engine_root) {
            return Err(EngineManagerError::UnsafeArchive);
        }
        match std::fs::remove_dir_all(&install_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(EngineManagerError::NotInstalled);
            }
            Err(error) => return Err(error.into()),
        }
        self.database.insert_audit_event(
            "engine_removed",
            "engine",
            "llama.cpp",
            &json!({"engine": "llama.cpp", "version": self.release.version}).to_string(),
            now_ms(),
        )?;
        self.status()
    }

    pub fn discard_remove_plan(&self, plan_id: &str) -> Result<bool, EngineManagerError> {
        let mut pending = self
            .pending_remove
            .lock()
            .map_err(|_| EngineManagerError::LockPoisoned)?;
        if pending.as_ref().is_some_and(|plan| plan.plan_id == plan_id) {
            pending.take();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn start_model(&self, model_id: &str) -> Result<LlamaCppStatus, EngineManagerError> {
        self.start_model_with_policy(model_id, false).await
    }

    pub async fn run_if_model_inactive<T, E>(
        &self,
        model_id: &str,
        operation: impl FnOnce() -> Result<T, E> + Send + 'static,
    ) -> Result<Result<T, E>, EngineManagerError>
    where
        T: Send + 'static,
        E: Send + 'static,
    {
        let _lifecycle = self.lifecycle.lock().await;
        let model_is_active = self
            .runtime
            .lock()
            .map_err(|_| EngineManagerError::LockPoisoned)?
            .session
            .as_ref()
            .is_some_and(|session| session.model_id == model_id);
        if model_is_active {
            return Err(EngineManagerError::ModelIsRunning);
        }
        tokio::task::spawn_blocking(operation)
            .await
            .map_err(|_| EngineManagerError::WorkerFailed)
    }

    pub async fn force_start_model(
        &self,
        model_id: &str,
    ) -> Result<LlamaCppStatus, EngineManagerError> {
        self.start_model_with_policy(model_id, true).await
    }

    pub async fn verified_runtime_assets(
        &self,
        model_id: &str,
    ) -> Result<(PathBuf, hal100_protocol::LocalModelSummary), EngineManagerError> {
        self.verified_runtime_assets_inner(model_id, None).await
    }

    pub async fn verified_runtime_assets_cancellable(
        &self,
        model_id: &str,
        cancellation: Arc<AtomicBool>,
    ) -> Result<(PathBuf, hal100_protocol::LocalModelSummary), EngineManagerError> {
        self.verified_runtime_assets_inner(model_id, Some(cancellation))
            .await
    }

    async fn verified_runtime_assets_inner(
        &self,
        model_id: &str,
        cancellation: Option<Arc<AtomicBool>>,
    ) -> Result<(PathBuf, hal100_protocol::LocalModelSummary), EngineManagerError> {
        ensure_not_cancelled(cancellation.as_deref())?;
        if self.install_state() != EngineInstallState::Installed {
            return Err(EngineManagerError::NotInstalled);
        }
        self.database.refresh_local_model_states()?;
        let model = self
            .database
            .local_model(model_id)?
            .filter(|model| model.state == LocalModelState::Ready)
            .ok_or(EngineManagerError::ModelUnavailable)?;
        let model_path = PathBuf::from(&model.path);
        if !model_path.is_file()
            || std::fs::symlink_metadata(&model_path)?
                .file_type()
                .is_symlink()
        {
            return Err(EngineManagerError::ModelUnavailable);
        }
        self.verify_model_with_cancellation(&model.id, &model_path, cancellation.clone())
            .await?;
        self.verify_binary_with_cancellation(cancellation).await?;
        Ok((self.binary_path(), model))
    }

    async fn start_model_with_policy(
        &self,
        model_id: &str,
        force_switch: bool,
    ) -> Result<LlamaCppStatus, EngineManagerError> {
        let _lifecycle = self.lifecycle.lock().await;
        if self.install_state() != EngineInstallState::Installed {
            return Err(EngineManagerError::NotInstalled);
        }
        self.database.refresh_local_model_states()?;
        let model = self
            .database
            .local_model(model_id)?
            .filter(|model| model.state == LocalModelState::Ready)
            .ok_or(EngineManagerError::ModelUnavailable)?;
        let model_path = PathBuf::from(&model.path);
        if !model_path.is_file()
            || std::fs::symlink_metadata(&model_path)?
                .file_type()
                .is_symlink()
        {
            return Err(EngineManagerError::ModelUnavailable);
        }
        self.verify_model(&model.id, &model_path).await?;
        self.verify_binary().await?;
        if force_switch {
            self.stop_internal_force(None).await?;
        } else {
            self.stop_internal_when_idle(None).await?;
        }
        let port = reserve_loopback_port()?;
        let api_key = format!(
            "hal100_backend_{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        let api_key_file = EphemeralSecretFile::create(
            self.install_path()
                .join(format!(".session-{}.key", Uuid::new_v4().simple())),
            api_key.as_bytes(),
        )?;

        let binary = self.binary_path();
        let child = Command::new(&binary)
            .arg("--model")
            .arg(&model.path)
            .arg("--alias")
            .arg("hal100-active")
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .arg("--ctx-size")
            .arg(self.capacity.context_window_tokens.to_string())
            .arg("--parallel")
            .arg("1")
            // HAL100's standard OpenAI-compatible route requires assistant text in
            // message.content. Hybrid models such as Qwen3.5 may otherwise spend the
            // entire completion budget in reasoning_content and return empty content.
            .arg("--reasoning")
            .arg(LLAMA_SERVER_REASONING_MODE)
            .arg("--api-key-file")
            .arg(api_key_file.path())
            .current_dir(self.install_path())
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| EngineManagerError::StartFailed)?;
        {
            let mut runtime = self
                .runtime
                .lock()
                .map_err(|_| EngineManagerError::LockPoisoned)?;
            runtime.state = EngineRuntimeState::Starting;
            runtime.last_error_code = None;
            runtime.session = Some(RuntimeSession {
                child,
                model_id: model.id,
                model_name: model.display_name,
                previous_backend: None,
                gateway_active: false,
                port,
            });
        }

        let started = std::time::Instant::now();
        let models_url = format!("http://127.0.0.1:{port}/v1/models");
        loop {
            {
                let mut runtime = self
                    .runtime
                    .lock()
                    .map_err(|_| EngineManagerError::LockPoisoned)?;
                let Some(session) = runtime.session.as_mut() else {
                    return Err(EngineManagerError::StartFailed);
                };
                if session.child.try_wait()?.is_some() {
                    runtime.session = None;
                    runtime.state = EngineRuntimeState::Error;
                    runtime.last_error_code = Some("exited_before_ready".to_owned());
                    return Err(EngineManagerError::ExitedBeforeReady);
                }
            }
            if self
                .client
                .get(&models_url)
                .bearer_auth(&api_key)
                .send()
                .await
                .is_ok_and(|response| response.status().is_success())
            {
                tokio::time::sleep(Duration::from_millis(100)).await;
                let mut runtime = self
                    .runtime
                    .lock()
                    .map_err(|_| EngineManagerError::LockPoisoned)?;
                let Some(session) = runtime.session.as_mut() else {
                    return Err(EngineManagerError::StartFailed);
                };
                if session.child.try_wait()?.is_some() {
                    runtime.session = None;
                    runtime.state = EngineRuntimeState::Error;
                    runtime.last_error_code = Some("exited_before_ready".to_owned());
                    return Err(EngineManagerError::ExitedBeforeReady);
                }
                break;
            }
            if started.elapsed() >= LLAMA_SERVER_START_TIMEOUT {
                self.stop_internal_when_idle(Some("start_timeout")).await?;
                return Err(EngineManagerError::StartTimeout);
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        let backend = BackendConfig::new(
            "managed-llama-cpp",
            &format!("http://127.0.0.1:{port}/v1/"),
            Some(api_key),
        )
        .map_err(|_| EngineManagerError::GatewayConfiguration)?;
        let backend_switch = if force_switch {
            self.gateway.force_replace_backend(Some(backend)).await
        } else {
            self.gateway
                .replace_backend_when_idle(Some(backend), GATEWAY_DRAIN_TIMEOUT)
                .await
        };
        let previous_backend = match backend_switch {
            Ok(previous_backend) => previous_backend,
            Err(error) => {
                let error_code = if force_switch {
                    "gateway_switch_failed"
                } else {
                    "gateway_drain_timeout"
                };
                self.stop_internal(Some(error_code))?;
                return Err(error.into());
            }
        };
        let session_missing = {
            let mut runtime = self
                .runtime
                .lock()
                .map_err(|_| EngineManagerError::LockPoisoned)?;
            if let Some(session) = runtime.session.as_mut() {
                session.previous_backend = previous_backend.clone();
                session.gateway_active = true;
                runtime.state = EngineRuntimeState::Running;
                false
            } else {
                true
            }
        };
        if session_missing {
            self.gateway.force_replace_backend(previous_backend).await?;
            return Err(EngineManagerError::StartFailed);
        }
        self.status()
    }

    pub async fn stop(&self) -> Result<LlamaCppStatus, EngineManagerError> {
        let _lifecycle = self.lifecycle.lock().await;
        self.stop_internal_when_idle(None).await?;
        self.status()
    }

    pub async fn force_stop(&self) -> Result<LlamaCppStatus, EngineManagerError> {
        let _lifecycle = self.lifecycle.lock().await;
        self.stop_internal_force(None).await?;
        self.status()
    }

    async fn install_release(
        &self,
        archive_path: &Path,
        staging_path: &Path,
    ) -> Result<(), EngineManagerError> {
        let response = self
            .client
            .get(self.release.download_url.clone())
            .send()
            .await
            .map_err(|error| EngineManagerError::Network(network_error(&error)))?;
        if response.status() != StatusCode::OK {
            return Err(EngineManagerError::UpstreamStatus(
                response.status().as_u16(),
            ));
        }
        if response
            .content_length()
            .is_some_and(|size| size > self.release.archive_size_bytes)
        {
            return Err(EngineManagerError::ArchiveSizeMismatch);
        }
        let mut archive_file = tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(archive_path)
            .await?;
        let mut received = 0_u64;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|error| EngineManagerError::Network(network_error(&error)))?;
            received = received
                .checked_add(chunk.len() as u64)
                .ok_or(EngineManagerError::ArchiveSizeMismatch)?;
            if received > self.release.archive_size_bytes {
                return Err(EngineManagerError::ArchiveSizeMismatch);
            }
            archive_file.write_all(&chunk).await?;
        }
        if received != self.release.archive_size_bytes {
            return Err(EngineManagerError::ArchiveSizeMismatch);
        }
        archive_file.sync_all().await?;
        drop(archive_file);

        let hash_path = archive_path.to_owned();
        let actual_hash = tokio::task::spawn_blocking(move || sha256_file(&hash_path))
            .await
            .map_err(|_| EngineManagerError::WorkerFailed)??;
        if actual_hash != self.release.archive_sha256 {
            return Err(EngineManagerError::ArchiveHashMismatch);
        }
        let archive_path = archive_path.to_owned();
        let staging_path = staging_path.to_owned();
        let extraction_path = staging_path.clone();
        tokio::task::spawn_blocking(move || extract_archive(&archive_path, &extraction_path))
            .await
            .map_err(|_| EngineManagerError::WorkerFailed)??;

        let extracted = staging_path.join(format!("llama-{}", self.release.version));
        let binary = extracted.join("llama-server");
        let metadata = std::fs::symlink_metadata(&binary)
            .map_err(|_| EngineManagerError::MissingServerBinary)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(EngineManagerError::MissingServerBinary);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))?;
        }
        let binary_hash = sha256_file(&binary)?;
        if binary_hash != self.release.binary_sha256 {
            return Err(EngineManagerError::BinaryVerificationFailed);
        }
        let manifest = InstallManifest {
            version: self.release.version.clone(),
            archive_sha256: encode_sha256(&self.release.archive_sha256),
            binary_sha256: encode_sha256(&self.release.binary_sha256),
        };
        std::fs::write(
            extracted.join(".hal100-manifest.json"),
            serde_json::to_vec_pretty(&manifest)
                .map_err(|_| EngineManagerError::InvalidManifest)?,
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&extracted, std::fs::Permissions::from_mode(0o700))?;
            std::fs::set_permissions(
                extracted.join(".hal100-manifest.json"),
                std::fs::Permissions::from_mode(0o600),
            )?;
        }
        let install_path = self.install_path();
        if install_path.exists() {
            return Err(EngineManagerError::AlreadyInstalled);
        }
        std::fs::rename(extracted, install_path)?;
        Ok(())
    }

    async fn verify_binary(&self) -> Result<(), EngineManagerError> {
        self.verify_binary_with_cancellation(None).await
    }

    async fn verify_binary_with_cancellation(
        &self,
        cancellation: Option<Arc<AtomicBool>>,
    ) -> Result<(), EngineManagerError> {
        ensure_not_cancelled(cancellation.as_deref())?;
        let manifest = self.read_manifest()?;
        if decode_sha256(&manifest.binary_sha256) != Some(self.release.binary_sha256) {
            return Err(EngineManagerError::BinaryVerificationFailed);
        }
        let binary_path = self.binary_path();
        let metadata = std::fs::symlink_metadata(&binary_path)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(EngineManagerError::BinaryVerificationFailed);
        }
        let actual_hash = tokio::task::spawn_blocking(move || {
            sha256_file_with_cancellation(&binary_path, cancellation.as_deref())
        })
        .await
        .map_err(|_| EngineManagerError::WorkerFailed)??;
        if actual_hash != self.release.binary_sha256 {
            return Err(EngineManagerError::BinaryVerificationFailed);
        }
        Ok(())
    }

    fn install_state(&self) -> EngineInstallState {
        let path = self.install_path();
        if !path.exists() {
            return EngineInstallState::NotInstalled;
        }
        match self.read_manifest() {
            Ok(manifest)
                if manifest.version == self.release.version
                    && decode_sha256(&manifest.archive_sha256)
                        == Some(self.release.archive_sha256)
                    && decode_sha256(&manifest.binary_sha256)
                        == Some(self.release.binary_sha256)
                    && std::fs::symlink_metadata(self.binary_path()).is_ok_and(|metadata| {
                        metadata.is_file() && !metadata.file_type().is_symlink()
                    }) =>
            {
                EngineInstallState::Installed
            }
            _ => EngineInstallState::VerificationFailed,
        }
    }

    fn read_manifest(&self) -> Result<InstallManifest, EngineManagerError> {
        let bytes = std::fs::read(self.install_path().join(".hal100-manifest.json"))?;
        serde_json::from_slice(&bytes).map_err(|_| EngineManagerError::InvalidManifest)
    }

    fn install_path(&self) -> PathBuf {
        self.engine_root.join(&self.release.version)
    }

    fn binary_path(&self) -> PathBuf {
        self.install_path().join("llama-server")
    }

    async fn verify_model(
        &self,
        model_id: &str,
        model_path: &Path,
    ) -> Result<(), EngineManagerError> {
        self.verify_model_with_cancellation(model_id, model_path, None)
            .await
    }

    async fn verify_model_with_cancellation(
        &self,
        model_id: &str,
        model_path: &Path,
        cancellation: Option<Arc<AtomicBool>>,
    ) -> Result<(), EngineManagerError> {
        ensure_not_cancelled(cancellation.as_deref())?;
        let integrity = self
            .database
            .model_integrity(model_id)?
            .ok_or(EngineManagerError::ModelUnavailable)?;
        let Some(expected) = integrity.sha256 else {
            self.database.mark_model_verification_failed(model_id)?;
            return Err(EngineManagerError::ModelVerificationFailed);
        };
        if Path::new(&integrity.path) != model_path {
            self.database.mark_model_verification_failed(model_id)?;
            return Err(EngineManagerError::ModelVerificationFailed);
        }
        let path = model_path.to_owned();
        let actual = tokio::task::spawn_blocking(move || {
            sha256_file_with_cancellation(&path, cancellation.as_deref())
        })
        .await
        .map_err(|_| EngineManagerError::WorkerFailed)?;
        let actual = match actual {
            Ok(hash) => hash,
            Err(EngineManagerError::OperationCancelled) => {
                return Err(EngineManagerError::OperationCancelled);
            }
            Err(error) => {
                self.database.mark_model_verification_failed(model_id)?;
                return Err(error);
            }
        };
        if actual != expected {
            self.database.mark_model_verification_failed(model_id)?;
            return Err(EngineManagerError::ModelVerificationFailed);
        }
        Ok(())
    }

    fn stop_internal(&self, error_code: Option<&str>) -> Result<(), EngineManagerError> {
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| EngineManagerError::LockPoisoned)?;
        if let Some(mut session) = runtime.session.take() {
            let _ = session.child.kill();
            let _ = session.child.wait();
            if session.gateway_active {
                self.gateway.replace_backend(session.previous_backend);
            }
        }
        runtime.state = if error_code.is_some() {
            EngineRuntimeState::Error
        } else {
            EngineRuntimeState::Stopped
        };
        runtime.last_error_code = error_code.map(str::to_owned);
        Ok(())
    }

    async fn stop_internal_when_idle(
        &self,
        error_code: Option<&str>,
    ) -> Result<(), EngineManagerError> {
        let previous_backend = {
            let runtime = self
                .runtime
                .lock()
                .map_err(|_| EngineManagerError::LockPoisoned)?;
            runtime.session.as_ref().and_then(|session| {
                session
                    .gateway_active
                    .then(|| session.previous_backend.clone())
            })
        };
        if let Some(previous_backend) = previous_backend {
            self.gateway
                .replace_backend_when_idle(previous_backend, GATEWAY_DRAIN_TIMEOUT)
                .await?;
            let mut runtime = self
                .runtime
                .lock()
                .map_err(|_| EngineManagerError::LockPoisoned)?;
            if let Some(session) = runtime.session.as_mut() {
                session.gateway_active = false;
            }
        }
        self.stop_internal(error_code)
    }

    async fn stop_internal_force(
        &self,
        error_code: Option<&str>,
    ) -> Result<(), EngineManagerError> {
        let previous_backend = {
            let runtime = self
                .runtime
                .lock()
                .map_err(|_| EngineManagerError::LockPoisoned)?;
            runtime.session.as_ref().and_then(|session| {
                session
                    .gateway_active
                    .then(|| session.previous_backend.clone())
            })
        };
        if let Some(previous_backend) = previous_backend {
            self.gateway.force_replace_backend(previous_backend).await?;
            let mut runtime = self
                .runtime
                .lock()
                .map_err(|_| EngineManagerError::LockPoisoned)?;
            if let Some(session) = runtime.session.as_mut() {
                session.gateway_active = false;
            }
        }
        self.stop_internal(error_code)
    }
}

impl Drop for LlamaCppManager {
    fn drop(&mut self) {
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
    fn create(path: PathBuf, contents: &[u8]) -> Result<Self, EngineManagerError> {
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

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for EphemeralSecretFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn reserve_loopback_port() -> Result<u16, EngineManagerError> {
    let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .map_err(|_| EngineManagerError::PortUnavailable)?;
    listener
        .local_addr()
        .map(|address| address.port())
        .map_err(|_| EngineManagerError::PortUnavailable)
}

fn extract_archive(archive_path: &Path, staging_path: &Path) -> Result<(), EngineManagerError> {
    std::fs::create_dir(staging_path)?;
    let file = File::open(archive_path)?;
    let mut archive = Archive::new(GzDecoder::new(file));
    for item in archive
        .entries()
        .map_err(|_| EngineManagerError::UnsafeArchive)?
    {
        let mut entry = item.map_err(|_| EngineManagerError::UnsafeArchive)?;
        let path = entry
            .path()
            .map_err(|_| EngineManagerError::UnsafeArchive)?;
        if !safe_archive_path(&path) {
            return Err(EngineManagerError::UnsafeArchive);
        }
        if let Some(link) = entry
            .link_name()
            .map_err(|_| EngineManagerError::UnsafeArchive)?
            && !safe_archive_path(&link)
        {
            return Err(EngineManagerError::UnsafeArchive);
        }
        if !entry
            .unpack_in(staging_path)
            .map_err(|_| EngineManagerError::UnsafeArchive)?
        {
            return Err(EngineManagerError::UnsafeArchive);
        }
    }
    Ok(())
}

fn safe_archive_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

fn sha256_file(path: &Path) -> Result<[u8; 32], EngineManagerError> {
    sha256_file_with_cancellation(path, None)
}

fn sha256_file_with_cancellation(
    path: &Path,
    cancellation: Option<&AtomicBool>,
) -> Result<[u8; 32], EngineManagerError> {
    ensure_not_cancelled(cancellation)?;
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        ensure_not_cancelled(cancellation)?;
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    ensure_not_cancelled(cancellation)?;
    Ok(hasher.finalize().into())
}

fn ensure_not_cancelled(cancellation: Option<&AtomicBool>) -> Result<(), EngineManagerError> {
    if cancellation.is_some_and(|cancellation| cancellation.load(Ordering::Acquire)) {
        Err(EngineManagerError::OperationCancelled)
    } else {
        Ok(())
    }
}

fn decode_sha256(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut output = [0_u8; 32];
    for (index, byte) in output.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(output)
}

fn encode_sha256(value: &[u8; 32]) -> String {
    use std::fmt::Write;
    let mut output = String::with_capacity(64);
    for byte in value {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn network_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "读取超时".to_owned()
    } else if error.is_connect() {
        "无法连接".to_owned()
    } else {
        "传输失败".to_owned()
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, response::Response, routing::get};
    use flate2::{Compression, write::GzEncoder};

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("hal100-engine-{}", Uuid::new_v4()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn runtime_asset_hashing_honors_a_preexisting_cancellation() {
        let temp = TestDirectory::new();
        let artifact = temp.0.join("artifact.bin");
        std::fs::write(&artifact, b"fixed artifact").expect("test artifact");
        let cancellation = AtomicBool::new(true);

        assert!(matches!(
            sha256_file_with_cancellation(&artifact, Some(&cancellation)),
            Err(EngineManagerError::OperationCancelled)
        ));
    }

    #[tokio::test]
    async fn install_and_remove_require_one_use_confirmation_plans() {
        let archive = test_archive();
        let archive_hash: [u8; 32] = Sha256::digest(&archive).into();
        let binary_hash: [u8; 32] = Sha256::digest(b"#!/bin/sh\nexit 0\n").into();
        let app = Router::new().route(
            "/engine.tar.gz",
            get({
                let archive = archive.clone();
                move || {
                    let archive = archive.clone();
                    async move { Response::new(Body::from(archive)) }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let temp = TestDirectory::new();
        let database = Arc::new(Database::open(temp.0.join("db.sqlite")).unwrap());
        let credential = crate::stored_client_credential(
            "test-key",
            "test-client",
            "Test client",
            "hal100_test_0123456789abcdef",
        )
        .unwrap();
        let gateway = crate::GatewayState::new(
            None,
            crate::CredentialRegistry::new(vec![credential]),
            crate::UsageWriter::start(database.clone()),
        )
        .unwrap();
        let manager = LlamaCppManager::with_release(
            database.clone(),
            gateway,
            temp.0.join("engines"),
            EngineRelease {
                version: "test-build".to_owned(),
                archive_name: "engine.tar.gz".to_owned(),
                archive_size_bytes: archive.len() as u64,
                archive_sha256: archive_hash,
                binary_sha256: binary_hash,
                download_url: Url::parse(&format!("http://{address}/engine.tar.gz")).unwrap(),
            },
        )
        .unwrap();

        let plan = manager.plan_install().expect("install plan");
        assert!(plan.requires_confirmation);
        assert_eq!(
            manager.status().unwrap().install_state,
            EngineInstallState::NotInstalled
        );
        let installed = manager.apply_install(&plan.plan_id).await.expect("install");
        assert_eq!(installed.install_state, EngineInstallState::Installed);
        assert!(matches!(
            manager.apply_install(&plan.plan_id).await,
            Err(EngineManagerError::InstallPlanNotFound)
        ));
        assert_eq!(database.audit_event_count().unwrap(), 1);

        let remove = manager.plan_remove().expect("remove plan");
        assert!(remove.action_summary.contains("不会删除任何模型"));
        let removed = manager.apply_remove(&remove.plan_id).await.expect("remove");
        assert_eq!(removed.install_state, EngineInstallState::NotInstalled);
        assert_eq!(database.audit_event_count().unwrap(), 2);
        server.abort();
    }

    #[test]
    fn archive_paths_cannot_escape_staging() {
        assert!(safe_archive_path(Path::new("llama-build/llama-server")));
        assert!(!safe_archive_path(Path::new("../outside")));
        assert!(!safe_archive_path(Path::new("/tmp/outside")));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn starting_and_stopping_a_model_switches_the_gateway_backend() {
        use hal100_protocol::{LocalModelSummary, ModelOwnership, ModelSource};
        use std::os::unix::fs::PermissionsExt;

        let temp = TestDirectory::new();
        let database = Arc::new(Database::open(temp.0.join("db.sqlite")).unwrap());
        let credential = crate::stored_client_credential(
            "test-key",
            "test-client",
            "Test client",
            "hal100_test_0123456789abcdef",
        )
        .unwrap();
        let gateway = crate::GatewayState::new(
            None,
            crate::CredentialRegistry::new(vec![credential]),
            crate::UsageWriter::start(database.clone()),
        )
        .unwrap();
        let engine_root = temp.0.join("engines");
        let install_path = engine_root.join("test-runtime");
        std::fs::create_dir_all(&install_path).unwrap();
        let binary = install_path.join("llama-server");
        std::fs::write(
            &binary,
            br##"#!/usr/bin/python3
import http.server
import socketserver
import sys

port = int(sys.argv[sys.argv.index("--port") + 1])
key_file = sys.argv[sys.argv.index("--api-key-file") + 1]
reasoning = sys.argv[sys.argv.index("--reasoning") + 1]
if reasoning != "off":
    sys.exit(64)
context_window = sys.argv[sys.argv.index("--ctx-size") + 1]
if context_window != "16384":
    sys.exit(65)
with open(key_file, "r", encoding="utf-8") as handle:
    api_key = handle.read()

class Handler(http.server.BaseHTTPRequestHandler):
    def authorized(self):
        if self.headers.get("Authorization") == "Bearer " + api_key:
            return True
        self.send_response(401)
        self.end_headers()
        return False
    def do_GET(self):
        if not self.authorized():
            return
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(b'{"status":"ok"}')
    def do_POST(self):
        if not self.authorized():
            return
        length = int(self.headers.get("Content-Length", "0"))
        self.rfile.read(length)
        body = b'{"id":"fake","object":"chat.completion","model":"hal100-active","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}}'
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, format, *args):
        pass

socketserver.TCPServer.allow_reuse_address = True
with socketserver.TCPServer(("127.0.0.1", port), Handler) as server:
    server.serve_forever()
"##,
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        let binary_hash = sha256_file(&binary).unwrap();
        let release_hash = [7_u8; 32];
        std::fs::write(
            install_path.join(".hal100-manifest.json"),
            serde_json::to_vec(&InstallManifest {
                version: "test-runtime".to_owned(),
                archive_sha256: encode_sha256(&release_hash),
                binary_sha256: encode_sha256(&binary_hash),
            })
            .unwrap(),
        )
        .unwrap();
        let model_path = temp.0.join("model.gguf");
        std::fs::write(&model_path, gguf_payload()).unwrap();
        let model = LocalModelSummary {
            id: "test-model".to_owned(),
            display_name: "测试模型".to_owned(),
            format: "gguf".to_owned(),
            quantization: Some("Q4_K_M".to_owned()),
            source: ModelSource::LocalFile,
            repository: None,
            revision: None,
            file_name: "model.gguf".to_owned(),
            ownership: ModelOwnership::External,
            license: None,
            state: LocalModelState::Ready,
            path: model_path.display().to_string(),
            size_bytes: std::fs::metadata(&model_path).unwrap().len(),
        };
        let modified_at_ms = std::fs::metadata(&model_path)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        database
            .upsert_external_model(
                &model,
                modified_at_ms,
                &sha256_file(&model_path).unwrap(),
                now_ms(),
            )
            .unwrap();
        let manager = LlamaCppManager::with_release(
            database.clone(),
            gateway.clone(),
            engine_root,
            EngineRelease {
                version: "test-runtime".to_owned(),
                archive_name: "unused.tar.gz".to_owned(),
                archive_size_bytes: 1,
                archive_sha256: release_hash,
                binary_sha256: binary_hash,
                download_url: Url::parse("http://127.0.0.1:1/unused").unwrap(),
            },
        )
        .unwrap();

        let running = manager.start_model(&model.id).await.expect("start model");
        assert_eq!(running.runtime_state, EngineRuntimeState::Running);
        assert_eq!(running.active_model_id.as_deref(), Some("test-model"));
        let port = running.port.expect("dynamic backend port");
        assert_eq!(
            reqwest::get(format!("http://127.0.0.1:{port}/v1/models"))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert!(gateway.has_backend());

        use axum::{body::Body, http::Request};
        use tower::ServiceExt;
        let response = crate::gateway_router(gateway.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header(
                        "authorization",
                        "Bearer hal100_test_0123456789abcdef",
                    )
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"model":"hal100-active","messages":[{"role":"user","content":"ping"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        for _ in 0..50 {
            if database.usage_request_count().unwrap() == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(database.usage_request_count().unwrap(), 1);
        let stopped = manager.stop().await.expect("stop model");
        assert_eq!(stopped.runtime_state, EngineRuntimeState::Stopped);
        assert!(!gateway.has_backend());

        let force_running = manager
            .force_start_model(&model.id)
            .await
            .expect("force start model");
        assert_eq!(force_running.runtime_state, EngineRuntimeState::Running);
        assert!(gateway.has_backend());
        let force_stopped = manager.force_stop().await.expect("force stop model");
        assert_eq!(force_stopped.runtime_state, EngineRuntimeState::Stopped);
        assert!(!gateway.has_backend());

        let mut tampered = std::fs::read(&model_path).unwrap();
        *tampered.last_mut().expect("model payload") ^= 0x01;
        std::fs::write(&model_path, tampered).unwrap();
        assert!(matches!(
            manager.verify_model(&model.id, &model_path).await,
            Err(EngineManagerError::ModelVerificationFailed)
        ));
        assert_eq!(
            database.local_model(&model.id).unwrap().unwrap().state,
            LocalModelState::VerificationFailed
        );
    }

    fn test_archive() -> Vec<u8> {
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut archive = tar::Builder::new(encoder);
        let body = b"#!/bin/sh\nexit 0\n";
        let mut header = tar::Header::new_gnu();
        header.set_path("llama-test-build/llama-server").unwrap();
        header.set_size(body.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive.append(&header, body.as_slice()).unwrap();
        let encoder = archive.into_inner().unwrap();
        encoder.finish().unwrap()
    }

    fn gguf_payload() -> Vec<u8> {
        let mut output = Vec::new();
        output.extend_from_slice(b"GGUF");
        output.extend_from_slice(&3_u32.to_le_bytes());
        output.extend_from_slice(&42_u64.to_le_bytes());
        output.extend_from_slice(&7_u64.to_le_bytes());
        output.extend_from_slice(b"runtime model");
        output
    }
}
