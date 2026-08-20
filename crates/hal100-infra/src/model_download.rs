use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures_util::StreamExt;
use hal100_protocol::{
    DownloadSource, LocalModelState, LocalModelSummary, ModelDownloadPlan, ModelDownloadSnapshot,
    ModelDownloadState, ModelOwnership, ModelSource,
};
use reqwest::{Client, StatusCode, Url, header, redirect::Policy};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use uuid::Uuid;

use crate::{
    Database, DatabaseError, DownloadRecord, RemoteModelCatalog, RemoteModelCatalogError,
    model_import::{display_name_from_file_name, validate_downloaded_gguf},
};

const DOWNLOAD_PLAN_TTL_MS: i64 = 5 * 60 * 1_000;
const STORAGE_RESERVE_BYTES: u64 = 512 * 1024 * 1024;
const PROGRESS_PERSIST_INTERVAL_BYTES: u64 = 4 * 1024 * 1024;
const MAX_PENDING_PLANS: usize = 16;

#[derive(Debug, Error)]
pub enum ModelDownloadError {
    #[error(transparent)]
    Catalog(#[from] RemoteModelCatalogError),
    #[error(transparent)]
    Database(#[from] DatabaseError),
    #[error("该仓库需要授权，HAL100 Alpha 暂只下载公开模型")]
    AuthorizationRequired,
    #[error("所选 GGUF 文件已不存在，请重新打开仓库")]
    FileNotFound,
    #[error("该文件没有可信 SHA-256，HAL100 不会在无法校验时安装")]
    MissingHash,
    #[error("模型下载至少需要 {required_bytes} 字节可用空间，当前只有 {available_bytes} 字节")]
    InsufficientStorage {
        required_bytes: u64,
        available_bytes: u64,
    },
    #[error("下载确认计划不存在或已被使用")]
    PlanNotFound,
    #[error("下载确认计划已过期，请重新检查模型文件")]
    PlanExpired,
    #[error("该模型文件已经下载过或存在未完成任务，请使用原任务继续")]
    DuplicateDownload,
    #[error("下载任务不存在")]
    DownloadNotFound,
    #[error("当前下载状态不能继续")]
    NotResumable,
    #[error("下载任务正在运行")]
    AlreadyRunning,
    #[error("远端文件在恢复前发生了变化，请重新创建下载")]
    RemoteChanged,
    #[error("模型下载端点配置无效")]
    InvalidEndpoint,
    #[error("模型存储路径无法安全表示")]
    InvalidStoragePath,
    #[error("下载路径超出 HAL100 托管目录")]
    UnsafeManagedPath,
    #[error("目标模型文件已经存在，HAL100 不会覆盖")]
    DestinationExists,
    #[error("模型下载请求失败：{0}")]
    Network(String),
    #[error("模型源返回 HTTP {0}")]
    UpstreamStatus(u16),
    #[error("模型源没有按要求返回断点数据")]
    InvalidRangeResponse,
    #[error("下载数据超过远端声明的模型大小")]
    ResponseTooLarge,
    #[error("下载未完成，实际收到 {actual_bytes} 字节，预期 {expected_bytes} 字节")]
    IncompleteDownload {
        actual_bytes: u64,
        expected_bytes: u64,
    },
    #[error("模型 SHA-256 校验失败")]
    HashMismatch,
    #[error("下载内容不是受支持的 GGUF 文件")]
    InvalidGguf,
    #[error("模型文件操作失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("模型下载状态锁已损坏")]
    LockPoisoned,
    #[error("下载已取消")]
    Cancelled,
    #[error("下载工作线程异常结束")]
    WorkerFailed,
}

impl ModelDownloadError {
    /// Stable, non-sensitive category suitable for UI, audit, and Agent tool failures.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::AuthorizationRequired => "authorization_required",
            Self::FileNotFound => "remote_file_not_found",
            Self::MissingHash => "missing_sha256",
            Self::InsufficientStorage { .. } => "insufficient_storage",
            Self::PlanNotFound => "plan_not_found",
            Self::PlanExpired => "plan_expired",
            Self::DuplicateDownload => "duplicate_download",
            Self::DownloadNotFound => "download_not_found",
            Self::NotResumable => "not_resumable",
            Self::AlreadyRunning => "already_running",
            Self::RemoteChanged => "remote_changed",
            Self::InvalidEndpoint => "invalid_endpoint",
            Self::InvalidStoragePath => "invalid_storage_path",
            Self::UnsafeManagedPath => "unsafe_managed_path",
            Self::DestinationExists => "destination_exists",
            Self::Network(_) => "network_error",
            Self::UpstreamStatus(_) => "upstream_status",
            Self::InvalidRangeResponse => "invalid_range_response",
            Self::ResponseTooLarge => "response_too_large",
            Self::IncompleteDownload { .. } => "incomplete_download",
            Self::HashMismatch => "hash_mismatch",
            Self::InvalidGguf => "invalid_gguf",
            Self::Io(_) => "io_error",
            Self::Database(_) => "database_error",
            Self::Catalog(error) => error.code(),
            Self::LockPoisoned => "state_lock_error",
            Self::Cancelled => "cancelled",
            Self::WorkerFailed => "worker_failed",
        }
    }
}

