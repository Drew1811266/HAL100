use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use hal100_core::HERMES_AGENT_INTEGRATION;
use hal100_protocol::{
    ExternalAgentConfigurationChange, ExternalAgentConfigurationPlan,
    ExternalAgentConfigurationResult, ExternalAgentDetection, ExternalAgentDisconnectPlan,
    ExternalAgentDisconnectResult, ExternalAgentGatewayProtocol, ExternalAgentIntegrationState,
    ExternalAgentManagedChange, ExternalAgentManagedChangeAction, ExternalAgentModelProfile,
};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::hermes_config::{
    HERMES_CREDENTIAL_ENV_KEY, HERMES_PROVIDER_KEY, HermesConfigError, env_entry_is_canonical,
    managed_provider, parse_yaml_config, patch_managed_env, patch_managed_provider,
    provider_fragment, read_managed_env_value, remove_managed_env, remove_managed_provider,
    serialize_yaml_config,
};
use crate::{
    BoundedCommandRunner, ClientCredentialError, CredentialRegistry, Database, DatabaseError,
    ExternalModelProfileRegistry, ManagedFileError, ManagedIntegrationRecord,
    ManagedIntegrationResourceRecord, ModelProfileError, PendingPlanStore, StoredClientCredential,
    atomic_write_managed_file, hash_client_key, managed_backup_path, managed_content_hash,
    managed_file_mode, read_managed_file, reject_managed_file_symlink, stored_client_credential,
    sync_managed_directory, write_new_managed_file,
};

const PLAN_TTL: Duration = Duration::from_secs(5 * 60);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_COMMAND_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_CONFIG_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ENV_BYTES: u64 = 1024 * 1024;
const MIN_TESTED_VERSION: (u64, u64, u64) = (0, 18, 2);
const MIN_CONTEXT_WINDOW_TOKENS: u32 = 64_000;

#[derive(Debug, Clone)]
pub struct HermesAgentPaths {
    pub home_directory: PathBuf,
    pub hermes_directory: PathBuf,
    pub config_path: PathBuf,
    pub environment_path: PathBuf,
    pub validation_root: PathBuf,
    pub binary_candidates: Vec<PathBuf>,
}

impl HermesAgentPaths {
    pub fn for_macos(home_directory: &Path, app_data_directory: &Path) -> Self {
        let hermes_directory = home_directory.join(".hermes");
        Self {
            home_directory: home_directory.to_path_buf(),
            config_path: hermes_directory.join("config.yaml"),
            environment_path: hermes_directory.join(".env"),
            validation_root: app_data_directory.join("temporary/hermes-validation"),
            hermes_directory,
            binary_candidates: vec![
                home_directory.join(".local/bin/hermes"),
                home_directory.join(".local/share/uv/tools/hermes-agent/bin/hermes"),
                home_directory.join(".hermes/bin/hermes"),
                home_directory.join(".hermes/hermes-agent/venv/bin/hermes"),
                home_directory.join(".hermes/hermes-agent/.venv/bin/hermes"),
                PathBuf::from("/opt/homebrew/bin/hermes"),
                PathBuf::from("/usr/local/bin/hermes"),
            ],
        }
    }
}

#[derive(Debug, Error)]
pub enum HermesAgentIntegrationError {
    #[error("未检测到官方Hermes Agent CLI，请先独立安装Hermes后再配置")]
    NotInstalled,
    #[error("Hermes Agent版本低于HAL100当前验收下限0.18.2，请先升级")]
    UnsupportedVersion,
    #[error("当前HAL100模型上下文窗口低于Hermes Agent要求的64000 Token，不能安全接入")]
    IncompatibleModelProfile,
    #[error("HAL100管理的Hermes配置已被外部修改，请先检查差异")]
    ManagedProviderModified,
    #[error("HAL100管理的Hermes模型能力版本已变化，请重新预览配置")]
    ModelProfileChanged,
    #[error("配置计划不存在、已使用或已经过期")]
    InvalidPlan,
    #[error("确认后Hermes配置或凭据发生了变化，请重新预览")]
    ChangedAfterPreview,
    #[error("HAL100 Hermes专属变量已存在但没有对应安装记录")]
    UnownedCredentialVariable,
    #[error("HAL100 Hermes专属变量无效")]
    InvalidCredentialVariable,
    #[error("Hermes Agent尚未由HAL100配置，无可断开的受管接入")]
    NotConfigured,
    #[error("HAL100管理的Hermes凭据已被外部修改，请先检查")]
    ManagedCredentialModified,
    #[error("Hermes官方CLI验证配置失败: {0}")]
    ConfigTool(String),
    #[error("写入后验证失败，已经恢复原配置")]
    VerificationFailed,
    #[error("{0}")]
    Config(String),
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

impl From<HermesConfigError> for HermesAgentIntegrationError {
    fn from(error: HermesConfigError) -> Self {
        Self::Config(error.to_string())
    }
}

trait HermesConfigTool: Send + Sync {
    fn version(&self, binary: &Path, environment: &[(String, OsString)]) -> Result<String, String>;

    fn show_config(
        &self,
        binary: &Path,
        environment: &[(String, OsString)],
    ) -> Result<String, String>;
}

#[derive(Debug, Default)]
struct OfficialHermesConfigTool;

impl HermesConfigTool for OfficialHermesConfigTool {
    fn version(&self, binary: &Path, environment: &[(String, OsString)]) -> Result<String, String> {
        BoundedCommandRunner::new(Duration::from_secs(3), 512)
            .run_utf8_with_env(binary, &["--version"], environment)
            .map(|output| output.trim().to_owned())
            .map_err(|error| error.to_string())
    }

