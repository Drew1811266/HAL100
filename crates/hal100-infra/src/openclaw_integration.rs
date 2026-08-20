use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use hal100_core::OPENCLAW_INTEGRATION;
use hal100_protocol::{
    ExternalAgentConfigurationChange, ExternalAgentConfigurationPlan,
    ExternalAgentConfigurationResult, ExternalAgentDetection, ExternalAgentDisconnectPlan,
    ExternalAgentDisconnectResult, ExternalAgentGatewayProtocol, ExternalAgentInputModality,
    ExternalAgentIntegrationState, ExternalAgentManagedChange, ExternalAgentManagedChangeAction,
    ExternalAgentModelProfile,
};
use serde_json::{Map, Value, json};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    BoundedCommandRunner, ClientCredentialError, CredentialRegistry, Database, DatabaseError,
    ExternalModelProfileRegistry, ManagedFileError, ManagedIntegrationRecord,
    ManagedIntegrationResourceRecord, ModelProfileError, PendingPlanStore, StoredClientCredential,
    atomic_write_managed_file, hash_client_key, managed_backup_path, managed_content_hash,
    managed_file_mode, read_managed_file, reject_managed_file_symlink, stored_client_credential,
    sync_managed_directory, write_new_managed_file,
};

const PLAN_TTL: Duration = Duration::from_secs(5 * 60);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_COMMAND_OUTPUT_BYTES: usize = 16 * 1024;
const MAX_CONFIG_BYTES: u64 = 8 * 1024 * 1024;
const MIN_TESTED_VERSION: (u64, u64, u64) = (2026, 7, 1);
const MODEL_PROVIDER_KEY: &str = "hal100";
const SECRET_PROVIDER_KEY: &str = "hal100_gateway";

#[derive(Debug, Clone)]
pub struct OpenClawPaths {
    pub home_directory: PathBuf,
    pub state_directory: PathBuf,
    pub config_path: PathBuf,
    pub credential_path: PathBuf,
    pub temporary_directory: PathBuf,
    pub binary_candidates: Vec<PathBuf>,
    /// Explicit, non-secret runtime directories used by npm-generated OpenClaw shims.
    pub runtime_directories: Vec<PathBuf>,
}

impl OpenClawPaths {
    pub fn for_macos(home_directory: &Path, app_data_directory: &Path) -> Self {
        let state_directory = home_directory.join(".openclaw");
        Self {
            home_directory: home_directory.to_path_buf(),
            config_path: state_directory.join("openclaw.json"),
            state_directory,
            credential_path: app_data_directory
                .join("credentials")
                .join("openclaw-gateway.key"),
            temporary_directory: app_data_directory.join("integration-plans/openclaw"),
            binary_candidates: vec![
                home_directory.join(".openclaw/bin/openclaw"),
                home_directory.join(".local/bin/openclaw"),
                home_directory.join(".npm-global/bin/openclaw"),
                home_directory.join(".bun/bin/openclaw"),
                PathBuf::from("/opt/homebrew/bin/openclaw"),
                PathBuf::from("/usr/local/bin/openclaw"),
            ],
            runtime_directories: node_runtime_directories(home_directory),
        }
    }
}