pub struct ModelDownloadManager {
    database: Arc<Database>,
    catalog: Arc<RemoteModelCatalog>,
    storage_root: PathBuf,
    client: Client,
    endpoints: DownloadEndpoints,
    pending: Mutex<HashMap<String, PendingDownload>>,
    active: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

#[derive(Clone)]
struct DownloadEndpoints {
    hugging_face_root: Url,
    model_scope_legacy_api: Url,
}

#[derive(Clone)]
struct PendingDownload {
    plan: ModelDownloadPlan,
    expected_sha256: [u8; 32],
    temporary_path: PathBuf,
    destination_path: PathBuf,
}

struct ResolvedDownload {
    record: DownloadRecord,
    display_name: String,
    license: Option<String>,
    quantization: Option<String>,
}

impl ModelDownloadManager {
    pub fn new(
        database: Arc<Database>,
        catalog: Arc<RemoteModelCatalog>,
        storage_root: PathBuf,
    ) -> Result<Self, ModelDownloadError> {
        Self::with_download_endpoints(
            database,
            catalog,
            storage_root,
            "https://huggingface.co/",
            "https://modelscope.cn/api/v1/",
        )
    }

    /// Constructs the manager against explicit download endpoints for deterministic adapters and
    /// tests. Production composition continues to use [`Self::new`].
    pub fn with_download_endpoints(
        database: Arc<Database>,
        catalog: Arc<RemoteModelCatalog>,
        storage_root: PathBuf,
        hugging_face_root: &str,
        model_scope_legacy_api: &str,
    ) -> Result<Self, ModelDownloadError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30))
            .redirect(Policy::limited(5))
            .user_agent("HAL100/0.0.1-dev model-download")
            .build()
            .map_err(|error| ModelDownloadError::Network(network_error(&error)))?;
        let endpoints = DownloadEndpoints {
            hugging_face_root: parse_base_url(hugging_face_root)?,
            model_scope_legacy_api: parse_base_url(model_scope_legacy_api)?,
        };
        database.pause_interrupted_downloads(now_ms())?;
        Ok(Self {
            database,
            catalog,
            storage_root,
            client,
            endpoints,
            pending: Mutex::new(HashMap::new()),
            active: Mutex::new(HashMap::new()),
        })
    }

    pub async fn plan_download(
        &self,
        source: DownloadSource,
        repository: &str,
        remote_path: &str,
        available_storage_bytes: u64,
    ) -> Result<ModelDownloadPlan, ModelDownloadError> {
        let repository_detail = self.catalog.repository(source, repository).await?;
        if repository_detail.gated || repository_detail.private {
            return Err(ModelDownloadError::AuthorizationRequired);
        }
        let file = repository_detail
            .files
            .into_iter()
            .find(|file| file.path == remote_path)
            .ok_or(ModelDownloadError::FileNotFound)?;
        let expected_sha256 = file
            .sha256
            .as_deref()
            .and_then(decode_sha256)
            .ok_or(ModelDownloadError::MissingHash)?;
        let required_storage_bytes = required_storage(file.size_bytes);
        ensure_storage(required_storage_bytes, available_storage_bytes)?;

        let plan_id = Uuid::new_v4().to_string();
        let file_name = remote_file_name(&file.path)?;
        let destination_path =
            self.destination_path(source, repository, &expected_sha256, file_name);
        let temporary_path = self
            .storage_root
            .join(".downloads")
            .join(format!("{plan_id}.gguf"));
        if destination_path.exists()
            || self.database.model_path_is_indexed(&destination_path)?
            || self.database.downloads()?.iter().any(|download| {
                Path::new(&download.destination_path) == destination_path
                    && !matches!(
                        download.state,
                        ModelDownloadState::Failed | ModelDownloadState::Cancelled
                    )
            })
        {
            return Err(ModelDownloadError::DuplicateDownload);
        }
        let now = now_ms();
        let plan = ModelDownloadPlan {
            plan_id: plan_id.clone(),
            expires_at_ms: now.saturating_add(DOWNLOAD_PLAN_TTL_MS),
            source,
            repository: repository.to_owned(),
            display_name: repository_detail.display_name,
            license: repository_detail.license,
            file,
            available_storage_bytes,
            required_storage_bytes,
            action_summary: "下载到 HAL100 托管目录，完成 SHA-256 与 GGUF 校验后原子安装"
                .to_owned(),
            requires_confirmation: true,
        };
        let pending_download = PendingDownload {
            plan: plan.clone(),
            expected_sha256,
            temporary_path,
            destination_path,
        };
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| ModelDownloadError::LockPoisoned)?;
        pending.retain(|_, item| item.plan.expires_at_ms >= now);
        if pending
            .values()
            .any(|item| item.destination_path == pending_download.destination_path)
        {
            return Err(ModelDownloadError::DuplicateDownload);
        }
        if pending.len() >= MAX_PENDING_PLANS
            && let Some(oldest) = pending
                .iter()
                .min_by_key(|(_, item)| item.plan.expires_at_ms)
                .map(|(id, _)| id.clone())
        {
            pending.remove(&oldest);
        }
        pending.insert(plan_id, pending_download);
        Ok(plan)
    }

    pub fn start_download(
        self: &Arc<Self>,
        plan_id: &str,
        available_storage_bytes: u64,
    ) -> Result<ModelDownloadSnapshot, ModelDownloadError> {
        let pending = {
            let mut plans = self
                .pending
                .lock()
                .map_err(|_| ModelDownloadError::LockPoisoned)?;
            plans
                .remove(plan_id)
                .ok_or(ModelDownloadError::PlanNotFound)?
        };
        if now_ms() > pending.plan.expires_at_ms {
            return Err(ModelDownloadError::PlanExpired);
        }
        ensure_storage(pending.plan.required_storage_bytes, available_storage_bytes)?;
        self.validate_managed_paths(&pending.temporary_path, &pending.destination_path)?;
        if pending.destination_path.exists()
            || self
                .database
                .model_path_is_indexed(&pending.destination_path)?
            || self.database.downloads()?.iter().any(|download| {
                Path::new(&download.destination_path) == pending.destination_path
                    && !matches!(
                        download.state,
                        ModelDownloadState::Failed | ModelDownloadState::Cancelled
                    )
            })
        {
            return Err(ModelDownloadError::DuplicateDownload);
        }
        let temporary_path = path_string(&pending.temporary_path)?;
        let destination_path = path_string(&pending.destination_path)?;
        let now = now_ms();
        let record = DownloadRecord {
            id: Uuid::new_v4().to_string(),
            source: pending.plan.source,
            repository: pending.plan.repository.clone(),
            revision: pending.plan.file.revision.clone(),
            file_name: pending.plan.file.path.clone(),
            state: ModelDownloadState::Pending,
            expected_size_bytes: pending.plan.file.size_bytes,
            downloaded_bytes: 0,
            expected_sha256: pending.expected_sha256,
            temporary_path,
            destination_path,
            error_code: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        self.database.create_download(&record)?;
        let resolved = ResolvedDownload {
            record: record.clone(),
            display_name: pending.plan.display_name,
            license: pending.plan.license,
            quantization: pending.plan.file.quantization,
        };
        self.spawn_download(resolved)?;
        Ok(snapshot_from_record(record))
    }

    /// Discards one unconsumed confirmation plan without touching files or download records.
    pub fn discard_plan(&self, plan_id: &str) -> Result<bool, ModelDownloadError> {
        let mut plans = self
            .pending
            .lock()
            .map_err(|_| ModelDownloadError::LockPoisoned)?;
        Ok(plans.remove(plan_id).is_some())
    }

    pub async fn resume_download(
        self: &Arc<Self>,
        download_id: &str,
        available_storage_bytes: u64,
    ) -> Result<ModelDownloadSnapshot, ModelDownloadError> {
        let record = self
            .database
            .download(download_id)?
            .ok_or(ModelDownloadError::DownloadNotFound)?;
        if !matches!(
            record.state,
            ModelDownloadState::Paused | ModelDownloadState::Failed | ModelDownloadState::Cancelled
        ) {
            return Err(ModelDownloadError::NotResumable);
        }
        if self
            .active
            .lock()
            .map_err(|_| ModelDownloadError::LockPoisoned)?
            .contains_key(download_id)
        {
            return Err(ModelDownloadError::AlreadyRunning);
        }
        self.validate_managed_paths(
            Path::new(&record.temporary_path),
            Path::new(&record.destination_path),
        )?;
        let restart_from_zero = matches!(
            record.error_code.as_deref(),
            Some("hash_mismatch" | "invalid_gguf" | "response_too_large")
        );
        let partial_size = existing_file_size(Path::new(&record.temporary_path))?;
        let remaining = if restart_from_zero {
            record.expected_size_bytes
        } else {
            record.expected_size_bytes.saturating_sub(partial_size)
        };
        ensure_storage(required_storage(remaining), available_storage_bytes)?;
        let repository = self
            .catalog
            .repository(record.source, &record.repository)
            .await?;
        if repository.gated || repository.private {
            return Err(ModelDownloadError::AuthorizationRequired);
        }
        let file = repository
            .files
            .iter()
            .find(|file| file.path == record.file_name)
            .ok_or(ModelDownloadError::RemoteChanged)?;
        if file.revision != record.revision
            || file.size_bytes != record.expected_size_bytes
            || file.sha256.as_deref().and_then(decode_sha256) != Some(record.expected_sha256)
        {
            return Err(ModelDownloadError::RemoteChanged);
        }
        if restart_from_zero && partial_size > 0 {
            std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&record.temporary_path)?;
        }
        self.database.update_download(
            &record.id,
            ModelDownloadState::Pending,
            if restart_from_zero { 0 } else { partial_size },
            None,
            now_ms(),
        )?;
        let resolved = ResolvedDownload {
            record: DownloadRecord {
                state: ModelDownloadState::Pending,
                error_code: None,
                ..record.clone()
            },
            display_name: repository.display_name,
            license: repository.license,
            quantization: file.quantization.clone(),
        };
        self.spawn_download(resolved)?;
        Ok(snapshot_from_record(record))
    }

    pub fn download(&self, download_id: &str) -> Result<ModelDownloadSnapshot, ModelDownloadError> {
        self.database
            .download(download_id)?
            .map(snapshot_from_record)
            .ok_or(ModelDownloadError::DownloadNotFound)
    }

    pub fn downloads(&self) -> Result<Vec<ModelDownloadSnapshot>, ModelDownloadError> {
        Ok(self
            .database
            .downloads()?
            .into_iter()
            .map(snapshot_from_record)
            .collect())
    }

    pub fn cancel_download(&self, download_id: &str) -> Result<(), ModelDownloadError> {
        let active = self
            .active
            .lock()
            .map_err(|_| ModelDownloadError::LockPoisoned)?;
        let cancel = active
            .get(download_id)
            .ok_or(ModelDownloadError::NotResumable)?;
        cancel.store(true, Ordering::Release);
        Ok(())
    }

    fn spawn_download(
        self: &Arc<Self>,
        resolved: ResolvedDownload,
    ) -> Result<(), ModelDownloadError> {
        let id = resolved.record.id.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut active = self
                .active
                .lock()
                .map_err(|_| ModelDownloadError::LockPoisoned)?;
            if active.insert(id.clone(), cancel.clone()).is_some() {
                return Err(ModelDownloadError::AlreadyRunning);
            }
        }
        let manager = self.clone();
        tauri_async_spawn(async move {
            manager.run_download(resolved, cancel).await;
        });
        Ok(())
    }

    async fn run_download(self: Arc<Self>, resolved: ResolvedDownload, cancel: Arc<AtomicBool>) {
        let id = resolved.record.id.clone();
        if let Err(error) = self.transfer_and_install(&resolved, &cancel).await {
            let bytes = existing_file_size(Path::new(&resolved.record.temporary_path)).unwrap_or(0);
            let state = if matches!(error, ModelDownloadError::Cancelled) {
                ModelDownloadState::Cancelled
            } else {
                ModelDownloadState::Failed
            };
            if let Err(database_error) = self.database.update_download(
                &id,
                state,
                bytes.min(resolved.record.expected_size_bytes),
                Some(error.code()),
                now_ms(),
            ) {
                tracing::error!(
                    error_code = "download_state_persist_failed",
                    download_id = %id,
                    error = %database_error,
                    "download_state_persist_failed"
                );
            }
            tracing::warn!(
                error_code = error.code(),
                download_id = %id,
                "model_download_stopped"
            );
        }
        if let Ok(mut active) = self.active.lock() {
            active.remove(&id);
        }
    }

    async fn transfer_and_install(
        &self,
        resolved: &ResolvedDownload,
        cancel: &AtomicBool,
    ) -> Result<(), ModelDownloadError> {
        let record = &resolved.record;
        let temporary_path = Path::new(&record.temporary_path);
        let destination_path = Path::new(&record.destination_path);
        self.validate_managed_paths(temporary_path, destination_path)?;
        if destination_path.exists() {
            if temporary_path.exists() {
                return Err(ModelDownloadError::DestinationExists);
            }
            let hash_path = destination_path.to_owned();
            let actual_hash = tokio::task::spawn_blocking(move || sha256_file(&hash_path))
                .await
                .map_err(|_| ModelDownloadError::WorkerFailed)??;
            if actual_hash != record.expected_sha256 {
                return Err(ModelDownloadError::DestinationExists);
            }
            validate_downloaded_gguf(destination_path)
                .map_err(|_| ModelDownloadError::InvalidGguf)?;
            let model = managed_model(resolved)?;
            self.database.complete_model_download(
                &record.id,
                &model,
                &record.expected_sha256,
                now_ms(),
            )?;
            return Ok(());
        }
        let temporary_parent = temporary_path
            .parent()
            .ok_or(ModelDownloadError::UnsafeManagedPath)?;
        tokio::fs::create_dir_all(temporary_parent).await?;
        reject_symlink(temporary_path)?;

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(temporary_path)
            .await?;
        let mut downloaded_bytes = file.metadata().await?.len();
        if downloaded_bytes > record.expected_size_bytes {
            file.set_len(0).await?;
            downloaded_bytes = 0;
        }
        file.seek(std::io::SeekFrom::Start(downloaded_bytes))
            .await?;
        self.database.update_download(
            &record.id,
            ModelDownloadState::Downloading,
            downloaded_bytes,
            None,
            now_ms(),
        )?;

        if downloaded_bytes < record.expected_size_bytes {
            let url = self.download_url(record)?;
            let mut request = self.client.get(url);
            if downloaded_bytes > 0 {
                request = request.header(header::RANGE, format!("bytes={downloaded_bytes}-"));
            }
            let response = request
                .send()
                .await
                .map_err(|error| ModelDownloadError::Network(network_error(&error)))?;
            if downloaded_bytes > 0 && response.status() == StatusCode::OK {
                file.set_len(0).await?;
                file.seek(std::io::SeekFrom::Start(0)).await?;
                downloaded_bytes = 0;
            } else if downloaded_bytes > 0 {
                if response.status() != StatusCode::PARTIAL_CONTENT
                    || !valid_content_range(
                        response.headers(),
                        downloaded_bytes,
                        record.expected_size_bytes,
                    )
                {
                    return Err(ModelDownloadError::InvalidRangeResponse);
                }
            } else if response.status() == StatusCode::PARTIAL_CONTENT {
                if !valid_content_range(response.headers(), 0, record.expected_size_bytes) {
                    return Err(ModelDownloadError::InvalidRangeResponse);
                }
            } else if response.status() != StatusCode::OK {
                return Err(ModelDownloadError::UpstreamStatus(
                    response.status().as_u16(),
                ));
            }

            let mut persisted_at = downloaded_bytes;
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                if cancel.load(Ordering::Acquire) {
                    file.sync_data().await?;
                    return Err(ModelDownloadError::Cancelled);
                }
                let chunk =
                    chunk.map_err(|error| ModelDownloadError::Network(network_error(&error)))?;
                let next = downloaded_bytes
                    .checked_add(chunk.len() as u64)
                    .ok_or(ModelDownloadError::ResponseTooLarge)?;
                if next > record.expected_size_bytes {
                    return Err(ModelDownloadError::ResponseTooLarge);
                }
                file.write_all(&chunk).await?;
                downloaded_bytes = next;
                if downloaded_bytes.saturating_sub(persisted_at) >= PROGRESS_PERSIST_INTERVAL_BYTES
                {
                    self.database.update_download(
                        &record.id,
                        ModelDownloadState::Downloading,
                        downloaded_bytes,
                        None,
                        now_ms(),
                    )?;
                    persisted_at = downloaded_bytes;
                }
            }
        }
        if cancel.load(Ordering::Acquire) {
            file.sync_data().await?;
            return Err(ModelDownloadError::Cancelled);
        }
        if downloaded_bytes != record.expected_size_bytes {
            return Err(ModelDownloadError::IncompleteDownload {
                actual_bytes: downloaded_bytes,
                expected_bytes: record.expected_size_bytes,
            });
        }
        file.sync_all().await?;
        drop(file);

        self.database.update_download(
            &record.id,
            ModelDownloadState::Verifying,
            downloaded_bytes,
            None,
            now_ms(),
        )?;
        let hash_path = temporary_path.to_owned();
        let actual_hash = tokio::task::spawn_blocking(move || sha256_file(&hash_path))
            .await
            .map_err(|_| ModelDownloadError::WorkerFailed)??;
        if actual_hash != record.expected_sha256 {
            return Err(ModelDownloadError::HashMismatch);
        }
        validate_downloaded_gguf(temporary_path).map_err(|_| ModelDownloadError::InvalidGguf)?;
        self.database.update_download(
            &record.id,
            ModelDownloadState::Installing,
            downloaded_bytes,
            None,
            now_ms(),
        )?;
        let destination_parent = destination_path
            .parent()
            .ok_or(ModelDownloadError::UnsafeManagedPath)?;
        tokio::fs::create_dir_all(destination_parent).await?;
        reject_symlink(destination_path)?;
        if destination_path.exists() {
            return Err(ModelDownloadError::DestinationExists);
        }
        tokio::fs::rename(temporary_path, destination_path).await?;

        let model = managed_model(resolved)?;
        self.database.complete_model_download(
            &record.id,
            &model,
            &record.expected_sha256,
            now_ms(),
        )?;
        tracing::info!(download_id = %record.id, model_id = %model.id, "model_download_ready");
        Ok(())
    }

    fn destination_path(
        &self,
        source: DownloadSource,
        repository: &str,
        hash: &[u8; 32],
        file_name: &str,
    ) -> PathBuf {
        let source = match source {
            DownloadSource::HuggingFace => "hugging-face",
            DownloadSource::ModelScope => "modelscope",
        };
        let repository = repository.replace('/', "--");
        self.storage_root
            .join("managed")
            .join(source)
            .join(repository)
            .join(&encode_sha256(hash)[..16])
            .join(file_name)
    }

    fn validate_managed_paths(
        &self,
        temporary_path: &Path,
        destination_path: &Path,
    ) -> Result<(), ModelDownloadError> {
        if !temporary_path.starts_with(self.storage_root.join(".downloads"))
            || !destination_path.starts_with(self.storage_root.join("managed"))
        {
            return Err(ModelDownloadError::UnsafeManagedPath);
        }
        Ok(())
    }

    fn download_url(&self, record: &DownloadRecord) -> Result<Url, ModelDownloadError> {
        let (owner, name) = record
            .repository
            .split_once('/')
            .ok_or(ModelDownloadError::InvalidEndpoint)?;
        match record.source {
            DownloadSource::HuggingFace => {
                let mut url = self.endpoints.hugging_face_root.clone();
                let mut segments = url
                    .path_segments_mut()
                    .map_err(|_| ModelDownloadError::InvalidEndpoint)?;
                segments
                    .pop_if_empty()
                    .extend([owner, name, "resolve", &record.revision])
                    .extend(record.file_name.split('/'));
                drop(segments);
                Ok(url)
            }
            DownloadSource::ModelScope => {
                let mut url = self.endpoints.model_scope_legacy_api.clone();
                let mut segments = url
                    .path_segments_mut()
                    .map_err(|_| ModelDownloadError::InvalidEndpoint)?;
                segments
                    .pop_if_empty()
                    .extend(["models", owner, name, "repo"]);
                drop(segments);
                url.query_pairs_mut()
                    .append_pair("Revision", &record.revision)
                    .append_pair("FilePath", &record.file_name);
                Ok(url)
            }
        }
    }
}