    fn show_config(
        &self,
        binary: &Path,
        environment: &[(String, OsString)],
    ) -> Result<String, String> {
        BoundedCommandRunner::new(COMMAND_TIMEOUT, MAX_COMMAND_OUTPUT_BYTES)
            .run_utf8_with_env(binary, &["-p", "default", "config", "show"], environment)
            .map_err(|error| error.to_string())
    }
}

pub struct HermesAgentIntegrationAdapter {
    database: Arc<Database>,
    credentials: CredentialRegistry,
    model_profiles: ExternalModelProfileRegistry,
    paths: HermesAgentPaths,
    gateway_base_url: String,
    config_tool: Arc<dyn HermesConfigTool>,
    pending: PendingPlanStore<PendingConfiguration>,
    pending_disconnect: PendingPlanStore<PendingDisconnect>,
}

struct PendingConfiguration {
    binary: PathBuf,
    original_config_digest: [u8; 32],
    original_config: Vec<u8>,
    config_existed: bool,
    patched_config: Vec<u8>,
    original_env_digest: [u8; 32],
    original_env: Vec<u8>,
    env_existed: bool,
    patched_env: Vec<u8>,
    fragment: Value,
    fragment_hash: [u8; 32],
    plaintext_key: String,
    profile_revision: String,
    prior_integration: Option<ManagedIntegrationRecord>,
    prior_credential: Option<StoredClientCredential>,
}

struct PendingDisconnect {
    binary: PathBuf,
    original_config_digest: [u8; 32],
    original_config: Vec<u8>,
    patched_config: Vec<u8>,
    original_env_digest: [u8; 32],
    original_env: Vec<u8>,
    patched_env: Vec<u8>,
    credential_digest: [u8; 32],
    integration: ManagedIntegrationRecord,
    resources: Vec<ManagedIntegrationResourceRecord>,
    credential: StoredClientCredential,
}

impl HermesAgentIntegrationAdapter {
    pub fn new(
        database: Arc<Database>,
        credentials: CredentialRegistry,
        model_profiles: ExternalModelProfileRegistry,
        paths: HermesAgentPaths,
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
        paths: HermesAgentPaths,
        gateway_base_url: String,
    ) -> Self {
        Self::with_config_tool(
            database,
            credentials,
            model_profiles,
            paths,
            gateway_base_url,
            Arc::new(OfficialHermesConfigTool),
        )
    }

