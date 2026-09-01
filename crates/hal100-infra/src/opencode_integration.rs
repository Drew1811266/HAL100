use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use hal100_core::OPENCODE_INTEGRATION;
use hal100_protocol::{
    ExternalAgentDisconnectPlan, ExternalAgentDisconnectResult, ExternalAgentManagedChange,
    ExternalAgentManagedChangeAction, OpenCodeApplyResult, OpenCodeConfigChange,
    OpenCodeConfigFormat, OpenCodeConfigPlan, OpenCodeDetection, OpenCodeIntegrationState,
    OpenCodeProjectDiagnosis,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    BoundedCommandRunner, ClientCredentialError, CredentialRegistry, Database, DatabaseError,
    ManagedIntegrationRecord, ManagedIntegrationResourceRecord, PendingPlanStore,
    StoredClientCredential, hash_client_key,
    jsonc_patch::{
        JsoncPatchError, hal100_provider, parse_jsonc, patch_hal100_provider,
        remove_hal100_provider,
    },
    stored_client_credential,
};

const PROVIDER_DISPLAY_NAME: &str = "HAL100 · 由 HAL100 管理";
const PLAN_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_CONFIG_BYTES: u64 = 2 * 1024 * 1024;
const MIN_TESTED_OPENCODE_VERSION: (u64, u64, u64) = (1, 17, 9);

#[derive(Debug, Clone)]
pub struct OpenCodePaths {
    pub home_directory: PathBuf,
    pub config_path: PathBuf,
    pub alternate_config_path: PathBuf,
    pub credential_path: PathBuf,
    pub binary_candidates: Vec<PathBuf>,
}

impl OpenCodePaths {
    pub fn for_macos(home_directory: &Path, app_data_directory: &Path) -> Self {
        let config_directory = home_directory.join(".config/opencode");
        let json = config_directory.join("opencode.json");
        let jsonc = config_directory.join("opencode.jsonc");
        let (config_path, alternate_config_path) = if json.exists() || !jsonc.exists() {
            (json, jsonc)
        } else {
            (jsonc, json)
        };
        Self {
            home_directory: home_directory.to_path_buf(),
            config_path,
            alternate_config_path,
            credential_path: app_data_directory
                .join("credentials")
                .join("opencode-gateway.key"),
            binary_candidates: vec![
                home_directory.join(".opencode/bin/opencode"),
                PathBuf::from("/opt/homebrew/bin/opencode"),
                PathBuf::from("/usr/local/bin/opencode"),
            ],
        }
    }
}