#[derive(Debug, Error)]
pub enum OpenClawIntegrationError {
    #[error("未检测到官方OpenClaw CLI，请先独立安装OpenClaw后再配置")]
    NotInstalled,
    #[error("OpenClaw版本低于HAL100当前验收下限2026.7.1，请先升级")]
    UnsupportedVersion,
    #[error("OpenClaw配置不是有效的JSON5对象: {0}")]
    InvalidJson5(String),
    #[error("当前OpenClaw版本不支持所选HAL100 Gateway协议")]
    UnsupportedProtocol,
    #[error("检测到OpenClaw自定义HOME、STATE、CONFIG或PROFILE；HAL100不会猜测并写入其他实例")]
    CustomInstanceRequiresExplicitConfiguration,
    #[error(
        "OpenClaw现有models.providers.hal100或secrets.providers.hal100_gateway不属于HAL100，已拒绝覆盖"
    )]
    ProviderConflict,
    #[error("HAL100管理的OpenClaw配置已被外部修改，请先检查差异")]
    ManagedProviderModified,
    #[error("HAL100管理的OpenClaw模型能力版本已变化，请重新预览配置")]
    ModelProfileChanged,
    #[error("配置计划不存在、已使用或已经过期")]
    InvalidPlan,
    #[error("确认后OpenClaw配置、CLI或凭据发生了变化，请重新预览")]
    ChangedAfterPreview,
    #[error("HAL100 OpenClaw凭据文件已存在但没有对应安装记录")]
    UnownedCredentialFile,
    #[error("HAL100 OpenClaw凭据文件无效或权限不安全")]
    InvalidCredentialFile,
    #[error("OpenClaw尚未由HAL100配置，无可断开的受管接入")]
    NotConfigured,
    #[error("HAL100管理的OpenClaw凭据已被外部修改，请先检查")]
    ManagedCredentialModified,
    #[error("OpenClaw官方配置工具验证或写入失败: {0}")]
    ConfigTool(String),
    #[error("写入后验证失败，已经恢复原配置")]
    VerificationFailed,
    #[error(transparent)]
    ManagedFile(#[from] ManagedFileError),
    #[error(transparent)]
    Database(#[from] DatabaseError),
    #[error(transparent)]
    Credential(#[from] ClientCredentialError),
    #[error(transparent)]
    ModelProfile(#[from] ModelProfileError),
    #[error("文件操作失败: {0}")]
    Io(#[from] std::io::Error),
}

trait OpenClawConfigTool: Send + Sync {
    fn version(&self, binary: &Path, environment: &[(String, OsString)]) -> Result<String, String>;

    fn patch(
        &self,
        binary: &Path,
        environment: &[(String, OsString)],
        config_path: &Path,
        patch_path: &Path,
        dry_run: bool,
    ) -> Result<(), String>;
}

#[derive(Debug, Default)]
struct OfficialOpenClawConfigTool;

impl OpenClawConfigTool for OfficialOpenClawConfigTool {
    fn version(&self, binary: &Path, environment: &[(String, OsString)]) -> Result<String, String> {
        BoundedCommandRunner::new(Duration::from_secs(3), 256)
            .run_utf8_with_env(binary, &["--version"], environment)
            .map(|output| output.trim().to_owned())
            .map_err(|error| error.to_string())
    }

    fn patch(
        &self,
        binary: &Path,
        environment: &[(String, OsString)],
        _config_path: &Path,
        patch_path: &Path,
        dry_run: bool,
    ) -> Result<(), String> {
        let patch_path = patch_path
            .to_str()
            .ok_or_else(|| "OpenClaw patch path is not UTF-8".to_owned())?;
        let mut args = vec!["config", "patch", "--file", patch_path];
        if dry_run {
            args.push("--dry-run");
            args.push("--json");
        }
        BoundedCommandRunner::new(COMMAND_TIMEOUT, MAX_COMMAND_OUTPUT_BYTES)
            .run_utf8_with_env(binary, &args, environment)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

pub struct OpenClawIntegrationAdapter {
    database: Arc<Database>,
    credentials: CredentialRegistry,
    model_profiles: ExternalModelProfileRegistry,
    paths: OpenClawPaths,
    gateway_base_url: String,
    config_tool: Arc<dyn OpenClawConfigTool>,
    pending: PendingPlanStore<PendingConfiguration>,
    pending_disconnect: PendingPlanStore<PendingDisconnect>,
}

struct PendingConfiguration {
    binary: PathBuf,
    original_digest: [u8; 32],
    original: Vec<u8>,
    config_existed: bool,
    patch: Value,
    secret_fragment: Value,
    provider_fragment: Value,
    fragment_hash: [u8; 32],
    plaintext_key: String,
    create_credential_file: bool,
    protocol: ExternalAgentGatewayProtocol,
    profile_revision: String,
    prior_integration: Option<ManagedIntegrationRecord>,
    prior_credential: Option<StoredClientCredential>,
}

struct PendingDisconnect {
    binary: PathBuf,
    original_digest: [u8; 32],
    original: Vec<u8>,
    patch: Value,
    credential_digest: [u8; 32],
    plaintext_key: String,
    integration: ManagedIntegrationRecord,
    resources: Vec<ManagedIntegrationResourceRecord>,
    credential: StoredClientCredential,
}

impl OpenClawIntegrationAdapter {
    pub fn new(
        database: Arc<Database>,
        credentials: CredentialRegistry,
        model_profiles: ExternalModelProfileRegistry,
        paths: OpenClawPaths,
    ) -> Self {
        Self::with_gateway_base_url(
            database,
            credentials,
            model_profiles,
            paths,
            "http://127.0.0.1:10100/v1".to_owned(),
        )
    }

    pub fn with_gateway_base_url(
        database: Arc<Database>,
        credentials: CredentialRegistry,
        model_profiles: ExternalModelProfileRegistry,
        paths: OpenClawPaths,
        gateway_base_url: String,
    ) -> Self {
        Self::with_config_tool(
            database,
            credentials,
            model_profiles,
            paths,
            gateway_base_url,
            Arc::new(OfficialOpenClawConfigTool),
        )
    }

    fn with_config_tool(
        database: Arc<Database>,
        credentials: CredentialRegistry,
        model_profiles: ExternalModelProfileRegistry,
        paths: OpenClawPaths,
        gateway_base_url: String,
        config_tool: Arc<dyn OpenClawConfigTool>,
    ) -> Self {
        Self {
            database,
            credentials,
            model_profiles,
            paths,
            gateway_base_url,
            config_tool,
            pending: PendingPlanStore::new(PLAN_TTL),
            pending_disconnect: PendingPlanStore::new(PLAN_TTL),
        }
    }

    pub fn detect(&self) -> Result<ExternalAgentDetection, OpenClawIntegrationError> {
        let profile = self.model_profiles.snapshot()?;
        let binary = find_binary(&self.paths.binary_candidates);
        let environment = self.command_environment();
        let version = binary
            .as_ref()
            .and_then(|path| self.config_tool.version(path, &environment).ok())
            .filter(|version| !version.is_empty());
        let version_supported = version
            .as_deref()
            .and_then(stable_version_triplet)
            .is_none_or(|version| version >= MIN_TESTED_VERSION);
        let prior = self
            .database
            .managed_integration(OPENCLAW_INTEGRATION.integration_id)?;
        let mut warnings = self.custom_instance_warnings();
        if binary.is_some() && version.is_none() {
            warnings.push("检测到OpenClaw CLI，但无法在受限环境中读取版本".to_owned());
        }
        if !version_supported {
            warnings.push("该OpenClaw版本早于HAL100当前自动验收下限2026.7.1".to_owned());
        }

        let config_exists = self.paths.config_path.exists();
        let (mut state, configured_protocol) = if config_exists {
            match self.read_config() {
                Ok((_, config)) => self.integration_status(&config, prior.as_ref(), &profile),
                Err(error) => {
                    warnings.push(format!("openclaw.json无法安全解析：{error}"));
                    (ExternalAgentIntegrationState::Conflict, None)
                }
            }
        } else if prior.is_some() {
            warnings.push("HAL100安装记录存在，但OpenClaw配置文件已不存在".to_owned());
            (ExternalAgentIntegrationState::ModifiedOutsideHal100, None)
        } else if binary.is_some() {
            (ExternalAgentIntegrationState::InstalledNotConfigured, None)
        } else {
            (ExternalAgentIntegrationState::NotInstalled, None)
        };
        if !version_supported && state == ExternalAgentIntegrationState::InstalledNotConfigured {
            state = ExternalAgentIntegrationState::UnsupportedVersion;
        }
        if !warnings.is_empty()
            && has_custom_instance_override()
            && state == ExternalAgentIntegrationState::InstalledNotConfigured
        {
            state = ExternalAgentIntegrationState::Blocked;
        }

        Ok(ExternalAgentDetection {
            integration_id: OPENCLAW_INTEGRATION.integration_id.to_owned(),
            display_name: OPENCLAW_INTEGRATION.display_name.to_owned(),
            installed: binary.is_some(),
            version,
            binary_path: binary.map(|path| display_path(&path, &self.paths.home_directory)),
            config_path: display_path(&self.paths.config_path, &self.paths.home_directory),
            config_exists,
            integration_state: state,
            configured_protocol,
            model_profile_revision: profile.revision,
            warnings,
        })
    }

    pub fn plan_configuration(
        &self,
        protocol: ExternalAgentGatewayProtocol,
    ) -> Result<ExternalAgentConfigurationPlan, OpenClawIntegrationError> {
        ensure_supported_protocol(protocol)?;
        self.ensure_default_instance()?;
        let binary = find_binary(&self.paths.binary_candidates)
            .ok_or(OpenClawIntegrationError::NotInstalled)?;
        self.ensure_supported_version(&binary)?;
        reject_managed_file_symlink(&self.paths.config_path)?;
        reject_managed_file_symlink(&self.paths.credential_path)?;
        let profile = self.model_profiles.snapshot()?;
        let config_existed = self.paths.config_path.exists();
        let original = if config_existed {
            read_managed_file(&self.paths.config_path, MAX_CONFIG_BYTES)?
        } else {
            b"{}\n".to_vec()
        };
        let config = parse_config(&original)?;
        let prior = self
            .database
            .managed_integration(OPENCLAW_INTEGRATION.integration_id)?;
        let (state, _) = self.integration_status(&config, prior.as_ref(), &profile);
        match state {
            ExternalAgentIntegrationState::Conflict => {
                return Err(OpenClawIntegrationError::ProviderConflict);
            }
            ExternalAgentIntegrationState::ModifiedOutsideHal100 => {
                return Err(OpenClawIntegrationError::ManagedProviderModified);
            }
            _ => {}
        }

        let prior_credential = self.stored_credential()?;
        let (plaintext_key, create_credential_file) = if prior.is_some() {
            let key = read_credential(&self.paths.credential_path)?;
            let credential = prior_credential
                .as_ref()
                .ok_or(OpenClawIntegrationError::ManagedCredentialModified)?;
            if hash_client_key(&key) != credential.key_hash {
                return Err(OpenClawIntegrationError::ManagedCredentialModified);
            }
            (key, false)
        } else {
            if self.paths.credential_path.exists() || prior_credential.is_some() {
                return Err(OpenClawIntegrationError::UnownedCredentialFile);
            }
            (generate_client_key(), true)
        };

        let secret_fragment = secret_provider_fragment(&self.paths.credential_path);
        let provider_fragment = model_provider_fragment(
            protocol,
            &self.gateway_base_url,
            &profile,
            &secret_reference(),
        );
        let patch = configuration_patch(&secret_fragment, &provider_fragment);
        self.validate_configuration_patch(
            &binary,
            &patch,
            create_credential_file.then_some(plaintext_key.as_str()),
            protocol,
            &profile,
        )?;
        let fragment_hash = combined_fragment_hash(&secret_fragment, &provider_fragment);
        let pending = PendingConfiguration {
            binary,
            original_digest: managed_content_hash(&original),
            original: original.clone(),
            config_existed,
            patch,
            secret_fragment,
            provider_fragment,
            fragment_hash,
            plaintext_key,
            create_credential_file,
            protocol,
            profile_revision: profile.revision.clone(),
            prior_integration: prior,
            prior_credential,
        };
        let ticket = self
            .pending
            .replace(pending)
            .map_err(|_| OpenClawIntegrationError::InvalidPlan)?;
        let mut warnings =
            vec!["HAL100不会修改OpenClaw默认模型，也不会启动、停止或重启OpenClaw服务".to_owned()];
        if serde_json::from_slice::<Value>(&original).is_err() {
            warnings.push(
                "OpenClaw官方配置工具会把JSON5标准化为JSON，原文件中的注释和排版可能变化"
                    .to_owned(),
            );
        }

        Ok(ExternalAgentConfigurationPlan {
            plan_id: ticket.plan_id,
            integration_id: OPENCLAW_INTEGRATION.integration_id.to_owned(),
            expires_at_ms: ticket.expires_at_ms,
            config_path: display_path(&self.paths.config_path, &self.paths.home_directory),
            credential_path: display_path(&self.paths.credential_path, &self.paths.home_directory),
            changes: vec![
                ExternalAgentConfigurationChange {
                    path: "secrets.providers.hal100_gateway".to_owned(),
                    value: "独立0600文件型SecretRef（内容不显示）".to_owned(),
                },
                ExternalAgentConfigurationChange {
                    path: "models.providers.hal100.baseUrl".to_owned(),
                    value: protocol_base_url(protocol, &self.gateway_base_url),
                },
                ExternalAgentConfigurationChange {
                    path: "models.providers.hal100.api".to_owned(),
                    value: protocol_api_name(protocol).to_owned(),
                },
                ExternalAgentConfigurationChange {
                    path: "models.providers.hal100.models[hal100-active]".to_owned(),
                    value: format!(
                        "{} · 上下文{} · 最大输出{}",
                        profile.display_name,
                        profile.context_window_tokens,
                        profile.max_output_tokens
                    ),
                },
            ],
            gateway_protocol: protocol,
            creates_backup: config_existed,
            preserves_default_model: true,
            requires_confirmation: true,
            model_profile_revision: profile.revision,
            warnings,
        })
    }

    pub fn discard_configuration_plan(
        &self,
        plan_id: &str,
    ) -> Result<bool, OpenClawIntegrationError> {
        self.pending
            .discard(plan_id)
            .map_err(|_| OpenClawIntegrationError::InvalidPlan)
    }

    pub fn apply_configuration(
        &self,
        plan_id: &str,
    ) -> Result<ExternalAgentConfigurationResult, OpenClawIntegrationError> {
        let pending = self
            .pending
            .take(plan_id)
            .map_err(|_| OpenClawIntegrationError::InvalidPlan)?;
        self.ensure_default_instance()?;
        if self.model_profiles.snapshot()?.revision != pending.profile_revision {
            return Err(OpenClawIntegrationError::ModelProfileChanged);
        }
        self.ensure_supported_version(&pending.binary)?;
        let current = self.current_config_bytes(pending.config_existed, &pending.original)?;
        if managed_content_hash(&current) != pending.original_digest {
            return Err(OpenClawIntegrationError::ChangedAfterPreview);
        }
        if !pending.create_credential_file {
            let current_key = read_credential(&self.paths.credential_path)?;
            if current_key != pending.plaintext_key {
                return Err(OpenClawIntegrationError::ChangedAfterPreview);
            }
        }

        let config_mode = if pending.config_existed {
            managed_file_mode(&self.paths.config_path)?
        } else {
            0o600
        };
        let backup = self.create_backup(&pending.original, pending.config_existed, config_mode)?;
        let mut credential_created = false;
        if pending.create_credential_file {
            atomic_write_managed_file(
                &self.paths.credential_path,
                pending.plaintext_key.as_bytes(),
                0o600,
            )?;
            credential_created = true;
        }
        if let Err(error) = self.run_patch(&pending.binary, &pending.patch, false) {
            rollback_configuration_files(
                self,
                &pending,
                config_mode,
                credential_created,
                backup.as_deref(),
            );
            return Err(error);
        }
        if !self.verify_fragments(&pending.secret_fragment, &pending.provider_fragment) {
            rollback_configuration_files(
                self,
                &pending,
                config_mode,
                credential_created,
                backup.as_deref(),
            );
            return Err(OpenClawIntegrationError::VerificationFailed);
        }

        let credential = stored_client_credential(
            OPENCLAW_INTEGRATION.credential_id,
            OPENCLAW_INTEGRATION.client_app_id,
            OPENCLAW_INTEGRATION.display_name,
            &pending.plaintext_key,
        )?;
        if let Err(error) = self.credentials.upsert(credential.clone()) {
            rollback_configuration_files(
                self,
                &pending,
                config_mode,
                credential_created,
                backup.as_deref(),
            );
            return Err(error.into());
        }
        let now = now_ms();
        let integration = ManagedIntegrationRecord {
            id: OPENCLAW_INTEGRATION.integration_id.to_owned(),
            kind: format!(
                "openclaw-model-provider:{}",
                protocol_api_name(pending.protocol)
            ),
            config_path: self.paths.config_path.to_string_lossy().into_owned(),
            credential_path: self.paths.credential_path.to_string_lossy().into_owned(),
            managed_fragment_hash: pending.fragment_hash,
            backup_path: backup
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
            rollback_registry(self, pending.prior_credential.clone());
            rollback_configuration_files(
                self,
                &pending,
                config_mode,
                credential_created,
                backup.as_deref(),
            );
            return Err(error.into());
        }

        tracing::info!(
            integration_id = OPENCLAW_INTEGRATION.integration_id,
            protocol = protocol_api_name(pending.protocol),
            action = "configure",
            result = "succeeded",
            model_profile_revision = %pending.profile_revision,
            "external_agent_configuration_applied"
        );
        Ok(ExternalAgentConfigurationResult {
            configured: true,
            integration_id: OPENCLAW_INTEGRATION.integration_id.to_owned(),
            config_path: display_path(&self.paths.config_path, &self.paths.home_directory),
            backup_path: backup
                .as_ref()
                .map(|path| display_path(path, &self.paths.home_directory)),
            credential_prefix: credential.display_prefix,
            model_profile_revision: pending.profile_revision,
        })
    }

    pub fn plan_disconnection(
        &self,
    ) -> Result<ExternalAgentDisconnectPlan, OpenClawIntegrationError> {
        self.ensure_default_instance()?;
        let binary = find_binary(&self.paths.binary_candidates)
            .ok_or(OpenClawIntegrationError::NotInstalled)?;
        self.ensure_supported_version(&binary)?;
        reject_managed_file_symlink(&self.paths.config_path)?;
        reject_managed_file_symlink(&self.paths.credential_path)?;
        let integration = self
            .database
            .managed_integration(OPENCLAW_INTEGRATION.integration_id)?
            .ok_or(OpenClawIntegrationError::NotConfigured)?;
        if Path::new(&integration.config_path) != self.paths.config_path
            || Path::new(&integration.credential_path) != self.paths.credential_path
        {
            return Err(OpenClawIntegrationError::ManagedProviderModified);
        }
        let original = read_managed_file(&self.paths.config_path, MAX_CONFIG_BYTES)?;
        let config = parse_config(&original)?;
        let (secret, provider) = managed_fragments(&config);
        let (Some(secret), Some(provider)) = (secret, provider) else {
            return Err(OpenClawIntegrationError::ManagedProviderModified);
        };
        if combined_fragment_hash(secret, provider) != integration.managed_fragment_hash {
            return Err(OpenClawIntegrationError::ManagedProviderModified);
        }
        let plaintext_key = read_credential(&self.paths.credential_path)?;
        let credential = self
            .stored_credential()?
            .ok_or(OpenClawIntegrationError::ManagedCredentialModified)?;
        if hash_client_key(&plaintext_key) != credential.key_hash {
            return Err(OpenClawIntegrationError::ManagedCredentialModified);
        }
        let patch = disconnection_patch();
        self.run_patch(&binary, &patch, true)?;
        let resources = self
            .database
            .managed_integration_resources(OPENCLAW_INTEGRATION.integration_id)?;
        let pending = PendingDisconnect {
            binary,
            original_digest: managed_content_hash(&original),
            original,
            patch,
            credential_digest: credential.key_hash,
            plaintext_key,
            integration,
            resources,
            credential,
        };
        let ticket = self
            .pending_disconnect
            .replace(pending)
            .map_err(|_| OpenClawIntegrationError::InvalidPlan)?;

        Ok(ExternalAgentDisconnectPlan {
            plan_id: ticket.plan_id,
            integration_id: OPENCLAW_INTEGRATION.integration_id.to_owned(),
            expires_at_ms: ticket.expires_at_ms,
            config_path: display_path(&self.paths.config_path, &self.paths.home_directory),
            credential_path: display_path(&self.paths.credential_path, &self.paths.home_directory),
            changes: vec![
                ExternalAgentManagedChange {
                    path: "models.providers.hal100".to_owned(),
                    action: ExternalAgentManagedChangeAction::RemoveManagedFragment,
                },
                ExternalAgentManagedChange {
                    path: "secrets.providers.hal100_gateway".to_owned(),
                    action: ExternalAgentManagedChangeAction::RemoveManagedFragment,
                },
                ExternalAgentManagedChange {
                    path: "openclaw-gateway-key".to_owned(),
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
    ) -> Result<bool, OpenClawIntegrationError> {
        self.pending_disconnect
            .discard(plan_id)
            .map_err(|_| OpenClawIntegrationError::InvalidPlan)
    }

    pub fn apply_disconnection(
        &self,
        plan_id: &str,
    ) -> Result<ExternalAgentDisconnectResult, OpenClawIntegrationError> {
        let pending = self
            .pending_disconnect
            .take(plan_id)
            .map_err(|_| OpenClawIntegrationError::InvalidPlan)?;
        self.ensure_default_instance()?;
        self.ensure_supported_version(&pending.binary)?;
        let current = read_managed_file(&self.paths.config_path, MAX_CONFIG_BYTES)?;
        if managed_content_hash(&current) != pending.original_digest {
            return Err(OpenClawIntegrationError::ChangedAfterPreview);
        }
        let current_key = read_credential(&self.paths.credential_path)?;
        if hash_client_key(&current_key) != pending.credential_digest {
            return Err(OpenClawIntegrationError::ChangedAfterPreview);
        }
        let config_mode = managed_file_mode(&self.paths.config_path)?;
        let credential_mode = managed_file_mode(&self.paths.credential_path)?;
        let backup = managed_backup_path(&self.paths.config_path);
        write_new_managed_file(&backup, &pending.original, config_mode)?;
        if let Err(error) = self.run_patch(&pending.binary, &pending.patch, false) {
            let _ = fs::remove_file(&backup);
            return Err(error);
        }
        if let Err(error) = fs::remove_file(&self.paths.credential_path) {
            rollback_disconnection_files(self, &pending, config_mode, credential_mode, &backup);
            return Err(error.into());
        }
        if let Some(parent) = self.paths.credential_path.parent() {
            let _ = sync_managed_directory(parent);
        }
        if !self.verify_absent() {
            rollback_disconnection_files(self, &pending, config_mode, credential_mode, &backup);
            return Err(OpenClawIntegrationError::VerificationFailed);
        }
        if let Err(error) = self
            .credentials
            .remove_client(OPENCLAW_INTEGRATION.client_app_id)
        {
            rollback_disconnection_files(self, &pending, config_mode, credential_mode, &backup);
            return Err(error.into());
        }
        let database_result = self.database.remove_managed_integration_and_client(
            OPENCLAW_INTEGRATION.integration_id,
            OPENCLAW_INTEGRATION.client_app_id,
            now_ms(),
        );
        if !matches!(database_result.as_ref(), Ok(true)) {
            rollback_disconnection_files(self, &pending, config_mode, credential_mode, &backup);
            let _ = self.credentials.upsert(pending.credential.clone());
            let _ = self.database.upsert_integration_resources_and_credential(
                &pending.integration,
                &pending.resources,
                &pending.credential,
            );
            return match database_result {
                Ok(false) => Err(OpenClawIntegrationError::NotConfigured),
                Err(error) => Err(error.into()),
                Ok(true) => unreachable!(),
            };
        }

        Ok(ExternalAgentDisconnectResult {
            disconnected: true,
            integration_id: OPENCLAW_INTEGRATION.integration_id.to_owned(),
            config_path: display_path(&self.paths.config_path, &self.paths.home_directory),
            backup_path: Some(display_path(&backup, &self.paths.home_directory)),
            credential_revoked: true,
        })
    }

    fn integration_status(
        &self,
        config: &Value,
        prior: Option<&ManagedIntegrationRecord>,
        profile: &ExternalAgentModelProfile,
    ) -> (
        ExternalAgentIntegrationState,
        Option<ExternalAgentGatewayProtocol>,
    ) {
        if prior.is_some_and(|record| {
            Path::new(&record.config_path) != self.paths.config_path
                || Path::new(&record.credential_path) != self.paths.credential_path
        }) {
            return (ExternalAgentIntegrationState::ModifiedOutsideHal100, None);
        }
        let (secret, provider) = managed_fragments(config);
        match (secret, provider, prior) {
            (None, None, None) => (ExternalAgentIntegrationState::InstalledNotConfigured, None),
            (Some(_), Some(_), None) | (Some(_), None, None) | (None, Some(_), None) => {
                (ExternalAgentIntegrationState::Conflict, None)
            }
            (None, None, Some(_)) | (Some(_), None, Some(_)) | (None, Some(_), Some(_)) => {
                (ExternalAgentIntegrationState::ModifiedOutsideHal100, None)
            }
            (Some(secret), Some(provider), Some(record)) => {
                if combined_fragment_hash(secret, provider) != record.managed_fragment_hash
                    || !self.credential_is_owned_and_valid()
                {
                    return (ExternalAgentIntegrationState::ModifiedOutsideHal100, None);
                }
                let configured_protocol = protocol_from_provider(provider);
                let expected = configured_protocol.map(|protocol| {
                    (
                        secret_provider_fragment(&self.paths.credential_path),
                        model_provider_fragment(
                            protocol,
                            &self.gateway_base_url,
                            profile,
                            &secret_reference(),
                        ),
                    )
                });
                if expected.is_some_and(|(expected_secret, expected_provider)| {
                    expected_secret == *secret && expected_provider == *provider
                }) {
                    (
                        ExternalAgentIntegrationState::Configured,
                        configured_protocol,
                    )
                } else {
                    (
                        ExternalAgentIntegrationState::NeedsRefresh,
                        configured_protocol,
                    )
                }
            }
        }
    }

    fn validate_configuration_patch(
        &self,
        binary: &Path,
        patch: &Value,
        new_plaintext_key: Option<&str>,
        protocol: ExternalAgentGatewayProtocol,
        profile: &ExternalAgentModelProfile,
    ) -> Result<(), OpenClawIntegrationError> {
        if let Some(key) = new_plaintext_key {
            fs::create_dir_all(&self.paths.temporary_directory)?;
            let validation_key = self
                .paths
                .temporary_directory
                .join(format!("validation-{}.key", Uuid::new_v4()));
            write_new_managed_file(&validation_key, key.as_bytes(), 0o600)?;
            let validation_secret = secret_provider_fragment(&validation_key);
            let validation_provider = model_provider_fragment(
                protocol,
                &self.gateway_base_url,
                profile,
                &secret_reference(),
            );
            let validation_patch = configuration_patch(&validation_secret, &validation_provider);
            let result = self.run_patch(binary, &validation_patch, true);
            let _ = fs::remove_file(validation_key);
            result
        } else {
            self.run_patch(binary, patch, true)
        }
    }

    fn run_patch(
        &self,
        binary: &Path,
        patch: &Value,
        dry_run: bool,
    ) -> Result<(), OpenClawIntegrationError> {
        fs::create_dir_all(&self.paths.temporary_directory)?;
        let patch_path = self
            .paths
            .temporary_directory
            .join(format!("patch-{}.json", Uuid::new_v4()));
        let bytes = serde_json::to_vec_pretty(patch)
            .expect("OpenClaw patches contain only serializable JSON values");
        write_new_managed_file(&patch_path, &bytes, 0o600)?;
        let result = self.config_tool.patch(
            binary,
            &self.command_environment(),
            &self.paths.config_path,
            &patch_path,
            dry_run,
        );
        let _ = fs::remove_file(&patch_path);
        result.map_err(OpenClawIntegrationError::ConfigTool)
    }

    fn command_environment(&self) -> Vec<(String, OsString)> {
        let mut environment = vec![
            (
                "OPENCLAW_HOME".to_owned(),
                self.paths.home_directory.as_os_str().to_owned(),
            ),
            (
                "OPENCLAW_STATE_DIR".to_owned(),
                self.paths.state_directory.as_os_str().to_owned(),
            ),
            (
                "OPENCLAW_CONFIG_PATH".to_owned(),
                self.paths.config_path.as_os_str().to_owned(),
            ),
            ("OPENCLAW_OFFLINE".to_owned(), OsString::from("1")),
            ("OPENCLAW_NO_AUTO_UPDATE".to_owned(), OsString::from("1")),
            ("OPENCLAW_LOAD_SHELL_ENV".to_owned(), OsString::from("0")),
            (
                "OPENCLAW_EXEC_SHELL_SNAPSHOT".to_owned(),
                OsString::from("0"),
            ),
            ("NO_COLOR".to_owned(), OsString::from("1")),
        ];
        if let Ok(path) = std::env::join_paths(
            [PathBuf::from("/usr/bin"), PathBuf::from("/bin")]
                .into_iter()
                .chain(self.paths.runtime_directories.iter().cloned()),
        ) {
            environment.push(("PATH".to_owned(), path));
        }
        environment
    }

    fn ensure_supported_version(&self, binary: &Path) -> Result<(), OpenClawIntegrationError> {
        let version = self
            .config_tool
            .version(binary, &self.command_environment())
            .map_err(OpenClawIntegrationError::ConfigTool)?;
        if stable_version_triplet(&version).is_none_or(|version| version < MIN_TESTED_VERSION) {
            return Err(OpenClawIntegrationError::UnsupportedVersion);
        }
        Ok(())
    }

    fn ensure_default_instance(&self) -> Result<(), OpenClawIntegrationError> {
        if has_custom_instance_override() {
            return Err(OpenClawIntegrationError::CustomInstanceRequiresExplicitConfiguration);
        }
        Ok(())
    }

    fn custom_instance_warnings(&self) -> Vec<String> {
        [
            "OPENCLAW_HOME",
            "OPENCLAW_STATE_DIR",
            "OPENCLAW_CONFIG_PATH",
            "OPENCLAW_PROFILE",
        ]
        .into_iter()
        .filter_map(|name| {
            std::env::var_os(name).map(|value| {
                format!(
                    "检测到{name}={}；HAL100默认不会写入该自定义OpenClaw实例",
                    value.to_string_lossy()
                )
            })
        })
        .collect()
    }

    fn read_config(&self) -> Result<(Vec<u8>, Value), OpenClawIntegrationError> {
        let bytes = read_managed_file(&self.paths.config_path, MAX_CONFIG_BYTES)?;
        let config = parse_config(&bytes)?;
        Ok((bytes, config))
    }

    fn current_config_bytes(
        &self,
        expected_to_exist: bool,
        virtual_empty: &[u8],
    ) -> Result<Vec<u8>, OpenClawIntegrationError> {
        if self.paths.config_path.exists() {
            read_managed_file(&self.paths.config_path, MAX_CONFIG_BYTES).map_err(Into::into)
        } else if expected_to_exist {
            Err(OpenClawIntegrationError::ChangedAfterPreview)
        } else {
            Ok(virtual_empty.to_vec())
        }
    }

    fn create_backup(
        &self,
        original: &[u8],
        config_existed: bool,
        config_mode: u32,
    ) -> Result<Option<PathBuf>, OpenClawIntegrationError> {
        if !config_existed {
            return Ok(None);
        }
        let backup = managed_backup_path(&self.paths.config_path);
        write_new_managed_file(&backup, original, config_mode)?;
        Ok(Some(backup))
    }

    fn verify_fragments(&self, expected_secret: &Value, expected_provider: &Value) -> bool {
        self.read_config()
            .ok()
            .map(|(_, config)| config)
            .and_then(|config| {
                let (secret, provider) = managed_fragments(&config);
                Some((secret?.clone(), provider?.clone()))
            })
            .is_some_and(|(secret, provider)| {
                secret == *expected_secret && provider == *expected_provider
            })
    }

    fn verify_absent(&self) -> bool {
        self.read_config()
            .ok()
            .is_some_and(|(_, config)| managed_fragments(&config) == (None, None))
    }

    fn credential_is_owned_and_valid(&self) -> bool {
        let Ok(key) = read_credential(&self.paths.credential_path) else {
            return false;
        };
        self.stored_credential()
            .ok()
            .flatten()
            .is_some_and(|credential| hash_client_key(&key) == credential.key_hash)
    }

    fn stored_credential(
        &self,
    ) -> Result<Option<StoredClientCredential>, OpenClawIntegrationError> {
        Ok(self
            .database
            .load_client_credentials()?
            .into_iter()
            .find(|credential| {
                credential.key_id == OPENCLAW_INTEGRATION.credential_id
                    && credential.client_app_id == OPENCLAW_INTEGRATION.client_app_id
            }))
    }
}

fn ensure_supported_protocol(
    protocol: ExternalAgentGatewayProtocol,
) -> Result<(), OpenClawIntegrationError> {
    match protocol {
        ExternalAgentGatewayProtocol::OpenAiChatCompletions
        | ExternalAgentGatewayProtocol::OpenAiResponses
        | ExternalAgentGatewayProtocol::AnthropicMessages => Ok(()),
    }
}

fn configuration_patch(secret_fragment: &Value, provider_fragment: &Value) -> Value {
    json!({
        "secrets": {"providers": {SECRET_PROVIDER_KEY: secret_fragment}},
        "models": {"providers": {MODEL_PROVIDER_KEY: provider_fragment}}
    })
}

fn disconnection_patch() -> Value {
    json!({
        "secrets": {"providers": {SECRET_PROVIDER_KEY: Value::Null}},
        "models": {"providers": {MODEL_PROVIDER_KEY: Value::Null}}
    })
}

fn secret_provider_fragment(credential_path: &Path) -> Value {
    json!({
        "source": "file",
        "path": credential_path.to_string_lossy(),
        "mode": "singleValue"
    })
}

fn secret_reference() -> Value {
    json!({
        "source": "file",
        "provider": SECRET_PROVIDER_KEY,
        "id": "value"
    })
}

fn model_provider_fragment(
    protocol: ExternalAgentGatewayProtocol,
    gateway_base_url: &str,
    profile: &ExternalAgentModelProfile,
    credential_reference: &Value,
) -> Value {
    let input = profile
        .input_modalities
        .iter()
        .map(|modality| match modality {
            ExternalAgentInputModality::Text => "text",
            ExternalAgentInputModality::Image => "image",
        })
        .collect::<Vec<_>>();
    let mut provider = Map::new();
    provider.insert(
        "baseUrl".to_owned(),
        Value::String(protocol_base_url(protocol, gateway_base_url)),
    );
    provider.insert(
        "api".to_owned(),
        Value::String(protocol_api_name(protocol).to_owned()),
    );
    provider.insert("apiKey".to_owned(), credential_reference.clone());
    provider.insert(
        "authHeader".to_owned(),
        Value::Bool(protocol != ExternalAgentGatewayProtocol::AnthropicMessages),
    );
    provider.insert(
        "models".to_owned(),
        json!([{
            "id": profile.model_id,
            "name": profile.display_name,
            "reasoning": profile.supports_reasoning,
            "input": input,
            "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0},
            "contextWindow": profile.context_window_tokens,
            "contextTokens": profile.context_window_tokens,
            "maxTokens": profile.max_output_tokens,
            "compat": {
                "supportsTools": profile.supports_tools,
                "supportsReasoningEffort": profile.supports_reasoning,
                "supportsUsageInStreaming": true,
                "supportsDeveloperRole": false,
                "maxTokensField": "max_tokens",
                "toolSchemaProfile": "llamacpp"
            }
        }]),
    );
    Value::Object(provider)
}

fn protocol_api_name(protocol: ExternalAgentGatewayProtocol) -> &'static str {
    match protocol {
        ExternalAgentGatewayProtocol::OpenAiChatCompletions => "openai-completions",
        ExternalAgentGatewayProtocol::OpenAiResponses => "openai-responses",
        ExternalAgentGatewayProtocol::AnthropicMessages => "anthropic-messages",
    }
}

fn protocol_base_url(protocol: ExternalAgentGatewayProtocol, gateway_base_url: &str) -> String {
    match protocol {
        ExternalAgentGatewayProtocol::AnthropicMessages => gateway_base_url
            .strip_suffix("/v1")
            .unwrap_or(gateway_base_url)
            .to_owned(),
        _ => gateway_base_url.to_owned(),
    }
}

fn managed_fragments(config: &Value) -> (Option<&Value>, Option<&Value>) {
    let secret = config
        .get("secrets")
        .and_then(|value| value.get("providers"))
        .and_then(|value| value.get(SECRET_PROVIDER_KEY));
    let provider = config
        .get("models")
        .and_then(|value| value.get("providers"))
        .and_then(|value| value.get(MODEL_PROVIDER_KEY));
    (secret, provider)
}

fn protocol_from_provider(provider: &Value) -> Option<ExternalAgentGatewayProtocol> {
    match provider.get("api")?.as_str()? {
        "openai-completions" => Some(ExternalAgentGatewayProtocol::OpenAiChatCompletions),
        "openai-responses" => Some(ExternalAgentGatewayProtocol::OpenAiResponses),
        "anthropic-messages" => Some(ExternalAgentGatewayProtocol::AnthropicMessages),
        _ => None,
    }
}

fn combined_fragment_hash(secret: &Value, provider: &Value) -> [u8; 32] {
    value_hash(&json!({"secretProvider": secret, "modelProvider": provider}))
}

fn parse_config(bytes: &[u8]) -> Result<Value, OpenClawIntegrationError> {
    let source = std::str::from_utf8(bytes)
        .map_err(|error| OpenClawIntegrationError::InvalidJson5(error.to_string()))?;
    let config: Value = json5::from_str(source)
        .map_err(|error| OpenClawIntegrationError::InvalidJson5(error.to_string()))?;
    if !config.is_object() {
        return Err(OpenClawIntegrationError::InvalidJson5(
            "根值必须是对象".to_owned(),
        ));
    }
    Ok(config)
}

fn read_credential(path: &Path) -> Result<String, OpenClawIntegrationError> {
    let bytes = read_managed_file(path, 256)?;
    if !(24..=256).contains(&bytes.len()) {
        return Err(OpenClawIntegrationError::InvalidCredentialFile);
    }
    let key =
        String::from_utf8(bytes).map_err(|_| OpenClawIntegrationError::InvalidCredentialFile)?;
    if key.contains(['\n', '\r']) {
        return Err(OpenClawIntegrationError::InvalidCredentialFile);
    }
    #[cfg(unix)]
    if managed_file_mode(path)? & 0o077 != 0 {
        return Err(OpenClawIntegrationError::InvalidCredentialFile);
    }
    Ok(key)
}

fn rollback_configuration_files(
    adapter: &OpenClawIntegrationAdapter,
    pending: &PendingConfiguration,
    config_mode: u32,
    credential_created: bool,
    backup: Option<&Path>,
) {
    if pending.config_existed {
        let _ =
            atomic_write_managed_file(&adapter.paths.config_path, &pending.original, config_mode);
    } else {
        let _ = fs::remove_file(&adapter.paths.config_path);
    }
    if credential_created {
        let _ = fs::remove_file(&adapter.paths.credential_path);
    }
    if let Some(backup) = backup {
        let _ = fs::remove_file(backup);
    }
}

fn rollback_registry(adapter: &OpenClawIntegrationAdapter, prior: Option<StoredClientCredential>) {
    let _ = adapter
        .credentials
        .remove_client(OPENCLAW_INTEGRATION.client_app_id);
    if let Some(prior) = prior {
        let _ = adapter.credentials.upsert(prior);
    }
}

fn rollback_disconnection_files(
    adapter: &OpenClawIntegrationAdapter,
    pending: &PendingDisconnect,
    config_mode: u32,
    credential_mode: u32,
    backup: &Path,
) {
    let _ = atomic_write_managed_file(&adapter.paths.config_path, &pending.original, config_mode);
    let _ = atomic_write_managed_file(
        &adapter.paths.credential_path,
        pending.plaintext_key.as_bytes(),
        credential_mode,
    );
    let _ = fs::remove_file(backup);
}

fn generate_client_key() -> String {
    format!(
        "hal100_openclaw_{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

fn value_hash(value: &Value) -> [u8; 32] {
    managed_content_hash(
        serde_json::to_string(value)
            .expect("JSON values always serialize")
            .as_bytes(),
    )
}

fn has_custom_instance_override() -> bool {
    [
        "OPENCLAW_HOME",
        "OPENCLAW_STATE_DIR",
        "OPENCLAW_CONFIG_PATH",
        "OPENCLAW_PROFILE",
    ]
    .into_iter()
    .any(|name| std::env::var_os(name).is_some())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}

fn display_path(path: &Path, home: &Path) -> String {
    path.strip_prefix(home).map_or_else(
        |_| path.to_string_lossy().into_owned(),
        |relative| format!("~/{}", relative.to_string_lossy()),
    )
}

fn find_binary(candidates: &[PathBuf]) -> Option<PathBuf> {
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

fn node_runtime_directories(home: &Path) -> Vec<PathBuf> {
    let mut directories = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        home.join(".volta/bin"),
        home.join(".local/bin"),
        home.join(".bun/bin"),
    ];
    for root in [home.join(".local/share"), home.join(".nvm/versions/node")] {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten().take(32) {
            directories.push(entry.path().join("bin"));
        }
    }
    directories.retain(|directory| is_executable_file(&directory.join("node")));
    directories.sort();
    directories.dedup();
    directories
}

fn stable_version_triplet(version: &str) -> Option<(u64, u64, u64)> {
    let version = version.split_whitespace().find(|part| {
        part.chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
    })?;
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
            let path = std::env::temp_dir().join(format!("hal100-openclaw-{}", Uuid::new_v4()));
            fs::create_dir(&path).expect("create test directory");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[derive(Debug, Default)]
    struct FakeConfigTool;

    impl OpenClawConfigTool for FakeConfigTool {
        fn version(
            &self,
            _binary: &Path,
            _environment: &[(String, OsString)],
        ) -> Result<String, String> {
            Ok("OpenClaw 2026.7.1-2".to_owned())
        }

        fn patch(
            &self,
            _binary: &Path,
            _environment: &[(String, OsString)],
            config_path: &Path,
            patch_path: &Path,
            dry_run: bool,
        ) -> Result<(), String> {
            let mut config = if config_path.exists() {
                parse_config(&fs::read(config_path).map_err(|error| error.to_string())?)
                    .map_err(|error| error.to_string())?
            } else {
                json!({})
            };
            let patch = parse_config(&fs::read(patch_path).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
            apply_merge_patch(&mut config, &patch);
            if !dry_run {
                if let Some(parent) = config_path.parent() {
                    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                }
                fs::write(
                    config_path,
                    serde_json::to_vec_pretty(&config).map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
            }
            Ok(())
        }
    }

    fn apply_merge_patch(target: &mut Value, patch: &Value) {
        let Value::Object(patch) = patch else {
            *target = patch.clone();
            return;
        };
        if !target.is_object() {
            *target = json!({});
        }
        let target = target.as_object_mut().expect("object target");
        for (key, value) in patch {
            if value.is_null() {
                target.remove(key);
            } else if value.is_object() {
                apply_merge_patch(target.entry(key).or_insert_with(|| json!({})), value);
            } else {
                target.insert(key.clone(), value.clone());
            }
        }
    }

    struct Fixture {
        _root: TestDirectory,
        adapter: OpenClawIntegrationAdapter,
        database: Arc<Database>,
        credentials: CredentialRegistry,
        config_path: PathBuf,
        credential_path: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = TestDirectory::new();
            let binary = root.0.join("bin/openclaw");
            fs::create_dir_all(binary.parent().expect("binary parent")).expect("binary directory");
            fs::write(&binary, "fake").expect("fake binary");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&binary, fs::Permissions::from_mode(0o700))
                    .expect("executable mode");
            }
            let database =
                Arc::new(Database::open(root.0.join("hal100.sqlite")).expect("database"));
            let credentials = CredentialRegistry::new(Vec::new());
            let state_directory = root.0.join(".openclaw");
            let config_path = state_directory.join("openclaw.json");
            let credential_path = root.0.join("app-data/credentials/openclaw-gateway.key");
            let paths = OpenClawPaths {
                home_directory: root.0.clone(),
                state_directory,
                config_path: config_path.clone(),
                credential_path: credential_path.clone(),
                temporary_directory: root.0.join("app-data/integration-plans/openclaw"),
                binary_candidates: vec![binary],
                runtime_directories: Vec::new(),
            };
            let adapter = OpenClawIntegrationAdapter::with_config_tool(
                database.clone(),
                credentials.clone(),
                ExternalModelProfileRegistry::conservative_managed_route(),
                paths,
                "http://127.0.0.1:10100/v1".to_owned(),
                Arc::new(FakeConfigTool),
            );
            Self {
                _root: root,
                adapter,
                database,
                credentials,
                config_path,
                credential_path,
            }
        }
    }

    #[test]
    fn protocol_fragments_use_the_correct_api_and_base_url() {
        let profile = ExternalModelProfileRegistry::conservative_managed_route()
            .snapshot()
            .expect("profile");
        for (protocol, api, base_url) in [
            (
                ExternalAgentGatewayProtocol::OpenAiChatCompletions,
                "openai-completions",
                "http://127.0.0.1:10100/v1",
            ),
            (
                ExternalAgentGatewayProtocol::OpenAiResponses,
                "openai-responses",
                "http://127.0.0.1:10100/v1",
            ),
            (
                ExternalAgentGatewayProtocol::AnthropicMessages,
                "anthropic-messages",
                "http://127.0.0.1:10100",
            ),
        ] {
            let fragment = model_provider_fragment(
                protocol,
                "http://127.0.0.1:10100/v1",
                &profile,
                &secret_reference(),
            );
            assert_eq!(fragment["api"], api);
            assert_eq!(fragment["baseUrl"], base_url);
        }
    }

    #[test]
    fn configuration_preserves_defaults_and_uses_file_secret_ref() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.config_path.parent().expect("config parent"))
            .expect("config directory");
        fs::write(
            &fixture.config_path,
            "{\n  // user comment\n  agents: {defaults: {model: {primary: 'user/model'}}},\n  channels: {keep: true},\n}\n",
        )
        .expect("config fixture");

        let plan = fixture
            .adapter
            .plan_configuration(ExternalAgentGatewayProtocol::OpenAiResponses)
            .expect("plan");
        assert!(plan.preserves_default_model);
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.contains("JSON5"))
        );
        let result = fixture
            .adapter
            .apply_configuration(&plan.plan_id)
            .expect("apply");
        let config = parse_config(&fs::read(&fixture.config_path).expect("config")).expect("JSON5");
        assert_eq!(
            config["agents"]["defaults"]["model"]["primary"],
            "user/model"
        );
        assert_eq!(config["channels"]["keep"], true);
        assert_eq!(
            config["models"]["providers"]["hal100"]["api"],
            "openai-responses"
        );
        assert_eq!(
            config["models"]["providers"]["hal100"]["apiKey"]["provider"],
            "hal100_gateway"
        );
        assert!(result.backup_path.is_some());
        assert_eq!(
            fixture
                .adapter
                .detect()
                .expect("detect")
                .configured_protocol,
            Some(ExternalAgentGatewayProtocol::OpenAiResponses)
        );
        let key = fs::read_to_string(&fixture.credential_path).expect("credential");
        assert_eq!(
            fixture
                .credentials
                .authenticate(&key)
                .expect("gateway client")
                .client_app_id,
            "openclaw"
        );
        #[cfg(unix)]
        assert_eq!(
            managed_file_mode(&fixture.credential_path).expect("mode"),
            0o600
        );
    }

    #[test]
    fn existing_foreign_fragment_is_never_overwritten() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.config_path.parent().expect("config parent"))
            .expect("config directory");
        fs::write(
            &fixture.config_path,
            r#"{"models":{"providers":{"hal100":{"baseUrl":"https://foreign"}}}}"#,
        )
        .expect("foreign config");

        assert!(matches!(
            fixture
                .adapter
                .plan_configuration(ExternalAgentGatewayProtocol::OpenAiChatCompletions),
            Err(OpenClawIntegrationError::ProviderConflict)
        ));
    }

    #[test]
    fn stale_plan_never_overwrites_user_changes() {
        let fixture = Fixture::new();
        let plan = fixture
            .adapter
            .plan_configuration(ExternalAgentGatewayProtocol::OpenAiChatCompletions)
            .expect("plan");
        fs::create_dir_all(fixture.config_path.parent().expect("config parent"))
            .expect("config directory");
        fs::write(&fixture.config_path, b"{user: true}").expect("user edit");

        assert!(matches!(
            fixture.adapter.apply_configuration(&plan.plan_id),
            Err(OpenClawIntegrationError::ChangedAfterPreview)
        ));
        assert!(!fixture.credential_path.exists());
    }

    #[test]
    fn disconnect_removes_only_hal100_fragments_and_revokes_key() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.config_path.parent().expect("config parent"))
            .expect("config directory");
        fs::write(
            &fixture.config_path,
            r#"{"agents":{"defaults":{"model":{"primary":"user/model"}}},"models":{"providers":{"other":{"baseUrl":"https://keep"}}},"secrets":{"providers":{"other":{"source":"env"}}}}"#,
        )
        .expect("config");
        let configure = fixture
            .adapter
            .plan_configuration(ExternalAgentGatewayProtocol::AnthropicMessages)
            .expect("plan");
        fixture
            .adapter
            .apply_configuration(&configure.plan_id)
            .expect("configure");
        let key = fs::read_to_string(&fixture.credential_path).expect("key");

        let disconnect = fixture
            .adapter
            .plan_disconnection()
            .expect("disconnect plan");
        fixture
            .adapter
            .apply_disconnection(&disconnect.plan_id)
            .expect("disconnect");
        let config = parse_config(&fs::read(&fixture.config_path).expect("config")).expect("JSON5");
        assert_eq!(
            config["agents"]["defaults"]["model"]["primary"],
            "user/model"
        );
        assert_eq!(
            config["models"]["providers"]["other"]["baseUrl"],
            "https://keep"
        );
        assert_eq!(config["secrets"]["providers"]["other"]["source"], "env");
        assert_eq!(managed_fragments(&config), (None, None));
        assert!(!fixture.credential_path.exists());
        assert!(fixture.credentials.authenticate(&key).is_none());
        assert_eq!(
            fixture
                .database
                .managed_integration(OPENCLAW_INTEGRATION.integration_id)
                .expect("database"),
            None
        );
    }

    #[test]
    fn version_parser_accepts_date_versions_with_package_suffixes() {
        assert_eq!(
            stable_version_triplet("OpenClaw 2026.7.1-2"),
            Some((2026, 7, 1))
        );
        assert_eq!(stable_version_triplet("2026.6.9"), Some((2026, 6, 9)));
    }
}