fn parse_base_url(value: &str) -> Result<Url, ModelDownloadError> {
    let url = Url::parse(value).map_err(|_| ModelDownloadError::InvalidEndpoint)?;
    if !url.path().ends_with('/') {
        return Err(ModelDownloadError::InvalidEndpoint);
    }
    Ok(url)
}

fn required_storage(file_size: u64) -> u64 {
    file_size.saturating_add(STORAGE_RESERVE_BYTES)
}

fn ensure_storage(required: u64, available: u64) -> Result<(), ModelDownloadError> {
    if available < required {
        Err(ModelDownloadError::InsufficientStorage {
            required_bytes: required,
            available_bytes: available,
        })
    } else {
        Ok(())
    }
}

fn remote_file_name(path: &str) -> Result<&str, ModelDownloadError> {
    Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .ok_or(ModelDownloadError::FileNotFound)
}

fn path_string(path: &Path) -> Result<String, ModelDownloadError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or(ModelDownloadError::InvalidStoragePath)
}

fn reject_symlink(path: &Path) -> Result<(), ModelDownloadError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(ModelDownloadError::UnsafeManagedPath)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn existing_file_size(path: &Path) -> Result<u64, ModelDownloadError> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(metadata.len()),
        Ok(_) => Err(ModelDownloadError::UnsafeManagedPath),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error.into()),
    }
}