#[derive(Debug, Error)]
pub enum OpenCodeIntegrationError {
    #[error("OpenCode的JSON和JSONC全局配置同时存在，请先保留其中一个")]
    AmbiguousGlobalConfig,
    #[error("OpenCode配置超过2 MiB安全上限")]
    ConfigTooLarge,
    #[error("OpenCode配置路径不能是符号链接")]
    ConfigIsSymlink,
    #[error("OpenCode凭据路径不能是符号链接")]
    CredentialIsSymlink,
    #[error("OpenCode现有HAL100 Provider不属于本应用，已拒绝覆盖")]
    ProviderConflict,
    #[error("HAL100管理的OpenCode配置已被外部修改，请先检查差异")]
    ManagedProviderModified,
    #[error("配置计划不存在、已使用或已经过期")]
    InvalidPlan,
    #[error("确认后配置文件又发生了变化，请重新预览")]
    ConfigChangedAfterPreview,
    #[error("HAL100 OpenCode凭据文件已存在但没有对应安装记录")]
    UnownedCredentialFile,
    #[error("HAL100 OpenCode凭据文件无效")]
    InvalidCredentialFile,
    #[error("OpenCode尚未由HAL100配置，无可断开的受管接入")]
    NotConfigured,
    #[error("HAL100管理的OpenCode凭据已被外部修改，请先检查")]
    ManagedCredentialModified,
    #[error("写入后验证失败，已经恢复原配置")]
    VerificationFailed,
    #[error("文件操作失败: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Database(#[from] DatabaseError),
    #[error(transparent)]
    Credential(#[from] ClientCredentialError),
    #[error(transparent)]
    Jsonc(#[from] JsoncPatchError),
}

/// Dedicated adapter for OpenCode's JSON/JSONC configuration and CLI lifecycle.
///
/// The shared external-Agent registry owns stable identity and credential boundaries; this
/// adapter deliberately retains the OpenCode-specific parser, plan and rollback behavior.
pub struct OpenCodeIntegrationAdapter {
    database: Arc<Database>,
    credentials: CredentialRegistry,
    paths: OpenCodePaths,
    gateway_base_url: String,
    pending: PendingPlanStore<PendingPlan>,
    pending_disconnect: PendingPlanStore<PendingDisconnect>,
}

/// Compatibility name retained for existing desktop and Agent application boundaries.
pub type OpenCodeManager = OpenCodeIntegrationAdapter;

struct PendingPlan {
    config_path: PathBuf,
    original_digest: [u8; 32],
    original: Vec<u8>,
    config_existed: bool,
    patched: Vec<u8>,
    fragment: Value,
    fragment_hash: [u8; 32],
    plaintext_key: String,
    create_credential_file: bool,
    prior_integration: Option<ManagedIntegrationRecord>,
}

struct PendingDisconnect {
    config_path: PathBuf,
    original_digest: [u8; 32],
    original: Vec<u8>,
    patched: Vec<u8>,
    credential_digest: [u8; 32],
    plaintext_key: String,
    integration: ManagedIntegrationRecord,
    resources: Vec<ManagedIntegrationResourceRecord>,
    credential: StoredClientCredential,
}

impl OpenCodeIntegrationAdapter {
    pub fn new(
        database: Arc<Database>,
        credentials: CredentialRegistry,
        paths: OpenCodePaths,
    ) -> Self {
        Self::with_gateway_base_url(
            database,
            credentials,
            paths,
            "http://127.0.0.1:10100/v1".to_owned(),
        )
    }

    pub fn with_gateway_base_url(
        database: Arc<Database>,
        credentials: CredentialRegistry,
        paths: OpenCodePaths,
        gateway_base_url: String,
    ) -> Self {
        Self {
            database,
            credentials,
            paths,
            gateway_base_url,
            pending: PendingPlanStore::new(PLAN_TTL),
            pending_disconnect: PendingPlanStore::new(PLAN_TTL),
        }
    }

    pub fn detect(&self) -> Result<OpenCodeDetection, OpenCodeIntegrationError> {
        let mut warnings = Vec::new();
        if self.paths.alternate_config_path.exists() && self.paths.config_path.exists() {
            warnings.push("同时检测到opencode.json和opencode.jsonc，HAL100不会自动修改".to_owned());
        }
        if std::env::var_os("OPENCODE_CONFIG").is_some() {
            warnings.push("检测到OPENCODE_CONFIG；自定义配置可能覆盖全局Provider".to_owned());
        }

        let binary = find_opencode_binary(&self.paths.binary_candidates);
        let version = binary.as_ref().and_then(|path| opencode_version(path));
        if version
            .as_deref()
            .and_then(stable_version_triplet)
            .is_some_and(|version| version < MIN_TESTED_OPENCODE_VERSION)
        {
            warnings.push(
                "该OpenCode版本早于HAL100当前自动验收下限1.17.9；建议升级后再配置".to_owned(),
            );
        }
        let config_path = self.current_config_path();
        let config_exists = config_path.exists();
        let prior = self
            .database
            .managed_integration(OPENCODE_INTEGRATION.integration_id)?;
        let integration_state = if config_exists {
            match read_config(&config_path).and_then(|bytes| {
                let text = std::str::from_utf8(&bytes).map_err(|_| {
                    OpenCodeIntegrationError::Jsonc(JsoncPatchError::InvalidJson(
                        "configuration is not UTF-8".to_owned(),
                    ))
                })?;
                Ok(parse_jsonc(text)?)
            }) {
                Ok(value) => self.integration_state(&value, prior.as_ref(), &config_path),
                Err(error) => {
                    warnings.push(format!("全局配置无法安全解析：{error}"));
                    OpenCodeIntegrationState::Conflict
                }
            }
        } else if prior.is_some() {
            warnings.push("HAL100安装记录存在，但OpenCode全局配置已不存在".to_owned());
            OpenCodeIntegrationState::ModifiedOutsideHal100
        } else {
            OpenCodeIntegrationState::NotConfigured
        };
        if let Ok(bytes) = read_config(&config_path)
            && let Ok(text) = std::str::from_utf8(&bytes)
            && let Ok(value) = parse_jsonc(text)
        {
            add_provider_precedence_warnings(&value, &mut warnings);
        }

        Ok(OpenCodeDetection {
            installed: binary.is_some(),
            version,
            binary_path: binary.map(|path| display_path(&path, &self.paths.home_directory)),
            config_path: display_path(&config_path, &self.paths.home_directory),
            config_exists,
            config_format: if config_path.extension().is_some_and(|ext| ext == "jsonc") {
                OpenCodeConfigFormat::Jsonc
            } else {
                OpenCodeConfigFormat::Json
            },
            integration_state,
            warnings,
        })
    }

    pub fn plan_configuration(&self) -> Result<OpenCodeConfigPlan, OpenCodeIntegrationError> {
        if self.paths.config_path.exists() && self.paths.alternate_config_path.exists() {
            return Err(OpenCodeIntegrationError::AmbiguousGlobalConfig);
        }
        let config_path = self.current_config_path();
        reject_symlink(&config_path, true)?;
        reject_symlink(&self.paths.credential_path, false)?;

        let config_existed = config_path.exists();
        let original = if config_existed {
            read_config(&config_path)?
        } else {
            b"{\n  \"$schema\": \"https://opencode.ai/config.json\"\n}\n".to_vec()
        };
        let source = std::str::from_utf8(&original)
            .map_err(|_| JsoncPatchError::InvalidJson("configuration is not UTF-8".to_owned()))?;
        let parsed = parse_jsonc(source)?;
        let prior = self
            .database
            .managed_integration(OPENCODE_INTEGRATION.integration_id)?;
        let state = self.integration_state(&parsed, prior.as_ref(), &config_path);
        match state {
            OpenCodeIntegrationState::Conflict => {
                return Err(OpenCodeIntegrationError::ProviderConflict);
            }
            OpenCodeIntegrationState::ModifiedOutsideHal100 => {
                return Err(OpenCodeIntegrationError::ManagedProviderModified);
            }
            _ => {}
        }

        let (plaintext_key, create_credential_file) = if prior.is_some() {
            let key = read_credential(&self.paths.credential_path)?;
            (key, false)
        } else {
            if self.paths.credential_path.exists() {
                return Err(OpenCodeIntegrationError::UnownedCredentialFile);
            }
            (generate_client_key(), true)
        };
        let credential_reference =
            format!("{{file:{}}}", self.paths.credential_path.to_string_lossy());
        let fragment = provider_fragment(&credential_reference, &self.gateway_base_url);
        let fragment_hash = value_hash(&fragment);
        let patch = patch_hal100_provider(
            source,
            &fragment,
            state == OpenCodeIntegrationState::Configured,
        )?;
        let pending = PendingPlan {
            config_path: config_path.clone(),
            original_digest: bytes_hash(&original),
            original,
            config_existed,
            patched: patch.output.into_bytes(),
            fragment,
            fragment_hash,
            plaintext_key,
            create_credential_file,
            prior_integration: prior,
        };
        let ticket = self
            .pending
            .replace(pending)
            .map_err(|_| OpenCodeIntegrationError::InvalidPlan)?;
        tracing::info!(
            action = "configure_opencode",
            config_existed,
            requires_confirmation = true,
            "opencode_configuration_plan_created"
        );

        Ok(OpenCodeConfigPlan {
            plan_id: ticket.plan_id,
            expires_at_ms: ticket.expires_at_ms,
            config_path: display_path(&config_path, &self.paths.home_directory),
            credential_path: display_path(&self.paths.credential_path, &self.paths.home_directory),
            changes: vec![
                OpenCodeConfigChange {
                    path: "provider.hal100.npm".to_owned(),
                    value: "@ai-sdk/openai-compatible".to_owned(),
                },
                OpenCodeConfigChange {
                    path: "provider.hal100.options.baseURL".to_owned(),
                    value: self.gateway_base_url.clone(),
                },
                OpenCodeConfigChange {
                    path: "provider.hal100.options.apiKey".to_owned(),
                    value: "独立0600凭据文件（内容不显示）".to_owned(),
                },
                OpenCodeConfigChange {
                    path: "provider.hal100.models.hal100-active".to_owned(),
                    value: "HAL100 当前模型".to_owned(),
                },
            ],
            creates_backup: config_existed,
            preserves_default_model: true,
            requires_confirmation: true,
        })
    }

    pub fn diagnose_project(
        &self,
        project_directory: &Path,
    ) -> Result<OpenCodeProjectDiagnosis, OpenCodeIntegrationError> {
        if !project_directory.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "project path must be an existing directory",
            )
            .into());
        }
        let json = project_directory.join("opencode.json");
        let jsonc = project_directory.join("opencode.jsonc");
        let mut warnings = Vec::new();
        let config_path = match (json.exists(), jsonc.exists()) {
            (true, true) => {
                warnings.push(
                    "项目同时包含opencode.json和opencode.jsonc，无法确定最终覆盖来源".to_owned(),
                );
                None
            }
            (true, false) => Some(json),
            (false, true) => Some(jsonc),
            (false, false) => None,
        };
        let mut overrides_hal100_provider = false;
        let mut overrides_default_model = false;
        if let Some(path) = config_path.as_ref() {
            let bytes = read_config(path)?;
            let text = std::str::from_utf8(&bytes).map_err(|_| {
                JsoncPatchError::InvalidJson("configuration is not UTF-8".to_owned())
            })?;
            let config = parse_jsonc(text)?;
            overrides_hal100_provider = hal100_provider(&config).is_some();
            overrides_default_model = config.get("model").is_some();
            if overrides_hal100_provider {
                warnings.push("项目配置定义了provider.hal100，会覆盖全局接入配置".to_owned());
            }
            if overrides_default_model {
                warnings.push("项目配置定义了默认模型；HAL100不会修改该字段".to_owned());
            }
        }
        Ok(OpenCodeProjectDiagnosis {
            project_path: display_path(project_directory, &self.paths.home_directory),
            config_path: config_path
                .as_ref()
                .map(|path| display_path(path, &self.paths.home_directory)),
            overrides_hal100_provider,
            overrides_default_model,
            warnings,
        })
    }

    pub fn apply_configuration(
        &self,
        plan_id: &str,
    ) -> Result<OpenCodeApplyResult, OpenCodeIntegrationError> {
        self.apply_with_verifier(plan_id, |written, expected| {
            let parsed = parse_jsonc(written)?;
            (hal100_provider(&parsed) == Some(expected))
                .then_some(())
                .ok_or(OpenCodeIntegrationError::VerificationFailed)
        })
    }

    pub fn discard_configuration_plan(
        &self,
        plan_id: &str,
    ) -> Result<bool, OpenCodeIntegrationError> {
        self.pending
            .discard(plan_id)
            .map_err(|_| OpenCodeIntegrationError::InvalidPlan)
    }

    pub fn plan_disconnection(
        &self,
    ) -> Result<ExternalAgentDisconnectPlan, OpenCodeIntegrationError> {
        if self.paths.config_path.exists() && self.paths.alternate_config_path.exists() {
            return Err(OpenCodeIntegrationError::AmbiguousGlobalConfig);
        }
        let config_path = self.current_config_path();
        reject_symlink(&config_path, true)?;
        reject_symlink(&self.paths.credential_path, false)?;
        let integration = self
            .database
            .managed_integration(OPENCODE_INTEGRATION.integration_id)?
            .ok_or(OpenCodeIntegrationError::NotConfigured)?;
        if Path::new(&integration.config_path) != config_path
            || Path::new(&integration.credential_path) != self.paths.credential_path
        {
            return Err(OpenCodeIntegrationError::ManagedProviderModified);
        }
        let original = read_config(&config_path)?;
        let source = std::str::from_utf8(&original)
            .map_err(|_| JsoncPatchError::InvalidJson("configuration is not UTF-8".to_owned()))?;
        let parsed = parse_jsonc(source)?;
        if self.integration_state(&parsed, Some(&integration), &config_path)
            != OpenCodeIntegrationState::Configured
        {
            return Err(OpenCodeIntegrationError::ManagedProviderModified);
        }
        let plaintext_key = read_credential(&self.paths.credential_path)?;
        let credential = self
            .database
            .load_client_credentials()?
            .into_iter()
            .find(|credential| {
                credential.key_id == OPENCODE_INTEGRATION.credential_id
                    && credential.client_app_id == OPENCODE_INTEGRATION.client_app_id
            })
            .ok_or(OpenCodeIntegrationError::ManagedCredentialModified)?;
        if hash_client_key(&plaintext_key) != credential.key_hash {
            return Err(OpenCodeIntegrationError::ManagedCredentialModified);
        }
        let patched = remove_hal100_provider(source)?.output.into_bytes();
        let resources = self
            .database
            .managed_integration_resources(OPENCODE_INTEGRATION.integration_id)?;
        let pending = PendingDisconnect {
            config_path: config_path.clone(),
            original_digest: bytes_hash(&original),
            original,
            patched,
            credential_digest: credential.key_hash,
            plaintext_key,
            integration,
            resources,
            credential,
        };
        let ticket = self
            .pending_disconnect
            .replace(pending)
            .map_err(|_| OpenCodeIntegrationError::InvalidPlan)?;
        tracing::info!(
            action = "disconnect_opencode",
            requires_confirmation = true,
            "opencode_disconnection_plan_created"
        );

        Ok(ExternalAgentDisconnectPlan {
            plan_id: ticket.plan_id,
            integration_id: OPENCODE_INTEGRATION.integration_id.to_owned(),
            expires_at_ms: ticket.expires_at_ms,
            config_path: display_path(&config_path, &self.paths.home_directory),
            credential_path: display_path(&self.paths.credential_path, &self.paths.home_directory),
            changes: vec![
                ExternalAgentManagedChange {
                    path: "provider.hal100".to_owned(),
                    action: ExternalAgentManagedChangeAction::RemoveManagedFragment,
                },
                ExternalAgentManagedChange {
                    path: "opencode-gateway-key".to_owned(),
                    action: ExternalAgentManagedChangeAction::RemoveManagedCredential,
                },
            ],
            creates_backup: true,
            revokes_credential: true,
            requires_confirmation: true,
        })
    }

    pub fn discard_disconnection_plan(
        &self,
        plan_id: &str,
    ) -> Result<bool, OpenCodeIntegrationError> {
        self.pending_disconnect
            .discard(plan_id)
            .map_err(|_| OpenCodeIntegrationError::InvalidPlan)
    }

    pub fn apply_disconnection(
        &self,
        plan_id: &str,
    ) -> Result<ExternalAgentDisconnectResult, OpenCodeIntegrationError> {
        let pending = self
            .pending_disconnect
            .take(plan_id)
            .map_err(|_| OpenCodeIntegrationError::InvalidPlan)?;
        let current = read_config(&pending.config_path)?;
        if bytes_hash(&current) != pending.original_digest {
            return Err(OpenCodeIntegrationError::ConfigChangedAfterPreview);
        }
        let current_key = read_credential(&self.paths.credential_path)?;
        if hash_client_key(&current_key) != pending.credential_digest {
            return Err(OpenCodeIntegrationError::ConfigChangedAfterPreview);
        }
        let config_mode = existing_mode(&pending.config_path)?;
        let credential_mode = existing_mode(&self.paths.credential_path)?;
        let backup = backup_path(&pending.config_path);
        atomic_write(&backup, &pending.original, config_mode)?;
        if let Err(error) = atomic_write(&pending.config_path, &pending.patched, config_mode) {
            let _ = fs::remove_file(&backup);
            return Err(error);
        }
        if let Err(error) = fs::remove_file(&self.paths.credential_path) {
            rollback_disconnection_files(
                &pending,
                config_mode,
                credential_mode,
                &self.paths.credential_path,
                &backup,
            );
            return Err(error.into());
        }
        if let Some(parent) = self.paths.credential_path.parent() {
            let _ = sync_directory(parent);
        }
        let verified = std::str::from_utf8(&pending.patched)
            .ok()
            .and_then(|source| parse_jsonc(source).ok())
            .is_some_and(|config| hal100_provider(&config).is_none());
        if !verified {
            rollback_disconnection_files(
                &pending,
                config_mode,
                credential_mode,
                &self.paths.credential_path,
                &backup,
            );
            return Err(OpenCodeIntegrationError::VerificationFailed);
        }
        if let Err(error) = self
            .credentials
            .remove_client(OPENCODE_INTEGRATION.client_app_id)
        {
            rollback_disconnection_files(
                &pending,
                config_mode,
                credential_mode,
                &self.paths.credential_path,
                &backup,
            );
            return Err(error.into());
        }
        let database_result = self.database.remove_managed_integration_and_client(
            OPENCODE_INTEGRATION.integration_id,
            OPENCODE_INTEGRATION.client_app_id,
            now_ms(),
        );
        if !matches!(database_result.as_ref(), Ok(true)) {
            rollback_disconnection_files(
                &pending,
                config_mode,
                credential_mode,
                &self.paths.credential_path,
                &backup,
            );
            let _ = self.credentials.upsert(pending.credential.clone());
            let _ = self.database.upsert_integration_resources_and_credential(
                &pending.integration,
                &pending.resources,
                &pending.credential,
            );
            return match database_result {
                Ok(false) => Err(OpenCodeIntegrationError::NotConfigured),
                Err(error) => Err(error.into()),
                Ok(true) => unreachable!(),
            };
        }
        tracing::info!(
            action = "disconnect_opencode",
            result = "succeeded",
            "opencode_disconnected"
        );
        Ok(ExternalAgentDisconnectResult {
            disconnected: true,
            integration_id: OPENCODE_INTEGRATION.integration_id.to_owned(),
            config_path: display_path(&pending.config_path, &self.paths.home_directory),
            backup_path: Some(display_path(&backup, &self.paths.home_directory)),
            credential_revoked: true,
        })
    }

    fn apply_with_verifier(
        &self,
        plan_id: &str,
        verifier: impl FnOnce(&str, &Value) -> Result<(), OpenCodeIntegrationError>,
    ) -> Result<OpenCodeApplyResult, OpenCodeIntegrationError> {
        let pending = self
            .pending
            .take(plan_id)
            .map_err(|_| OpenCodeIntegrationError::InvalidPlan)?;
        if self.current_config_path() != pending.config_path {
            return Err(OpenCodeIntegrationError::ConfigChangedAfterPreview);
        }
        let current = if pending.config_path.exists() {
            read_config(&pending.config_path)?
        } else if pending.config_existed {
            return Err(OpenCodeIntegrationError::ConfigChangedAfterPreview);
        } else {
            pending.original.clone()
        };
        if bytes_hash(&current) != pending.original_digest {
            return Err(OpenCodeIntegrationError::ConfigChangedAfterPreview);
        }

        if let Some(parent) = pending.config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        if pending.create_credential_file
            && let Some(parent) = self.paths.credential_path.parent()
        {
            fs::create_dir_all(parent)?;
        }
        let backup_path = if pending.config_existed {
            let backup = backup_path(&pending.config_path);
            write_new_file(
                &backup,
                &pending.original,
                existing_mode(&pending.config_path)?,
            )?;
            Some(backup)
        } else {
            None
        };

        let mut credential_created = false;
        if pending.create_credential_file {
            atomic_write(
                &self.paths.credential_path,
                pending.plaintext_key.as_bytes(),
                0o600,
            )?;
            credential_created = true;
        }
        let config_mode = if pending.config_existed {
            existing_mode(&pending.config_path)?
        } else {
            0o600
        };
        if let Err(error) = atomic_write(&pending.config_path, &pending.patched, config_mode) {
            if credential_created {
                let _ = fs::remove_file(&self.paths.credential_path);
            }
            return Err(error);
        }

        let verify_result = std::str::from_utf8(&pending.patched)
            .map_err(|_| OpenCodeIntegrationError::VerificationFailed)
            .and_then(|written| verifier(written, &pending.fragment));
        if verify_result.is_err() {
            rollback_files(
                &pending.config_path,
                &pending.original,
                pending.config_existed,
                config_mode,
                &self.paths.credential_path,
                credential_created,
            );
            tracing::warn!(
                action = "configure_opencode",
                result = "rolled_back",
                error_code = "verification_failed",
                "opencode_configuration_apply_failed"
            );
            return Err(OpenCodeIntegrationError::VerificationFailed);
        }

        let now = now_ms();
        let credential = stored_client_credential(
            OPENCODE_INTEGRATION.credential_id,
            OPENCODE_INTEGRATION.client_app_id,
            OPENCODE_INTEGRATION.display_name,
            &pending.plaintext_key,
        )?;
        let integration = ManagedIntegrationRecord {
            id: OPENCODE_INTEGRATION.integration_id.to_owned(),
            kind: "opencode-global-provider".to_owned(),
            config_path: pending.config_path.to_string_lossy().into_owned(),
            credential_path: self.paths.credential_path.to_string_lossy().into_owned(),
            managed_fragment_hash: pending.fragment_hash,
            backup_path: backup_path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            created_at_ms: pending
                .prior_integration
                .as_ref()
                .map_or(now, |record| record.created_at_ms),
            updated_at_ms: now,
        };
        if let Err(error) = self
            .database
            .upsert_integration_and_credential(&integration, &credential)
        {
            rollback_files(
                &pending.config_path,
                &pending.original,
                pending.config_existed,
                config_mode,
                &self.paths.credential_path,
                credential_created,
            );
            tracing::warn!(
                action = "configure_opencode",
                result = "rolled_back",
                error_code = "database_commit_failed",
                "opencode_configuration_apply_failed"
            );
            return Err(error.into());
        }
        self.credentials.upsert(credential.clone())?;
        tracing::info!(
            action = "configure_opencode",
            result = "succeeded",
            backup_created = backup_path.is_some(),
            "opencode_configuration_applied"
        );

        Ok(OpenCodeApplyResult {
            configured: true,
            config_path: display_path(&pending.config_path, &self.paths.home_directory),
            backup_path: backup_path
                .as_ref()
                .map(|path| display_path(path, &self.paths.home_directory)),
            credential_prefix: credential.display_prefix,
        })
    }

    fn integration_state(
        &self,
        config: &Value,
        prior: Option<&ManagedIntegrationRecord>,
        config_path: &Path,
    ) -> OpenCodeIntegrationState {
        if prior.is_some_and(|record| {
            Path::new(&record.config_path) != config_path
                || Path::new(&record.credential_path) != self.paths.credential_path
        }) {
            return OpenCodeIntegrationState::ModifiedOutsideHal100;
        }
        integration_state(config, prior)
    }

    fn current_config_path(&self) -> PathBuf {
        if self.paths.config_path.exists() || !self.paths.alternate_config_path.exists() {
            self.paths.config_path.clone()
        } else {
            self.paths.alternate_config_path.clone()
        }
    }
}