    fn with_config_tool(
        database: Arc<Database>,
        credentials: CredentialRegistry,
        model_profiles: ExternalModelProfileRegistry,
        paths: HermesAgentPaths,
        gateway_base_url: String,
        config_tool: Arc<dyn HermesConfigTool>,
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

    pub fn detect(&self) -> Result<ExternalAgentDetection, HermesAgentIntegrationError> {
        let profile = self.model_profiles.snapshot()?;
        let binary = find_binary(&self.paths.binary_candidates);
        let environment = self.command_environment(&self.paths.hermes_directory);
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
            .managed_integration(HERMES_AGENT_INTEGRATION.integration_id)?;
        let mut warnings = self.profile_warnings();
        if binary.is_some() && version.is_none() {
            warnings.push("检测到Hermes CLI，但无法在受限环境中读取版本".to_owned());
        }
        if !version_supported {
            warnings.push("该Hermes版本早于HAL100当前自动验收下限0.18.2".to_owned());
        }
        if profile.context_window_tokens < MIN_CONTEXT_WINDOW_TOKENS {
            warnings.push(format!(
                "当前模型仅声明{} Token上下文；Hermes 0.18.2要求至少{} Token",
                profile.context_window_tokens, MIN_CONTEXT_WINDOW_TOKENS
            ));
        }

        let config_exists = self.paths.config_path.exists();
        let env_exists = self.paths.environment_path.exists();
        let mut state = if config_exists || env_exists || prior.is_some() {
            match self.read_current_files() {
                Ok((_, config, env)) => {
                    self.integration_state(&config, &env, prior.as_ref(), &profile)?
                }
                Err(error) => {
                    warnings.push(format!("Hermes配置无法安全解析：{error}"));
                    ExternalAgentIntegrationState::Conflict
                }
            }
        } else if binary.is_some() {
            ExternalAgentIntegrationState::InstalledNotConfigured
        } else {
            ExternalAgentIntegrationState::NotInstalled
        };
        if !version_supported && state == ExternalAgentIntegrationState::InstalledNotConfigured {
            state = ExternalAgentIntegrationState::UnsupportedVersion;
        }
        if binary.is_some()
            && profile.context_window_tokens < MIN_CONTEXT_WINDOW_TOKENS
            && matches!(
                state,
                ExternalAgentIntegrationState::InstalledNotConfigured
                    | ExternalAgentIntegrationState::Configured
                    | ExternalAgentIntegrationState::NeedsRefresh
            )
        {
            state = ExternalAgentIntegrationState::Blocked;
        }

        Ok(ExternalAgentDetection {
            integration_id: HERMES_AGENT_INTEGRATION.integration_id.to_owned(),
            display_name: HERMES_AGENT_INTEGRATION.display_name.to_owned(),
            installed: binary.is_some(),
            version,
            binary_path: binary.map(|path| display_path(&path, &self.paths.home_directory)),
            config_path: display_path(&self.paths.config_path, &self.paths.home_directory),
            config_exists,
            integration_state: state,
            configured_protocol: matches!(
                state,
                ExternalAgentIntegrationState::Configured
                    | ExternalAgentIntegrationState::NeedsRefresh
            )
            .then_some(ExternalAgentGatewayProtocol::OpenAiChatCompletions),
            model_profile_revision: profile.revision,
            warnings,
        })
    }

    pub fn plan_configuration(
        &self,
    ) -> Result<ExternalAgentConfigurationPlan, HermesAgentIntegrationError> {
        let binary = find_binary(&self.paths.binary_candidates)
            .ok_or(HermesAgentIntegrationError::NotInstalled)?;
        self.ensure_supported_version(&binary)?;
        reject_managed_file_symlink(&self.paths.config_path)?;
        reject_managed_file_symlink(&self.paths.environment_path)?;
        let profile = self.model_profiles.snapshot()?;
        self.ensure_compatible_profile(&profile)?;
        let (files, config, env) = self.read_current_files()?;
        let prior = self
            .database
            .managed_integration(HERMES_AGENT_INTEGRATION.integration_id)?;
        let state = self.integration_state(&config, &env, prior.as_ref(), &profile)?;
        match state {
            ExternalAgentIntegrationState::Conflict => {
                return Err(HermesConfigError::ProviderConflict.into());
            }
            ExternalAgentIntegrationState::ModifiedOutsideHal100 => {
                return Err(HermesAgentIntegrationError::ManagedProviderModified);
            }
            _ => {}
        }

        let prior_credential = self.stored_credential()?;
        let plaintext_key = if prior.is_some() {
            let key = read_env_credential(&files.environment)?;
            let credential = prior_credential
                .as_ref()
                .ok_or(HermesAgentIntegrationError::ManagedCredentialModified)?;
            if hash_client_key(&key) != credential.key_hash {
                return Err(HermesAgentIntegrationError::ManagedCredentialModified);
            }
            key
        } else {
            if read_managed_env_value(&files.environment)?.is_some() || prior_credential.is_some() {
                return Err(HermesAgentIntegrationError::UnownedCredentialVariable);
            }
            generate_client_key()
        };

        let fragment = provider_fragment(&self.gateway_base_url, &profile);
        let allow_replace = matches!(
            state,
            ExternalAgentIntegrationState::Configured | ExternalAgentIntegrationState::NeedsRefresh
        );
        let patched_config_value = patch_managed_provider(&config, &fragment, allow_replace)?;
        let patched_config = serialize_yaml_config(&patched_config_value)?;
        let patched_env = patch_managed_env(&files.environment, &plaintext_key, allow_replace)?;
        self.validate_staged(&binary, &patched_config, &patched_env, true)?;

        let fragment_hash = value_hash(&fragment);
        let pending = PendingConfiguration {
            binary,
            original_config_digest: managed_content_hash(&files.config),
            original_config: files.config,
            config_existed: files.config_existed,
            patched_config,
            original_env_digest: managed_content_hash(&files.environment),
            original_env: files.environment,
            env_existed: files.env_existed,
            patched_env,
            fragment,
            fragment_hash,
            plaintext_key,
            profile_revision: profile.revision.clone(),
            prior_integration: prior,
            prior_credential,
        };
        let ticket = self
            .pending
            .replace(pending)
            .map_err(|_| HermesAgentIntegrationError::InvalidPlan)?;
        let mut warnings = vec![
            "HAL100只配置Hermes default Profile，不改变粘性Profile、默认模型或运行中的服务"
                .to_owned(),
            "Hermes凭据写入.env中的HAL100专属变量；其他变量逐字保留且文件权限收紧为0600".to_owned(),
        ];
        if files.config_existed {
            warnings
                .push("YAML会被语义化重写；注释、锚点和排版可能标准化，原文件会先备份".to_owned());
        }

        Ok(ExternalAgentConfigurationPlan {
            plan_id: ticket.plan_id,
            integration_id: HERMES_AGENT_INTEGRATION.integration_id.to_owned(),
            expires_at_ms: ticket.expires_at_ms,
            config_path: display_path(&self.paths.config_path, &self.paths.home_directory),
            credential_path: display_path(&self.paths.environment_path, &self.paths.home_directory),
            changes: vec![
                ExternalAgentConfigurationChange {
                    path: "providers.hal100.api".to_owned(),
                    value: self.gateway_base_url.clone(),
                },
                ExternalAgentConfigurationChange {
                    path: "providers.hal100.transport".to_owned(),
                    value: "chat_completions".to_owned(),
                },
                ExternalAgentConfigurationChange {
                    path: "providers.hal100.key_env".to_owned(),
                    value: format!("{HERMES_CREDENTIAL_ENV_KEY}（值不显示）"),
                },
                ExternalAgentConfigurationChange {
                    path: "providers.hal100.models.hal100-active".to_owned(),
                    value: format!(
                        "{} · 上下文{} · 最大输出{}",
                        profile.display_name,
                        profile.context_window_tokens,
                        profile.max_output_tokens
                    ),
                },
            ],
            gateway_protocol: ExternalAgentGatewayProtocol::OpenAiChatCompletions,
            creates_backup: files.config_existed,
            preserves_default_model: true,
            requires_confirmation: true,
            model_profile_revision: profile.revision,
            warnings,
        })
    }

    pub fn discard_configuration_plan(
        &self,
        plan_id: &str,
    ) -> Result<bool, HermesAgentIntegrationError> {
        self.pending
            .discard(plan_id)
            .map_err(|_| HermesAgentIntegrationError::InvalidPlan)
    }

    pub fn apply_configuration(
        &self,
        plan_id: &str,
    ) -> Result<ExternalAgentConfigurationResult, HermesAgentIntegrationError> {
        let pending = self
            .pending
            .take(plan_id)
            .map_err(|_| HermesAgentIntegrationError::InvalidPlan)?;
        if self.model_profiles.snapshot()?.revision != pending.profile_revision {
            return Err(HermesAgentIntegrationError::ModelProfileChanged);
        }
        self.ensure_supported_version(&pending.binary)?;
        self.ensure_files_unchanged(
            pending.config_existed,
            &pending.original_config,
            pending.original_config_digest,
            pending.env_existed,
            &pending.original_env,
            pending.original_env_digest,
        )?;

        let config_mode = if pending.config_existed {
            managed_file_mode(&self.paths.config_path)?
        } else {
            0o600
        };
        let env_mode = if pending.env_existed {
            managed_file_mode(&self.paths.environment_path)?
        } else {
            0o600
        };
        let backup = if pending.config_existed {
            let path = managed_backup_path(&self.paths.config_path);
            write_new_managed_file(&path, &pending.original_config, config_mode)?;
            Some(path)
        } else {
            None
        };

        if let Err(error) =
            atomic_write_managed_file(&self.paths.environment_path, &pending.patched_env, 0o600)
        {
            if let Some(backup) = backup.as_deref() {
                let _ = fs::remove_file(backup);
            }
            return Err(error.into());
        }
        if let Err(error) = atomic_write_managed_file(
            &self.paths.config_path,
            &pending.patched_config,
            config_mode,
        ) {
            rollback_configuration_files(self, &pending, config_mode, env_mode, backup.as_deref());
            return Err(error.into());
        }
        if !self.verify_configuration(&pending.fragment, &pending.plaintext_key)
            || self
                .config_tool
                .show_config(
                    &pending.binary,
                    &self.command_environment(&self.paths.hermes_directory),
                )
                .is_err()
        {
            rollback_configuration_files(self, &pending, config_mode, env_mode, backup.as_deref());
            return Err(HermesAgentIntegrationError::VerificationFailed);
        }

        let credential = stored_client_credential(
            HERMES_AGENT_INTEGRATION.credential_id,
            HERMES_AGENT_INTEGRATION.client_app_id,
            HERMES_AGENT_INTEGRATION.display_name,
            &pending.plaintext_key,
        )?;
        if let Err(error) = self.credentials.upsert(credential.clone()) {
            rollback_configuration_files(self, &pending, config_mode, env_mode, backup.as_deref());
            return Err(error.into());
        }
        let now = now_ms();
        let integration = ManagedIntegrationRecord {
            id: HERMES_AGENT_INTEGRATION.integration_id.to_owned(),
            kind: "hermes-model-provider:chat_completions".to_owned(),
            config_path: self.paths.config_path.to_string_lossy().into_owned(),
            credential_path: self.paths.environment_path.to_string_lossy().into_owned(),
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
            rollback_configuration_files(self, &pending, config_mode, env_mode, backup.as_deref());
            return Err(error.into());
        }

        tracing::info!(
            integration_id = HERMES_AGENT_INTEGRATION.integration_id,
            action = "configure",
            result = "succeeded",
            model_profile_revision = %pending.profile_revision,
            "external_agent_configuration_applied"
        );
        Ok(ExternalAgentConfigurationResult {
            configured: true,
            integration_id: HERMES_AGENT_INTEGRATION.integration_id.to_owned(),
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
    ) -> Result<ExternalAgentDisconnectPlan, HermesAgentIntegrationError> {
        let binary = find_binary(&self.paths.binary_candidates)
            .ok_or(HermesAgentIntegrationError::NotInstalled)?;
        self.ensure_supported_version(&binary)?;
        reject_managed_file_symlink(&self.paths.config_path)?;
        reject_managed_file_symlink(&self.paths.environment_path)?;
        let integration = self
            .database
            .managed_integration(HERMES_AGENT_INTEGRATION.integration_id)?
            .ok_or(HermesAgentIntegrationError::NotConfigured)?;
        if Path::new(&integration.config_path) != self.paths.config_path
            || Path::new(&integration.credential_path) != self.paths.environment_path
        {
            return Err(HermesAgentIntegrationError::ManagedProviderModified);
        }
        let (files, config, _) = self.read_current_files()?;
        let fragment = managed_provider(&config)?
            .ok_or(HermesAgentIntegrationError::ManagedProviderModified)?;
        if value_hash(fragment) != integration.managed_fragment_hash {
            return Err(HermesAgentIntegrationError::ManagedProviderModified);
        }
        let plaintext_key = read_env_credential(&files.environment)?;
        let credential = self
            .stored_credential()?
            .ok_or(HermesAgentIntegrationError::ManagedCredentialModified)?;
        if hash_client_key(&plaintext_key) != credential.key_hash
            || !env_entry_is_canonical(&files.environment, &plaintext_key)
        {
            return Err(HermesAgentIntegrationError::ManagedCredentialModified);
        }
        let patched_config = serialize_yaml_config(&remove_managed_provider(&config)?)?;
        let patched_env = remove_managed_env(&files.environment)?;
        self.validate_staged(&binary, &patched_config, &patched_env, false)?;
        let resources = self
            .database
            .managed_integration_resources(HERMES_AGENT_INTEGRATION.integration_id)?;
        let pending = PendingDisconnect {
            binary,
            original_config_digest: managed_content_hash(&files.config),
            original_config: files.config,
            patched_config,
            original_env_digest: managed_content_hash(&files.environment),
            original_env: files.environment,
            patched_env,
            credential_digest: credential.key_hash,
            integration,
            resources,
            credential,
        };
        let ticket = self
            .pending_disconnect
            .replace(pending)
            .map_err(|_| HermesAgentIntegrationError::InvalidPlan)?;

        Ok(ExternalAgentDisconnectPlan {
            plan_id: ticket.plan_id,
            integration_id: HERMES_AGENT_INTEGRATION.integration_id.to_owned(),
            expires_at_ms: ticket.expires_at_ms,
            config_path: display_path(&self.paths.config_path, &self.paths.home_directory),
            credential_path: display_path(&self.paths.environment_path, &self.paths.home_directory),
            changes: vec![
                ExternalAgentManagedChange {
                    path: "providers.hal100".to_owned(),
                    action: ExternalAgentManagedChangeAction::RemoveManagedFragment,
                },
                ExternalAgentManagedChange {
                    path: HERMES_CREDENTIAL_ENV_KEY.to_owned(),
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
    ) -> Result<bool, HermesAgentIntegrationError> {
        self.pending_disconnect
            .discard(plan_id)
            .map_err(|_| HermesAgentIntegrationError::InvalidPlan)
    }

    pub fn apply_disconnection(
        &self,
        plan_id: &str,
    ) -> Result<ExternalAgentDisconnectResult, HermesAgentIntegrationError> {
        let pending = self
            .pending_disconnect
            .take(plan_id)
            .map_err(|_| HermesAgentIntegrationError::InvalidPlan)?;
        self.ensure_supported_version(&pending.binary)?;
        let current_config = read_managed_file(&self.paths.config_path, MAX_CONFIG_BYTES)?;
        let current_env = read_managed_file(&self.paths.environment_path, MAX_ENV_BYTES)?;
        if managed_content_hash(&current_config) != pending.original_config_digest
            || managed_content_hash(&current_env) != pending.original_env_digest
            || hash_client_key(&read_env_credential(&current_env)?) != pending.credential_digest
        {
            return Err(HermesAgentIntegrationError::ChangedAfterPreview);
        }
        let config_mode = managed_file_mode(&self.paths.config_path)?;
        let env_mode = managed_file_mode(&self.paths.environment_path)?;
        let backup = managed_backup_path(&self.paths.config_path);
        write_new_managed_file(&backup, &pending.original_config, config_mode)?;

        if let Err(error) = atomic_write_managed_file(
            &self.paths.config_path,
            &pending.patched_config,
            config_mode,
        ) {
            let _ = fs::remove_file(&backup);
            return Err(error.into());
        }
        if let Err(error) = self.write_or_remove_environment(&pending.patched_env) {
            rollback_disconnection_files(self, &pending, config_mode, env_mode, &backup);
            return Err(error);
        }
        if !self.verify_disconnected()
            || self
                .config_tool
                .show_config(
                    &pending.binary,
                    &self.command_environment(&self.paths.hermes_directory),
                )
                .is_err()
        {
            rollback_disconnection_files(self, &pending, config_mode, env_mode, &backup);
            return Err(HermesAgentIntegrationError::VerificationFailed);
        }
        if let Err(error) = self
            .credentials
            .remove_client(HERMES_AGENT_INTEGRATION.client_app_id)
        {
            rollback_disconnection_files(self, &pending, config_mode, env_mode, &backup);
            return Err(error.into());
        }
        let database_result = self.database.remove_managed_integration_and_client(
            HERMES_AGENT_INTEGRATION.integration_id,
            HERMES_AGENT_INTEGRATION.client_app_id,
            now_ms(),
        );
        if !matches!(database_result.as_ref(), Ok(true)) {
            rollback_disconnection_files(self, &pending, config_mode, env_mode, &backup);
            let _ = self.credentials.upsert(pending.credential.clone());
            let _ = self.database.upsert_integration_resources_and_credential(
                &pending.integration,
                &pending.resources,
                &pending.credential,
            );
            return match database_result {
                Ok(false) => Err(HermesAgentIntegrationError::NotConfigured),
                Err(error) => Err(error.into()),
                Ok(true) => unreachable!(),
            };
        }

        tracing::info!(
            integration_id = HERMES_AGENT_INTEGRATION.integration_id,
            action = "disconnect",
            result = "succeeded",
            "external_agent_disconnected"
        );
        Ok(ExternalAgentDisconnectResult {
            disconnected: true,
            integration_id: HERMES_AGENT_INTEGRATION.integration_id.to_owned(),
            config_path: display_path(&self.paths.config_path, &self.paths.home_directory),
            backup_path: Some(display_path(&backup, &self.paths.home_directory)),
            credential_revoked: true,
        })
    }

    fn integration_state(
        &self,
        config: &Value,
        env: &[u8],
        prior: Option<&ManagedIntegrationRecord>,
        profile: &ExternalAgentModelProfile,
    ) -> Result<ExternalAgentIntegrationState, HermesAgentIntegrationError> {
        if prior.is_some_and(|record| {
            Path::new(&record.config_path) != self.paths.config_path
                || Path::new(&record.credential_path) != self.paths.environment_path
        }) {
            return Ok(ExternalAgentIntegrationState::ModifiedOutsideHal100);
        }
        let fragment = managed_provider(config)?;
        let env_value = read_managed_env_value(env)?;
        match (fragment, env_value, prior) {
            (None, None, None) => Ok(ExternalAgentIntegrationState::InstalledNotConfigured),
            (Some(_), _, None) | (None, Some(_), None) => {
                Ok(ExternalAgentIntegrationState::Conflict)
            }
            (None, _, Some(_)) | (_, None, Some(_)) => {
                Ok(ExternalAgentIntegrationState::ModifiedOutsideHal100)
            }
            (Some(fragment), Some(key), Some(record)) => {
                if value_hash(fragment) != record.managed_fragment_hash
                    || !env_entry_is_canonical(env, &key)
                    || !self.credential_is_owned_and_valid(&key)
                {
                    return Ok(ExternalAgentIntegrationState::ModifiedOutsideHal100);
                }
                let expected = provider_fragment(&self.gateway_base_url, profile);
                if expected != *fragment {
                    Ok(ExternalAgentIntegrationState::NeedsRefresh)
                } else {
                    Ok(ExternalAgentIntegrationState::Configured)
                }
            }
        }
    }

    fn read_current_files(
        &self,
    ) -> Result<(CurrentFiles, Value, Vec<u8>), HermesAgentIntegrationError> {
        let config_existed = self.paths.config_path.exists();
        let env_existed = self.paths.environment_path.exists();
        let config = if config_existed {
            read_managed_file(&self.paths.config_path, MAX_CONFIG_BYTES)?
        } else {
            b"{}\n".to_vec()
        };
        let environment = if env_existed {
            read_managed_file(&self.paths.environment_path, MAX_ENV_BYTES)?
        } else {
            Vec::new()
        };
        let parsed = parse_yaml_config(&config)?;
        Ok((
            CurrentFiles {
                config,
                environment: environment.clone(),
                config_existed,
                env_existed,
            },
            parsed,
            environment,
        ))
    }

    fn ensure_supported_version(&self, binary: &Path) -> Result<(), HermesAgentIntegrationError> {
        let version = self
            .config_tool
            .version(
                binary,
                &self.command_environment(&self.paths.hermes_directory),
            )
            .map_err(HermesAgentIntegrationError::ConfigTool)?;
        if stable_version_triplet(&version).is_some_and(|version| version >= MIN_TESTED_VERSION) {
            Ok(())
        } else {
            Err(HermesAgentIntegrationError::UnsupportedVersion)
        }
    }

    fn ensure_compatible_profile(
        &self,
        profile: &ExternalAgentModelProfile,
    ) -> Result<(), HermesAgentIntegrationError> {
        if profile.context_window_tokens < MIN_CONTEXT_WINDOW_TOKENS {
            Err(HermesAgentIntegrationError::IncompatibleModelProfile)
        } else {
            Ok(())
        }
    }

    fn ensure_files_unchanged(
        &self,
        config_existed: bool,
        original_config: &[u8],
        config_digest: [u8; 32],
        env_existed: bool,
        original_env: &[u8],
        env_digest: [u8; 32],
    ) -> Result<(), HermesAgentIntegrationError> {
        let current_config = current_bytes(
            &self.paths.config_path,
            config_existed,
            original_config,
            MAX_CONFIG_BYTES,
        )?;
        let current_env = current_bytes(
            &self.paths.environment_path,
            env_existed,
            original_env,
            MAX_ENV_BYTES,
        )?;
        if managed_content_hash(&current_config) != config_digest
            || managed_content_hash(&current_env) != env_digest
        {
            return Err(HermesAgentIntegrationError::ChangedAfterPreview);
        }
        Ok(())
    }

    fn validate_staged(
        &self,
        binary: &Path,
        config: &[u8],
        env: &[u8],
        expect_provider: bool,
    ) -> Result<(), HermesAgentIntegrationError> {
        fs::create_dir_all(&self.paths.validation_root)?;
        let validation_home = self
            .paths
            .validation_root
            .join(format!("plan-{}", Uuid::new_v4()));
        fs::create_dir(&validation_home)?;
        let result = (|| {
            atomic_write_managed_file(&validation_home.join("config.yaml"), config, 0o600)?;
            atomic_write_managed_file(&validation_home.join(".env"), env, 0o600)?;
            let output = self
                .config_tool
                .show_config(binary, &self.command_environment(&validation_home))
                .map_err(HermesAgentIntegrationError::ConfigTool)?;
            if expect_provider && !output.to_ascii_lowercase().contains(HERMES_PROVIDER_KEY) {
                return Err(HermesAgentIntegrationError::VerificationFailed);
            }
            Ok(())
        })();
        let _ = fs::remove_dir_all(&validation_home);
        result
    }

    fn verify_configuration(&self, expected: &Value, plaintext_key: &str) -> bool {
        let Ok(config) = read_managed_file(&self.paths.config_path, MAX_CONFIG_BYTES) else {
            return false;
        };
        let Ok(config) = parse_yaml_config(&config) else {
            return false;
        };
        let Ok(fragment) = managed_provider(&config) else {
            return false;
        };
        let Ok(env) = read_managed_file(&self.paths.environment_path, MAX_ENV_BYTES) else {
            return false;
        };
        fragment.is_some_and(|fragment| fragment == expected)
            && env_entry_is_canonical(&env, plaintext_key)
            && managed_file_mode(&self.paths.environment_path).is_ok_and(|mode| mode & 0o077 == 0)
    }

    fn verify_disconnected(&self) -> bool {
        let Ok(config) = read_managed_file(&self.paths.config_path, MAX_CONFIG_BYTES) else {
            return false;
        };
        let Ok(config) = parse_yaml_config(&config) else {
            return false;
        };
        let provider_absent = managed_provider(&config).is_ok_and(|provider| provider.is_none());
        let env_absent = if self.paths.environment_path.exists() {
            read_managed_file(&self.paths.environment_path, MAX_ENV_BYTES)
                .ok()
                .and_then(|env| read_managed_env_value(&env).ok())
                .flatten()
                .is_none()
        } else {
            true
        };
        provider_absent && env_absent
    }

    fn write_or_remove_environment(
        &self,
        contents: &[u8],
    ) -> Result<(), HermesAgentIntegrationError> {
        if contents.is_empty() {
            fs::remove_file(&self.paths.environment_path)?;
            if let Some(parent) = self.paths.environment_path.parent() {
                sync_managed_directory(parent)?;
            }
        } else {
            atomic_write_managed_file(&self.paths.environment_path, contents, 0o600)?;
        }
        Ok(())
    }

    fn credential_is_owned_and_valid(&self, key: &str) -> bool {
        self.stored_credential()
            .ok()
            .flatten()
            .is_some_and(|credential| hash_client_key(key) == credential.key_hash)
            && managed_file_mode(&self.paths.environment_path).is_ok_and(|mode| mode & 0o077 == 0)
    }

    fn stored_credential(
        &self,
    ) -> Result<Option<StoredClientCredential>, HermesAgentIntegrationError> {
        Ok(self
            .database
            .load_client_credentials()?
            .into_iter()
            .find(|credential| {
                credential.key_id == HERMES_AGENT_INTEGRATION.credential_id
                    && credential.client_app_id == HERMES_AGENT_INTEGRATION.client_app_id
            }))
    }

    fn command_environment(&self, hermes_home: &Path) -> Vec<(String, OsString)> {
        vec![
            (
                "HOME".to_owned(),
                self.paths.home_directory.as_os_str().to_owned(),
            ),
            (
                "HERMES_REAL_HOME".to_owned(),
                self.paths.home_directory.as_os_str().to_owned(),
            ),
            ("HERMES_HOME".to_owned(), hermes_home.as_os_str().to_owned()),
            ("NO_COLOR".to_owned(), OsString::from("1")),
            ("TERM".to_owned(), OsString::from("dumb")),
            ("CI".to_owned(), OsString::from("1")),
        ]
    }

    fn profile_warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        if let Some(custom) = std::env::var_os("HERMES_HOME") {
            let custom = PathBuf::from(custom);
            if custom != self.paths.hermes_directory {
                warnings.push(format!(
                    "检测到HERMES_HOME={}；HAL100当前只管理default Profile {}",
                    custom.to_string_lossy(),
                    display_path(&self.paths.hermes_directory, &self.paths.home_directory)
                ));
            }
        }
        let active_profile = self.paths.hermes_directory.join("active_profile");
        if let Ok(bytes) = read_managed_file(&active_profile, 256)
            && let Ok(name) = std::str::from_utf8(&bytes)
        {
            let name = name.trim();
            if !name.is_empty() && name != "default" {
                warnings.push(format!(
                    "Hermes当前粘性Profile为{name}；HAL100接入位于default，调用时可显式使用-p default"
                ));
            }
        }
        warnings
    }
}

struct CurrentFiles {
    config: Vec<u8>,
    environment: Vec<u8>,
    config_existed: bool,
    env_existed: bool,
}

fn current_bytes(
    path: &Path,
    existed: bool,
    original: &[u8],
    max_bytes: u64,
) -> Result<Vec<u8>, HermesAgentIntegrationError> {
    if path.exists() {
        read_managed_file(path, max_bytes).map_err(Into::into)
    } else if existed {
        Err(HermesAgentIntegrationError::ChangedAfterPreview)
    } else {
        Ok(original.to_vec())
    }
}

fn read_env_credential(bytes: &[u8]) -> Result<String, HermesAgentIntegrationError> {
    let key = read_managed_env_value(bytes)?
        .ok_or(HermesAgentIntegrationError::ManagedCredentialModified)?;
    if !(24..=256).contains(&key.len()) || key.contains(['\n', '\r']) {
        return Err(HermesAgentIntegrationError::InvalidCredentialVariable);
    }
    Ok(key)
}

fn rollback_configuration_files(
    adapter: &HermesAgentIntegrationAdapter,
    pending: &PendingConfiguration,
    config_mode: u32,
    env_mode: u32,
    backup: Option<&Path>,
) {
    restore_file(
        &adapter.paths.config_path,
        pending.config_existed,
        &pending.original_config,
        config_mode,
    );
    restore_file(
        &adapter.paths.environment_path,
        pending.env_existed,
        &pending.original_env,
        env_mode,
    );
    if let Some(backup) = backup {
        let _ = fs::remove_file(backup);
    }
}

fn rollback_disconnection_files(
    adapter: &HermesAgentIntegrationAdapter,
    pending: &PendingDisconnect,
    config_mode: u32,
    env_mode: u32,
    backup: &Path,
) {
    let _ = atomic_write_managed_file(
        &adapter.paths.config_path,
        &pending.original_config,
        config_mode,
    );
    let _ = atomic_write_managed_file(
        &adapter.paths.environment_path,
        &pending.original_env,
        env_mode,
    );
    let _ = fs::remove_file(backup);
}

fn restore_file(path: &Path, existed: bool, contents: &[u8], mode: u32) {
    if existed {
        let _ = atomic_write_managed_file(path, contents, mode);
    } else {
        let _ = fs::remove_file(path);
    }
}

fn rollback_registry(
    adapter: &HermesAgentIntegrationAdapter,
    prior: Option<StoredClientCredential>,
) {
    let _ = adapter
        .credentials
        .remove_client(HERMES_AGENT_INTEGRATION.client_app_id);
    if let Some(prior) = prior {
        let _ = adapter.credentials.upsert(prior);
    }
}

fn generate_client_key() -> String {
    format!(
        "hal100_hermes_{}{}",
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

fn stable_version_triplet(version: &str) -> Option<(u64, u64, u64)> {
    version.split_whitespace().find_map(|token| {
        let candidate = token
            .trim_start_matches(|character: char| !character.is_ascii_digit())
            .trim_end_matches(|character: char| !character.is_ascii_digit());
        let stable = candidate
            .split_once('-')
            .map_or(candidate, |(stable, _)| stable);
        let mut parts = stable.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        parts.next().is_none().then_some((major, minor, patch))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    struct TestDirectory(PathBuf);

    #[cfg(unix)]
    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("hal100-hermes-{}", Uuid::new_v4()));
            fs::create_dir(&path).expect("create test directory");
            Self(path)
        }
    }

    #[cfg(unix)]
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    struct Fixture {
        root: TestDirectory,
        adapter: HermesAgentIntegrationAdapter,
        database: Arc<Database>,
        credentials: CredentialRegistry,
        config_path: PathBuf,
        environment_path: PathBuf,
    }

    #[cfg(unix)]
    impl Fixture {
        fn new() -> Self {
            let root = TestDirectory::new();
            let binary = root.0.join("bin/hermes");
            fs::create_dir_all(binary.parent().expect("binary parent")).expect("binary directory");
            fs::write(
                &binary,
                "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  printf 'Hermes Agent v0.18.2 (2026.7.7.2)\\n'\nelse\n  /bin/cat \"$HERMES_HOME/config.yaml\"\nfi\n",
            )
            .expect("fake Hermes CLI");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&binary, fs::Permissions::from_mode(0o700))
                    .expect("fake Hermes executable mode");
            }
            let database =
                Arc::new(Database::open(root.0.join("hal100.sqlite")).expect("database"));
            let credentials = CredentialRegistry::new(Vec::new());
            let hermes_directory = root.0.join(".hermes");
            let config_path = hermes_directory.join("config.yaml");
            let environment_path = hermes_directory.join(".env");
            let paths = HermesAgentPaths {
                home_directory: root.0.clone(),
                hermes_directory,
                config_path: config_path.clone(),
                environment_path: environment_path.clone(),
                validation_root: root.0.join("app-data/temporary/hermes-validation"),
                binary_candidates: vec![binary],
            };
            let adapter = HermesAgentIntegrationAdapter::new(
                database.clone(),
                credentials.clone(),
                ExternalModelProfileRegistry::new(hermes_capable_profile())
                    .expect("Hermes-capable profile"),
                paths,
            );
            Self {
                root,
                adapter,
                database,
                credentials,
                config_path,
                environment_path,
            }
        }

        fn write_config(&self, source: &str) {
            fs::create_dir_all(self.config_path.parent().expect("config parent"))
                .expect("config directory");
            fs::write(&self.config_path, source).expect("write config");
        }

        fn write_env(&self, source: &str) {
            fs::create_dir_all(self.environment_path.parent().expect("env parent"))
                .expect("env directory");
            fs::write(&self.environment_path, source).expect("write env");
        }
    }

    #[cfg(unix)]
    fn hermes_capable_profile() -> ExternalAgentModelProfile {
        ExternalAgentModelProfile {
            model_id: "hal100-active".to_owned(),
            display_name: "HAL100 Hermes测试模型".to_owned(),
            context_window_tokens: 65_536,
            max_output_tokens: 4_096,
            input_modalities: vec![hal100_protocol::ExternalAgentInputModality::Text],
            supports_tools: true,
            supports_reasoning: false,
            revision: "hermes-test-route-v1".to_owned(),
        }
    }

    #[cfg(unix)]
    #[test]
    fn configuration_preserves_defaults_and_other_secrets() {
        let fixture = Fixture::new();
        fixture.write_config(
            "model:\n  default: personal-model\n  provider: openrouter\nproviders:\n  personal:\n    api: https://example.test/v1\n",
        );
        fixture.write_env("OPENROUTER_API_KEY=personal-secret\n");
        let plan = fixture.adapter.plan_configuration().expect("plan");
        assert!(plan.preserves_default_model);
        let result = fixture
            .adapter
            .apply_configuration(&plan.plan_id)
            .expect("apply");

        let config =
            parse_yaml_config(&fs::read(&fixture.config_path).expect("config")).expect("parse");
        assert_eq!(config["model"]["default"], "personal-model");
        assert_eq!(config["model"]["provider"], "openrouter");
        assert_eq!(
            config["providers"]["personal"]["api"],
            "https://example.test/v1"
        );
        assert_eq!(
            config["providers"]["hal100"]["key_env"],
            HERMES_CREDENTIAL_ENV_KEY
        );
        let env = fs::read_to_string(&fixture.environment_path).expect("env");
        assert!(env.contains("OPENROUTER_API_KEY=personal-secret"));
        let key = read_env_credential(env.as_bytes()).expect("key");
        assert!(
            !fs::read_to_string(&fixture.config_path)
                .expect("config source")
                .contains(&key)
        );
        assert!(
            fixture
                .credentials
                .authenticate(&key)
                .is_some_and(|client| {
                    client.client_app_id == HERMES_AGENT_INTEGRATION.client_app_id
                })
        );
        assert!(result.backup_path.is_some());
        assert_eq!(
            fixture
                .database
                .managed_integration_resources(HERMES_AGENT_INTEGRATION.integration_id)
                .expect("resources")
                .len(),
            2
        );
        assert!(fixture.root.0.exists());
    }

    #[cfg(unix)]
    #[test]
    fn disconnection_removes_only_hal100_fragments_and_revokes_its_key() {
        let fixture = Fixture::new();
        fixture.write_config(
            "providers:\n  personal:\n    api: https://example.test/v1\nmodel:\n  default: personal-model\n",
        );
        fixture.write_env("PERSONAL_KEY=keep-me\n");
        let configure = fixture.adapter.plan_configuration().expect("plan");
        fixture
            .adapter
            .apply_configuration(&configure.plan_id)
            .expect("configure");
        let key =
            read_env_credential(&fs::read(&fixture.environment_path).expect("configured env"))
                .expect("key");
        let disconnect = fixture
            .adapter
            .plan_disconnection()
            .expect("disconnect plan");
        fixture
            .adapter
            .apply_disconnection(&disconnect.plan_id)
            .expect("disconnect");

        let config =
            parse_yaml_config(&fs::read(&fixture.config_path).expect("config")).expect("parse");
        assert!(config["providers"].get("hal100").is_none());
        assert_eq!(config["model"]["default"], "personal-model");
        assert_eq!(
            fs::read_to_string(&fixture.environment_path).expect("env"),
            "PERSONAL_KEY=keep-me\n"
        );
        assert!(fixture.credentials.authenticate(&key).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn foreign_fragment_and_external_modification_are_never_overwritten() {
        let fixture = Fixture::new();
        fixture.write_config("providers:\n  hal100:\n    api: https://foreign.test/v1\n");
        assert!(matches!(
            fixture.adapter.plan_configuration(),
            Err(HermesAgentIntegrationError::Config(message))
                if message.contains("providers.hal100")
        ));

        let fixture = Fixture::new();
        fixture.write_config("{}\n");
        let plan = fixture.adapter.plan_configuration().expect("plan");
        fixture
            .adapter
            .apply_configuration(&plan.plan_id)
            .expect("apply");
        let mut config =
            parse_yaml_config(&fs::read(&fixture.config_path).expect("config")).expect("parse");
        config["providers"]["hal100"]["api"] =
            Value::String("https://tampered.invalid/v1".to_owned());
        fs::write(
            &fixture.config_path,
            serialize_yaml_config(&config).expect("serialize"),
        )
        .expect("tamper");
        assert_eq!(
            fixture.adapter.detect().expect("detect").integration_state,
            ExternalAgentIntegrationState::ModifiedOutsideHal100
        );
    }

    #[cfg(unix)]
    #[test]
    fn stale_and_discarded_plans_never_mutate_files() {
        let fixture = Fixture::new();
        fixture.write_config("{}\n");
        let plan = fixture.adapter.plan_configuration().expect("plan");
        assert!(
            fixture
                .adapter
                .discard_configuration_plan(&plan.plan_id)
                .expect("discard")
        );
        assert!(matches!(
            fixture.adapter.apply_configuration(&plan.plan_id),
            Err(HermesAgentIntegrationError::InvalidPlan)
        ));
        assert_eq!(
            fs::read_to_string(&fixture.config_path).expect("config"),
            "{}\n"
        );
        assert!(!fixture.environment_path.exists());
    }

    #[test]
    fn parses_official_hermes_version_output() {
        assert_eq!(
            stable_version_triplet("Hermes Agent v0.18.2 (2026.7.7.2)"),
            Some((0, 18, 2))
        );
        assert_eq!(stable_version_triplet("0.19.0-beta.1"), Some((0, 19, 0)));
        assert_eq!(stable_version_triplet("development"), None);
    }

    #[cfg(unix)]
    #[test]
    fn conservative_route_is_reported_as_blocked_instead_of_overstating_context() {
        let fixture = Fixture::new();
        fixture
            .adapter
            .model_profiles
            .replace(
                ExternalModelProfileRegistry::conservative_managed_route()
                    .snapshot()
                    .expect("conservative profile"),
            )
            .expect("replace profile");
        let detection = fixture.adapter.detect().expect("detect");
        assert_eq!(
            detection.integration_state,
            ExternalAgentIntegrationState::Blocked
        );
        assert!(
            detection
                .warnings
                .iter()
                .any(|warning| warning.contains("64000"))
        );
        assert!(matches!(
            fixture.adapter.plan_configuration(),
            Err(HermesAgentIntegrationError::IncompatibleModelProfile)
        ));
    }
}