fn sha256_file(path: &Path) -> Result<[u8; 32], ModelDownloadError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().into())
}

fn decode_sha256(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut result = [0_u8; 32];
    for (index, output) in result.iter_mut().enumerate() {
        let start = index * 2;
        *output = u8::from_str_radix(&value[start..start + 2], 16).ok()?;
    }
    Some(result)
}

fn encode_sha256(value: &[u8; 32]) -> String {
    use std::fmt::Write;
    let mut output = String::with_capacity(64);
    for byte in value {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn valid_content_range(
    headers: &header::HeaderMap,
    expected_start: u64,
    expected_total: u64,
) -> bool {
    let Some(value) = headers
        .get(header::CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("bytes "))
    else {
        return false;
    };
    let Some((range, total)) = value.split_once('/') else {
        return false;
    };
    let Some((start, end)) = range.split_once('-') else {
        return false;
    };
    let (Ok(start), Ok(end), Ok(total)) = (
        start.parse::<u64>(),
        end.parse::<u64>(),
        total.parse::<u64>(),
    ) else {
        return false;
    };
    start == expected_start && start <= end && end < total && total == expected_total
}

fn snapshot_from_record(record: DownloadRecord) -> ModelDownloadSnapshot {
    let can_resume = matches!(
        record.state,
        ModelDownloadState::Paused | ModelDownloadState::Failed | ModelDownloadState::Cancelled
    );
    ModelDownloadSnapshot {
        download_id: record.id,
        source: record.source,
        repository: record.repository,
        file_name: record.file_name,
        state: record.state,
        downloaded_bytes: record.downloaded_bytes,
        expected_size_bytes: record.expected_size_bytes,
        error_code: record.error_code,
        can_resume,
        model: None,
    }
}

fn managed_model(resolved: &ResolvedDownload) -> Result<LocalModelSummary, ModelDownloadError> {
    let record = &resolved.record;
    let file_name = remote_file_name(&record.file_name)?.to_owned();
    let display_name = if resolved.display_name.trim().is_empty() {
        display_name_from_file_name(&file_name, resolved.quantization.as_deref())
    } else {
        resolved.display_name.clone()
    };
    Ok(LocalModelSummary {
        id: format!("managed-{}", encode_sha256(&record.expected_sha256)),
        display_name,
        format: "gguf".to_owned(),
        quantization: resolved.quantization.clone(),
        source: match record.source {
            DownloadSource::HuggingFace => ModelSource::HuggingFace,
            DownloadSource::ModelScope => ModelSource::ModelScope,
        },
        repository: Some(record.repository.clone()),
        revision: Some(record.revision.clone()),
        file_name,
        ownership: ModelOwnership::Managed,
        license: resolved.license.clone(),
        state: LocalModelState::Ready,
        path: record.destination_path.clone(),
        size_bytes: record.expected_size_bytes,
    })
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

fn tauri_async_spawn(task: impl Future<Output = ()> + Send + 'static) {
    tokio::spawn(task);
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        body::Body,
        http::{HeaderMap, HeaderValue, Response},
        routing::get,
    };
    use serde_json::{Value, json};
    use std::fs;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("hal100-download-{}", Uuid::new_v4()));
            fs::create_dir(&path).expect("test directory");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn storage_guard_rejects_a_download_before_any_file_or_database_mutation() {
        let required = required_storage(1_024);
        let error = ensure_storage(required, required - 1).expect_err("insufficient storage");
        assert!(matches!(
            error,
            ModelDownloadError::InsufficientStorage {
                required_bytes,
                available_bytes
            } if required_bytes == required && available_bytes == required - 1
        ));
    }

    #[tokio::test]
    async fn confirms_download_resumes_partial_and_atomically_indexes_model() {
        let payload = gguf_payload();
        let hash = Sha256::digest(&payload);
        let hash_hex = hash
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let app = Router::new()
            .route("/catalog/models/acme/model", get(hf_repository))
            .route(
                "/download/acme/model/resolve/revision/model-Q4_K_M.gguf",
                get(download),
            )
            .with_state((payload.clone(), hash_hex.clone()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let address = listener.local_addr().expect("address");
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let temp = TestDirectory::new();
        let database = Arc::new(Database::open(temp.0.join("db.sqlite")).expect("database"));
        let catalog = Arc::new(
            RemoteModelCatalog::with_endpoints(
                &format!("http://{address}/catalog/"),
                &format!("http://{address}/unused-ms-open/"),
                &format!("http://{address}/unused-ms-legacy/"),
            )
            .expect("catalog"),
        );
        let manager = Arc::new(
            ModelDownloadManager::with_download_endpoints(
                database.clone(),
                catalog,
                temp.0.join("models"),
                &format!("http://{address}/download/"),
                &format!("http://{address}/unused-ms-download/"),
            )
            .expect("manager"),
        );
        let discarded_plan = manager
            .plan_download(
                DownloadSource::HuggingFace,
                "acme/model",
                "model-Q4_K_M.gguf",
                u64::MAX,
            )
            .await
            .expect("plan");
        assert!(matches!(
            manager
                .plan_download(
                    DownloadSource::HuggingFace,
                    "acme/model",
                    "model-Q4_K_M.gguf",
                    u64::MAX,
                )
                .await,
            Err(ModelDownloadError::DuplicateDownload)
        ));
        assert!(
            manager
                .discard_plan(&discarded_plan.plan_id)
                .expect("discard pending plan")
        );
        assert!(
            !manager
                .discard_plan(&discarded_plan.plan_id)
                .expect("discard is exact and idempotent")
        );
        assert!(matches!(
            manager.start_download(&discarded_plan.plan_id, u64::MAX),
            Err(ModelDownloadError::PlanNotFound)
        ));
        let plan = manager
            .plan_download(
                DownloadSource::HuggingFace,
                "acme/model",
                "model-Q4_K_M.gguf",
                u64::MAX,
            )
            .await
            .expect("replacement plan");
        assert!(plan.requires_confirmation);
        assert_eq!(database.downloads().expect("downloads").len(), 0);
        let partial_path = manager
            .pending
            .lock()
            .unwrap()
            .get(&plan.plan_id)
            .unwrap()
            .temporary_path
            .clone();
        fs::create_dir_all(partial_path.parent().unwrap()).unwrap();
        fs::write(&partial_path, &payload[..16]).unwrap();
        let snapshot = manager
            .start_download(&plan.plan_id, u64::MAX)
            .expect("start");

        for _ in 0..100 {
            let current = manager.download(&snapshot.download_id).expect("snapshot");
            if current.state == ModelDownloadState::Ready {
                assert_eq!(current.downloaded_bytes, payload.len() as u64);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let completed = manager.download(&snapshot.download_id).expect("completed");
        assert_eq!(completed.state, ModelDownloadState::Ready);
        let models = database.local_models().expect("models");
        assert_eq!(models.len(), 1);
        assert!(Path::new(&models[0].path).is_file());
        assert_eq!(database.audit_event_count().expect("audit count"), 1);
        server.abort();

        async fn hf_repository(
            axum::extract::State((payload, hash)): axum::extract::State<(Vec<u8>, String)>,
        ) -> Json<Value> {
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

        async fn download(
            axum::extract::State((payload, _)): axum::extract::State<(Vec<u8>, String)>,
            headers: HeaderMap,
        ) -> Response<Body> {
            if let Some(range) = headers
                .get(header::RANGE)
                .and_then(|value| value.to_str().ok())
            {
                let start = range
                    .strip_prefix("bytes=")
                    .and_then(|value| value.strip_suffix('-'))
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(0);
                return Response::builder()
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(
                        header::CONTENT_RANGE,
                        HeaderValue::from_str(&format!(
                            "bytes {start}-{}/{}",
                            payload.len() - 1,
                            payload.len()
                        ))
                        .unwrap(),
                    )
                    .body(Body::from(payload[start..].to_vec()))
                    .unwrap();
            }
            Response::new(Body::from(payload))
        }
    }

    #[tokio::test]
    async fn rejects_missing_hash_before_creating_a_confirmation() {
        let app = Router::new().route(
            "/catalog/models/acme/no-hash",
            get(|| async {
                Json(json!({
                    "id": "acme/no-hash",
                    "sha": "revision",
                    "siblings": [{"rfilename": "model.gguf", "size": 32}]
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let temp = TestDirectory::new();
        let database = Arc::new(Database::open(temp.0.join("db.sqlite")).unwrap());
        let catalog = Arc::new(
            RemoteModelCatalog::with_endpoints(
                &format!("http://{address}/catalog/"),
                &format!("http://{address}/unused/"),
                &format!("http://{address}/unused/"),
            )
            .unwrap(),
        );
        let manager = ModelDownloadManager::with_download_endpoints(
            database.clone(),
            catalog,
            temp.0.join("models"),
            &format!("http://{address}/download/"),
            &format!("http://{address}/unused/"),
        )
        .unwrap();
        assert!(matches!(
            manager
                .plan_download(
                    DownloadSource::HuggingFace,
                    "acme/no-hash",
                    "model.gguf",
                    u64::MAX
                )
                .await,
            Err(ModelDownloadError::MissingHash)
        ));
        assert!(database.downloads().unwrap().is_empty());
        server.abort();
    }

    #[test]
    fn cancellation_is_scoped_to_an_active_download() {
        let temp = TestDirectory::new();
        let database = Arc::new(Database::open(temp.0.join("db.sqlite")).unwrap());
        let catalog = Arc::new(
            RemoteModelCatalog::with_endpoints(
                "http://127.0.0.1:1/catalog/",
                "http://127.0.0.1:1/ms-open/",
                "http://127.0.0.1:1/ms-legacy/",
            )
            .unwrap(),
        );
        let manager = ModelDownloadManager::with_download_endpoints(
            database,
            catalog,
            temp.0.join("models"),
            "http://127.0.0.1:1/download/",
            "http://127.0.0.1:1/ms-download/",
        )
        .unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        manager
            .active
            .lock()
            .unwrap()
            .insert("active".to_owned(), flag.clone());

        manager.cancel_download("active").expect("cancel active");
        assert!(flag.load(Ordering::Acquire));
        assert!(matches!(
            manager.cancel_download("unknown"),
            Err(ModelDownloadError::NotResumable)
        ));
    }

    fn gguf_payload() -> Vec<u8> {
        let mut output = Vec::new();
        output.extend_from_slice(b"GGUF");
        output.extend_from_slice(&3_u32.to_le_bytes());
        output.extend_from_slice(&42_u64.to_le_bytes());
        output.extend_from_slice(&7_u64.to_le_bytes());
        output.extend_from_slice(b"model payload");
        output
    }
}