fn provider_fragment(credential_reference: &str, gateway_base_url: &str) -> Value {
    json!({
        "npm": "@ai-sdk/openai-compatible",
        "name": PROVIDER_DISPLAY_NAME,
        "options": {
            "baseURL": gateway_base_url,
            "apiKey": credential_reference
        },
        "models": {
            "hal100-active": {
                "name": "HAL100 当前模型"
            }
        }
    })
}

fn integration_state(
    config: &Value,
    prior: Option<&ManagedIntegrationRecord>,
) -> OpenCodeIntegrationState {
    match (hal100_provider(config), prior) {
        (None, None) => OpenCodeIntegrationState::NotConfigured,
        (Some(_), None) => OpenCodeIntegrationState::Conflict,
        (None, Some(_)) => OpenCodeIntegrationState::ModifiedOutsideHal100,
        (Some(fragment), Some(record)) if value_hash(fragment) == record.managed_fragment_hash => {
            OpenCodeIntegrationState::Configured
        }
        (Some(_), Some(_)) => OpenCodeIntegrationState::ModifiedOutsideHal100,
    }
}

fn add_provider_precedence_warnings(config: &Value, warnings: &mut Vec<String>) {
    if config
        .get("disabled_providers")
        .and_then(Value::as_array)
        .is_some_and(|providers| providers.iter().any(|provider| provider == "hal100"))
    {
        warnings.push("disabled_providers包含hal100，Provider将不会加载".to_owned());
    }
    if let Some(enabled) = config.get("enabled_providers").and_then(Value::as_array)
        && !enabled.iter().any(|provider| provider == "hal100")
    {
        warnings.push("enabled_providers未包含hal100，Provider将不会加载".to_owned());
    }
}

fn read_config(path: &Path) -> Result<Vec<u8>, OpenCodeIntegrationError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(OpenCodeIntegrationError::ConfigIsSymlink);
    }
    if metadata.len() > MAX_CONFIG_BYTES {
        return Err(OpenCodeIntegrationError::ConfigTooLarge);
    }
    let file = File::open(path)?;
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    file.take(MAX_CONFIG_BYTES + 1).read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_CONFIG_BYTES {
        return Err(OpenCodeIntegrationError::ConfigTooLarge);
    }
    Ok(bytes)
}

fn read_credential(path: &Path) -> Result<String, OpenCodeIntegrationError> {
    reject_symlink(path, false)?;
    let bytes = fs::read(path)?;
    if !(24..=256).contains(&bytes.len()) {
        return Err(OpenCodeIntegrationError::InvalidCredentialFile);
    }
    let key =
        String::from_utf8(bytes).map_err(|_| OpenCodeIntegrationError::InvalidCredentialFile)?;
    if key.contains(['\n', '\r']) {
        return Err(OpenCodeIntegrationError::InvalidCredentialFile);
    }
    Ok(key)
}

fn reject_symlink(path: &Path, config: bool) -> Result<(), OpenCodeIntegrationError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if config {
                Err(OpenCodeIntegrationError::ConfigIsSymlink)
            } else {
                Err(OpenCodeIntegrationError::CredentialIsSymlink)
            }
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn generate_client_key() -> String {
    format!(
        "hal100_opencode_{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

fn bytes_hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn value_hash(value: &Value) -> [u8; 32] {
    bytes_hash(
        serde_json::to_string(value)
            .expect("JSON values always serialize")
            .as_bytes(),
    )
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}

fn backup_path(config_path: &Path) -> PathBuf {
    let file_name = config_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("opencode.json");
    config_path.with_file_name(format!("{file_name}.hal100-backup-{}", now_ms()))
}

fn atomic_write(target: &Path, contents: &[u8], mode: u32) -> Result<(), OpenCodeIntegrationError> {
    let parent = target.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "target has no parent")
    })?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".hal100-{}.tmp", Uuid::new_v4()));
    let result = (|| {
        write_new_file(&temporary, contents, mode)?;
        fs::rename(&temporary, target)?;
        sync_directory(parent)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn write_new_file(path: &Path, contents: &[u8], mode: u32) -> Result<(), OpenCodeIntegrationError> {
    #[cfg(not(unix))]
    let _ = mode;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    let mut file = options.open(path)?;
    file.write_all(contents)?;
    file.sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    Ok(())
}

fn existing_mode(path: &Path) -> Result<u32, OpenCodeIntegrationError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(fs::metadata(path)?.permissions().mode() & 0o777)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(0o600)
    }
}

fn sync_directory(path: &Path) -> Result<(), OpenCodeIntegrationError> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn rollback_files(
    config_path: &Path,
    original: &[u8],
    config_existed: bool,
    config_mode: u32,
    credential_path: &Path,
    credential_created: bool,
) {
    if config_existed {
        let _ = atomic_write(config_path, original, config_mode);
    } else {
        let _ = fs::remove_file(config_path);
    }
    if credential_created {
        let _ = fs::remove_file(credential_path);
    }
}

fn rollback_disconnection_files(
    pending: &PendingDisconnect,
    config_mode: u32,
    credential_mode: u32,
    credential_path: &Path,
    backup_path: &Path,
) {
    let _ = atomic_write(&pending.config_path, &pending.original, config_mode);
    if credential_path.exists() {
        let _ = atomic_write(
            credential_path,
            pending.plaintext_key.as_bytes(),
            credential_mode,
        );
    } else {
        let _ = write_new_file(
            credential_path,
            pending.plaintext_key.as_bytes(),
            credential_mode,
        );
    }
    let _ = fs::remove_file(backup_path);
    if let Some(parent) = backup_path.parent() {
        let _ = sync_directory(parent);
    }
}

fn display_path(path: &Path, home: &Path) -> String {
    path.strip_prefix(home).map_or_else(
        |_| path.to_string_lossy().into_owned(),
        |relative| format!("~/{}", relative.to_string_lossy()),
    )
}

fn find_opencode_binary(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|path| is_executable_file(path))
        .cloned()
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    true
}

fn opencode_version(binary: &Path) -> Option<String> {
    let version = BoundedCommandRunner::new(Duration::from_secs(2), 128)
        .run_utf8(binary, &["--version"])
        .ok()?;
    let version = version.trim();
    (!version.is_empty()).then(|| version.to_owned())
}

fn stable_version_triplet(version: &str) -> Option<(u64, u64, u64)> {
    let version = version.strip_prefix("opencode ").unwrap_or(version);
    let stable = version
        .split_once('-')
        .map_or(version, |(stable, _)| stable);
    let mut parts = stable.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    parts.next().is_none().then_some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("hal100-opencode-{}", Uuid::new_v4()));
            fs::create_dir(&path).expect("create test directory");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn manager(root: &Path) -> (OpenCodeManager, Arc<Database>, CredentialRegistry) {
        let database = Arc::new(Database::open(root.join("hal100.sqlite")).expect("database"));
        let credentials = CredentialRegistry::new(Vec::new());
        let paths = OpenCodePaths {
            home_directory: root.to_path_buf(),
            config_path: root.join(".config/opencode/opencode.json"),
            alternate_config_path: root.join(".config/opencode/opencode.jsonc"),
            credential_path: root.join("app-data/credentials/opencode-gateway.key"),
            binary_candidates: Vec::new(),
        };
        (
            OpenCodeManager::new(database.clone(), credentials.clone(), paths),
            database,
            credentials,
        )
    }

    #[test]
    fn parses_only_stable_opencode_versions_for_compatibility_warnings() {
        assert_eq!(stable_version_triplet("1.18.11"), Some((1, 18, 11)));
        assert_eq!(stable_version_triplet("opencode 1.17.9"), Some((1, 17, 9)));
        assert_eq!(stable_version_triplet("1.19.0-beta.1"), Some((1, 19, 0)));
        assert_eq!(stable_version_triplet("dev"), None);
        assert_eq!(stable_version_triplet("1.18"), None);
        assert!((1, 15, 10) < MIN_TESTED_OPENCODE_VERSION);
        assert!((1, 17, 9) >= MIN_TESTED_OPENCODE_VERSION);
    }

    #[test]
    fn detection_warns_when_opencode_is_older_than_the_tested_floor() {
        let temp = TestDirectory::new();
        let binary = temp.0.join("opencode");
        fs::write(&binary, "#!/bin/sh\nprintf '1.15.10\\n'\n").expect("fake OpenCode binary");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o700))
                .expect("fake OpenCode executable mode");
        }
        let database = Arc::new(Database::open(temp.0.join("hal100.sqlite")).expect("database"));
        let manager = OpenCodeManager::new(
            database,
            CredentialRegistry::new(Vec::new()),
            OpenCodePaths {
                home_directory: temp.0.clone(),
                config_path: temp.0.join(".config/opencode/opencode.json"),
                alternate_config_path: temp.0.join(".config/opencode/opencode.jsonc"),
                credential_path: temp.0.join("app-data/credentials/opencode-gateway.key"),
                binary_candidates: vec![binary],
            },
        );

        let detection = manager.detect().expect("OpenCode detection");
        assert_eq!(detection.version.as_deref(), Some("1.15.10"));
        assert!(
            detection
                .warnings
                .iter()
                .any(|warning| warning.contains("自动验收下限1.17.9"))
        );
    }

    #[test]
    fn confirmation_applies_backup_atomic_patch_and_client_attribution() {
        let temp = TestDirectory::new();
        let (manager, database, credentials) = manager(&temp.0);
        let config = temp.0.join(".config/opencode/opencode.json");
        fs::create_dir_all(config.parent().expect("config parent")).expect("config directory");
        let original = b"{\n  // user field must survive\n  \"model\": \"existing/default\",\n  \"mcp\": {\"keep\": true}\n}\n";
        fs::write(&config, original).expect("write existing config");

        let plan = manager.plan_configuration().expect("plan");
        assert!(plan.requires_confirmation);
        assert!(plan.creates_backup);
        assert!(plan.preserves_default_model);
        let result = manager
            .apply_configuration(&plan.plan_id)
            .expect("apply confirmed plan");

        let written = fs::read_to_string(&config).expect("read patched config");
        let parsed = parse_jsonc(&written).expect("parse patched config");
        assert!(written.contains("// user field must survive"));
        assert_eq!(parsed["model"], "existing/default");
        assert_eq!(parsed["mcp"]["keep"], true);
        assert_eq!(
            parsed["provider"]["hal100"]["options"]["baseURL"],
            "http://127.0.0.1:10100/v1"
        );
        assert!(result.backup_path.is_some());
        let backup = database
            .managed_integration(OPENCODE_INTEGRATION.integration_id)
            .expect("integration query")
            .expect("integration record")
            .backup_path
            .expect("backup path");
        assert_eq!(fs::read(backup).expect("backup bytes"), original);

        let key = fs::read_to_string(temp.0.join("app-data/credentials/opencode-gateway.key"))
            .expect("credential");
        assert_eq!(
            credentials
                .authenticate(&key)
                .expect("gateway authenticates OpenCode")
                .client_app_id,
            "opencode"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(temp.0.join("app-data/credentials/opencode-gateway.key"))
                .expect("credential metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn confirmed_disconnect_removes_only_owned_state_and_revokes_the_client() {
        let temp = TestDirectory::new();
        let (manager, database, credentials) = manager(&temp.0);
        let config = temp.0.join(".config/opencode/opencode.json");
        fs::create_dir_all(config.parent().expect("config parent")).expect("config directory");
        fs::write(
            &config,
            "{\n  // keep user configuration\n  \"model\": \"existing/default\",\n  \"provider\": {\"other\": {\"name\": \"keep\"}}\n}\n",
        )
        .expect("write config");
        let configure = manager.plan_configuration().expect("configure plan");
        manager
            .apply_configuration(&configure.plan_id)
            .expect("configure");
        let credential_path = temp.0.join("app-data/credentials/opencode-gateway.key");
        let plaintext_key = fs::read_to_string(&credential_path).expect("credential");
        assert!(credentials.authenticate(&plaintext_key).is_some());

        let disconnect = manager.plan_disconnection().expect("disconnect plan");
        assert!(disconnect.revokes_credential);
        assert_eq!(disconnect.changes.len(), 2);
        let result = manager
            .apply_disconnection(&disconnect.plan_id)
            .expect("disconnect");

        let written = fs::read_to_string(&config).expect("disconnected config");
        let parsed = parse_jsonc(&written).expect("parse disconnected config");
        assert_eq!(parsed["model"], "existing/default");
        assert_eq!(parsed["provider"]["other"]["name"], "keep");
        assert!(hal100_provider(&parsed).is_none());
        assert!(written.contains("// keep user configuration"));
        assert!(!credential_path.exists());
        assert!(credentials.authenticate(&plaintext_key).is_none());
        assert_eq!(
            database
                .managed_integration(OPENCODE_INTEGRATION.integration_id)
                .expect("integration query"),
            None
        );
        assert!(
            database
                .managed_integration_resources(OPENCODE_INTEGRATION.integration_id)
                .expect("resource query")
                .is_empty()
        );
        assert!(result.credential_revoked);
        assert!(result.backup_path.is_some());
    }

    #[test]
    fn discarded_and_stale_disconnect_plans_never_mutate_owned_state() {
        let temp = TestDirectory::new();
        let (manager, _, credentials) = manager(&temp.0);
        let config = temp.0.join(".config/opencode/opencode.json");
        fs::create_dir_all(config.parent().expect("config parent")).expect("config directory");
        fs::write(&config, "{\"user\":true}\n").expect("write config");
        let configure = manager.plan_configuration().expect("configure plan");
        manager
            .apply_configuration(&configure.plan_id)
            .expect("configure");
        let credential_path = temp.0.join("app-data/credentials/opencode-gateway.key");
        let key = fs::read_to_string(&credential_path).expect("credential");

        let discarded = manager.plan_disconnection().expect("discarded plan");
        assert!(
            manager
                .discard_disconnection_plan(&discarded.plan_id)
                .expect("discard")
        );
        assert!(matches!(
            manager.apply_disconnection(&discarded.plan_id),
            Err(OpenCodeIntegrationError::InvalidPlan)
        ));
        assert!(credentials.authenticate(&key).is_some());

        let stale = manager.plan_disconnection().expect("stale plan");
        let mut changed = fs::read_to_string(&config).expect("read config");
        changed.push_str("\n// changed after preview\n");
        fs::write(&config, &changed).expect("change config");
        assert!(matches!(
            manager.apply_disconnection(&stale.plan_id),
            Err(OpenCodeIntegrationError::ConfigChangedAfterPreview)
        ));
        assert_eq!(
            fs::read_to_string(&config).expect("preserved change"),
            changed
        );
        assert!(credential_path.exists());
        assert!(credentials.authenticate(&key).is_some());
    }

    #[test]
    fn stale_plan_never_overwrites_new_user_changes() {
        let temp = TestDirectory::new();
        let (manager, _, _) = manager(&temp.0);
        let config = temp.0.join(".config/opencode/opencode.json");
        fs::create_dir_all(config.parent().expect("config parent")).expect("config directory");
        fs::write(&config, "{\"mcp\":{}}\n").expect("config");
        let plan = manager.plan_configuration().expect("plan");
        fs::write(&config, "{\"mcp\":{},\"newUserField\":true}\n").expect("user edit");

        assert!(matches!(
            manager.apply_configuration(&plan.plan_id),
            Err(OpenCodeIntegrationError::ConfigChangedAfterPreview)
        ));
        assert!(
            fs::read_to_string(config)
                .expect("unchanged config")
                .contains("newUserField")
        );
        assert!(
            !temp
                .0
                .join("app-data/credentials/opencode-gateway.key")
                .exists()
        );
    }

    #[test]
    fn verification_failure_rolls_back_config_and_removes_new_key() {
        let temp = TestDirectory::new();
        let (manager, _, _) = manager(&temp.0);
        let config = temp.0.join(".config/opencode/opencode.json");
        fs::create_dir_all(config.parent().expect("config parent")).expect("config directory");
        let original = b"{\"unknown\":true}\n";
        fs::write(&config, original).expect("config");
        let plan = manager.plan_configuration().expect("plan");

        assert!(matches!(
            manager.apply_with_verifier(&plan.plan_id, |_, _| {
                Err(OpenCodeIntegrationError::VerificationFailed)
            }),
            Err(OpenCodeIntegrationError::VerificationFailed)
        ));
        assert_eq!(fs::read(config).expect("rolled back config"), original);
        assert!(
            !temp
                .0
                .join("app-data/credentials/opencode-gateway.key")
                .exists()
        );
    }

    #[test]
    fn existing_unowned_hal100_provider_is_never_replaced() {
        let temp = TestDirectory::new();
        let (manager, _, _) = manager(&temp.0);
        let config = temp.0.join(".config/opencode/opencode.json");
        fs::create_dir_all(config.parent().expect("config parent")).expect("config directory");
        let original = b"{\"provider\":{\"hal100\":{\"name\":\"user owned\"}}}\n";
        fs::write(&config, original).expect("config");

        assert!(matches!(
            manager.plan_configuration(),
            Err(OpenCodeIntegrationError::ProviderConflict)
        ));
        assert_eq!(fs::read(config).expect("unchanged config"), original);
    }

    #[test]
    fn jsonc_created_after_manager_start_is_selected_without_creating_json() {
        let temp = TestDirectory::new();
        let (manager, _, _) = manager(&temp.0);
        let json = temp.0.join(".config/opencode/opencode.json");
        let jsonc = temp.0.join(".config/opencode/opencode.jsonc");
        fs::create_dir_all(jsonc.parent().expect("config parent")).expect("config directory");
        fs::write(&jsonc, "{\n  // jsonc selected\n}\n").expect("jsonc config");

        let plan = manager.plan_configuration().expect("plan JSONC");
        assert!(plan.config_path.ends_with("opencode.jsonc"));
        manager
            .apply_configuration(&plan.plan_id)
            .expect("apply JSONC plan");

        assert!(!json.exists());
        assert!(
            fs::read_to_string(jsonc)
                .expect("JSONC result")
                .contains("// jsonc selected")
        );
    }

    #[test]
    fn project_config_is_diagnosed_but_never_modified() {
        let temp = TestDirectory::new();
        let (manager, _, _) = manager(&temp.0);
        let project = temp.0.join("project");
        fs::create_dir(&project).expect("project directory");
        let config = project.join("opencode.jsonc");
        let original = b"{\n  // project-owned\n  \"model\": \"other/model\",\n  \"provider\": {\"hal100\": {\"name\": \"project override\"}}\n}\n";
        fs::write(&config, original).expect("project config");

        let diagnosis = manager.diagnose_project(&project).expect("diagnosis");

        assert!(diagnosis.overrides_hal100_provider);
        assert!(diagnosis.overrides_default_model);
        assert_eq!(diagnosis.warnings.len(), 2);
        assert_eq!(
            fs::read(config).expect("project config unchanged"),
            original
        );
    }
}
