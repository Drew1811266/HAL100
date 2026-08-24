use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use hal100_core::PI_CODING_AGENT_INTEGRATION;
use hal100_protocol::{
    ExternalAgentConfigurationChange, ExternalAgentConfigurationPlan,
    ExternalAgentConfigurationResult, ExternalAgentDetection, ExternalAgentDisconnectPlan,
    ExternalAgentDisconnectResult, ExternalAgentGatewayProtocol, ExternalAgentInputModality,
    ExternalAgentIntegrationState, ExternalAgentManagedChange, ExternalAgentManagedChangeAction,
    ExternalAgentModelProfile,
};
use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;

use crate::jsonc_patch::{
    JsoncPatchError, nested_object_member, patch_nested_object_member, remove_nested_object_member,
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
const MAX_CONFIG_BYTES: u64 = 2 * 1024 * 1024;
const MIN_TESTED_PI_VERSION: (u64, u64, u64) = (0, 84, 2);
const PROVIDER_KEY: &str = "hal100";
const PROVIDER_PARENT_KEY: &str = "providers";

#[derive(Debug, Clone)]
pub struct PiCodingAgentPaths {
    pub home_directory: PathBuf,
    pub agent_directory: PathBuf,
    pub config_path: PathBuf,
    pub credential_path: PathBuf,
    pub binary_candidates: Vec<PathBuf>,
}

impl PiCodingAgentPaths {
    pub fn for_macos(home_directory: &Path, app_data_directory: &Path) -> Self {
        let agent_directory = home_directory.join(".pi/agent");
        Self {
            home_directory: home_directory.to_path_buf(),
            config_path: agent_directory.join("models.json"),
            agent_directory,
            credential_path: app_data_directory
                .join("credentials")
                .join("pi-coding-agent-gateway.key"),
            binary_candidates: vec![
                home_directory.join(".local/bin/pi"),
                home_directory.join(".bun/bin/pi"),
                home_directory.join(".npm-global/bin/pi"),
                PathBuf::from("/opt/homebrew/bin/pi"),
                PathBuf::from("/usr/local/bin/pi"),
                app_data_directory
                    .join("external-agents/pi-coding-agent/runtime/node_modules/.bin/pi"),
            ],
        }
    }
}

#[derive(Debug, Error)]
pub enum PiCodingAgentIntegrationError {
    #[error("未检测到官方Pi Coding Agent CLI，请先独立安装Pi后再配置")]
    NotInstalled,
    #[error("Pi Coding Agent版本低于HAL100当前验收下限0.84.2，请先升级")]
    UnsupportedVersion,
    #[error("Pi models.json必须是严格JSON，不能包含注释或尾随逗号: {0}")]
    InvalidStrictJson(String),
    #[error("Pi现有providers.hal100不属于HAL100，已拒绝覆盖")]
    ProviderConflict,
    #[error("HAL100管理的Pi配置已被外部修改，请先检查差异")]
    ManagedProviderModified,
    #[error("HAL100管理的Pi模型能力版本已变化，请重新预览配置")]
    ModelProfileChanged,
    #[error("配置计划不存在、已使用或已经过期")]
    InvalidPlan,
    #[error("确认后Pi配置或凭据发生了变化，请重新预览")]
    ChangedAfterPreview,
    #[error("HAL100 Pi凭据文件已存在但没有对应安装记录")]
    UnownedCredentialFile,
    #[error("HAL100 Pi凭据文件无效或权限不安全")]
    InvalidCredentialFile,
    #[error("Pi Coding Agent尚未由HAL100配置，无可断开的受管接入")]
    NotConfigured,
    #[error("HAL100管理的Pi凭据已被外部修改，请先检查")]
    ManagedCredentialModified,
    #[error("写入后验证失败，已经恢复原配置")]
    VerificationFailed,
    #[error("Pi凭据路径无法安全表示为固定读取命令")]
    UnsafeCredentialPath,
    #[error(transparent)]
    ManagedFile(#[from] ManagedFileError),
    #[error(transparent)]
    Database(#[from] DatabaseError),
    #[error(transparent)]
    Credential(#[from] ClientCredentialError),
    #[error(transparent)]
    ModelProfile(#[from] ModelProfileError),
    #[error(transparent)]
    JsonPatch(#[from] JsoncPatchError),
    #[error("文件操作失败: {0}")]
    Io(#[from] std::io::Error),
}

pub struct PiCodingAgentIntegrationAdapter {
    database: Arc<Database>,
    credentials: CredentialRegistry,
    model_profiles: ExternalModelProfileRegistry,
    paths: PiCodingAgentPaths,
    gateway_base_url: String,
    pending: PendingPlanStore<PendingConfiguration>,
    pending_disconnect: PendingPlanStore<PendingDisconnect>,
}

struct PendingConfiguration {
    original_digest: [u8; 32],
    original: Vec<u8>,
    config_existed: bool,
    patched: Vec<u8>,
    fragment: Value,
    fragment_hash: [u8; 32],
    plaintext_key: String,
    create_credential_file: bool,
    profile_revision: String,
    prior_integration: Option<ManagedIntegrationRecord>,
    prior_credential: Option<StoredClientCredential>,
}

struct PendingDisconnect {
    original_digest: [u8; 32],
    original: Vec<u8>,
    patched: Vec<u8>,
    credential_digest: [u8; 32],
    plaintext_key: String,
    integration: ManagedIntegrationRecord,
    resources: Vec<ManagedIntegrationResourceRecord>,
    credential: StoredClientCredential,
}

impl PiCodingAgentIntegrationAdapter {
    pub fn new(
        database: Arc<Database>,
        credentials: CredentialRegistry,
        model_profiles: ExternalModelProfileRegistry,
        paths: PiCodingAgentPaths,
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
        paths: PiCodingAgentPaths,
        gateway_base_url: String,
    ) -> Self {
        Self {
            database,
            credentials,
            model_profiles,
            paths,
            gateway_base_url,
            pending: PendingPlanStore::new(PLAN_TTL),
            pending_disconnect: PendingPlanStore::new(PLAN_TTL),
        }
    }

    pub fn detect(&self) -> Result<ExternalAgentDetection, PiCodingAgentIntegrationError> {
        let profile = self.model_profiles.snapshot()?;
        let binary = find_binary(&self.paths.binary_candidates);
        let version = binary.as_ref().and_then(|path| pi_version(path));
        let version_supported = version
            .as_deref()
            .and_then(stable_version_triplet)
            .is_none_or(|version| version >= MIN_TESTED_PI_VERSION);
        let prior = self
            .database
            .managed_integration(PI_CODING_AGENT_INTEGRATION.integration_id)?;
        let mut warnings = Vec::new();
        self.add_path_warnings(&mut warnings);

        if binary.is_some() && version.is_none() {
            warnings.push("检测到Pi CLI，但无法在受限环境中读取版本".to_owned());
        }
        if !version_supported {
            warnings.push("该Pi版本早于HAL100当前自动验收下限0.84.2".to_owned());
        }

        let config_exists = self.paths.config_path.exists();
        let state = if config_exists {
            match self.read_strict_config() {
                Ok((_, config)) => self.integration_state(&config, prior.as_ref(), &profile),
                Err(error) => {
                    warnings.push(format!("models.json无法安全解析：{error}"));
                    ExternalAgentIntegrationState::Conflict
                }
            }
        } else if prior.is_some() {
            warnings.push("HAL100安装记录存在，但Pi models.json已不存在".to_owned());
            ExternalAgentIntegrationState::ModifiedOutsideHal100
        } else if binary.is_some() {
            ExternalAgentIntegrationState::InstalledNotConfigured
        } else {
            ExternalAgentIntegrationState::NotInstalled
        };
        let state = if !version_supported
            && state == ExternalAgentIntegrationState::InstalledNotConfigured
        {
            ExternalAgentIntegrationState::UnsupportedVersion
        } else {
            state
        };

        Ok(ExternalAgentDetection {
            integration_id: PI_CODING_AGENT_INTEGRATION.integration_id.to_owned(),
            display_name: PI_CODING_AGENT_INTEGRATION.display_name.to_owned(),
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
    ) -> Result<ExternalAgentConfigurationPlan, PiCodingAgentIntegrationError> {
        let binary = find_binary(&self.paths.binary_candidates)
            .ok_or(PiCodingAgentIntegrationError::NotInstalled)?;
        let version =
            pi_version(&binary).ok_or(PiCodingAgentIntegrationError::UnsupportedVersion)?;
        if stable_version_triplet(&version).is_some_and(|version| version < MIN_TESTED_PI_VERSION) {
            return Err(PiCodingAgentIntegrationError::UnsupportedVersion);
        }
        reject_managed_file_symlink(&self.paths.config_path)?;
        reject_managed_file_symlink(&self.paths.credential_path)?;
        let profile = self.model_profiles.snapshot()?;
        let config_existed = self.paths.config_path.exists();
        let original = if config_existed {
            read_managed_file(&self.paths.config_path, MAX_CONFIG_BYTES)?
        } else {
            b"{\n  \"providers\": {}\n}\n".to_vec()
        };
        let source = strict_json_source(&original)?;
        let config: Value = serde_json::from_str(source)
            .map_err(|error| PiCodingAgentIntegrationError::InvalidStrictJson(error.to_string()))?;
        if !config.is_object() {
            return Err(JsoncPatchError::RootMustBeObject.into());
        }
        let prior = self
            .database
            .managed_integration(PI_CODING_AGENT_INTEGRATION.integration_id)?;
        let state = self.integration_state(&config, prior.as_ref(), &profile);
        match state {
            ExternalAgentIntegrationState::Conflict => {
                return Err(PiCodingAgentIntegrationError::ProviderConflict);
            }
            ExternalAgentIntegrationState::ModifiedOutsideHal100 => {
                return Err(PiCodingAgentIntegrationError::ManagedProviderModified);
            }
            _ => {}
        }

        let prior_credential = self.stored_credential()?;
        let (plaintext_key, create_credential_file) = if prior.is_some() {
            let key = read_credential(&self.paths.credential_path)?;
            let credential = prior_credential
                .as_ref()
                .ok_or(PiCodingAgentIntegrationError::ManagedCredentialModified)?;
            if hash_client_key(&key) != credential.key_hash {
                return Err(PiCodingAgentIntegrationError::ManagedCredentialModified);
            }
            (key, false)
        } else {
            if self.paths.credential_path.exists() || prior_credential.is_some() {
                return Err(PiCodingAgentIntegrationError::UnownedCredentialFile);
            }
            (generate_client_key(), true)
        };
        let credential_command = credential_command(&self.paths.credential_path)?;
        let fragment = provider_fragment(&credential_command, &self.gateway_base_url, &profile);
        let fragment_hash = value_hash(&fragment);
        let patch = patch_nested_object_member(
            source,
            PROVIDER_PARENT_KEY,
            PROVIDER_KEY,
            &fragment,
            matches!(
                state,
                ExternalAgentIntegrationState::Configured
                    | ExternalAgentIntegrationState::NeedsRefresh
            ),
        )?;
        let pending = PendingConfiguration {
            original_digest: managed_content_hash(&original),
            original,
            config_existed,
            patched: patch.output.into_bytes(),
            fragment,
            fragment_hash,
            plaintext_key,
            create_credential_file,
            profile_revision: profile.revision.clone(),
            prior_integration: prior,
            prior_credential,
        };
        let ticket = self
            .pending
            .replace(pending)
            .map_err(|_| PiCodingAgentIntegrationError::InvalidPlan)?;

        Ok(ExternalAgentConfigurationPlan {
            plan_id: ticket.plan_id,
            integration_id: PI_CODING_AGENT_INTEGRATION.integration_id.to_owned(),
            expires_at_ms: ticket.expires_at_ms,
            config_path: display_path(&self.paths.config_path, &self.paths.home_directory),
            credential_path: display_path(&self.paths.credential_path, &self.paths.home_directory),
            changes: vec![
                ExternalAgentConfigurationChange {
                    path: "providers.hal100.baseUrl".to_owned(),
                    value: self.gateway_base_url.clone(),
                },
                ExternalAgentConfigurationChange {
                    path: "providers.hal100.api".to_owned(),
                    value: "openai-completions".to_owned(),
                },
                ExternalAgentConfigurationChange {
                    path: "providers.hal100.apiKey".to_owned(),
                    value: "固定/bin/cat命令读取独立0600凭据（内容不显示）".to_owned(),
                },
                ExternalAgentConfigurationChange {
                    path: "providers.hal100.models[hal100-active]".to_owned(),
                    value: format!(
                        "{} · 上下文{} · 最大输出{}",
                        profile.display_name,
                        profile.context_window_tokens,
                        profile.max_output_tokens
                    ),
                },
            ],
            gateway_protocol: ExternalAgentGatewayProtocol::OpenAiChatCompletions,
            creates_backup: config_existed,
            preserves_default_model: true,
            requires_confirmation: true,
            model_profile_revision: profile.revision,
            warnings: Vec::new(),
        })
    }

    pub fn discard_configuration_plan(
        &self,
        plan_id: &str,
    ) -> Result<bool, PiCodingAgentIntegrationError> {
        self.pending
            .discard(plan_id)
            .map_err(|_| PiCodingAgentIntegrationError::InvalidPlan)
    }

    pub fn apply_configuration(
        &self,
        plan_id: &str,
    ) -> Result<ExternalAgentConfigurationResult, PiCodingAgentIntegrationError> {
        let pending = self
            .pending
            .take(plan_id)
            .map_err(|_| PiCodingAgentIntegrationError::InvalidPlan)?;
        if self.model_profiles.snapshot()?.revision != pending.profile_revision {
            return Err(PiCodingAgentIntegrationError::ModelProfileChanged);
        }
        let current = if self.paths.config_path.exists() {
            read_managed_file(&self.paths.config_path, MAX_CONFIG_BYTES)?
        } else if pending.config_existed {
            return Err(PiCodingAgentIntegrationError::ChangedAfterPreview);
        } else {
            pending.original.clone()
        };
        if managed_content_hash(&current) != pending.original_digest {
            return Err(PiCodingAgentIntegrationError::ChangedAfterPreview);
        }
        if !pending.create_credential_file {
            let current_key = read_credential(&self.paths.credential_path)?;
            if current_key != pending.plaintext_key {
                return Err(PiCodingAgentIntegrationError::ChangedAfterPreview);
            }
        }

        let config_mode = if pending.config_existed {
            managed_file_mode(&self.paths.config_path)?
        } else {
            0o600
        };
        let backup = if pending.config_existed {
            let path = managed_backup_path(&self.paths.config_path);
            write_new_managed_file(&path, &pending.original, config_mode)?;
            Some(path)
        } else {
            None
        };
        let mut credential_created = false;
        if pending.create_credential_file {
            atomic_write_managed_file(
                &self.paths.credential_path,
                pending.plaintext_key.as_bytes(),
                0o600,
            )?;
            credential_created = true;
        }
        if let Err(error) =
            atomic_write_managed_file(&self.paths.config_path, &pending.patched, config_mode)
        {
            rollback_configuration_files(
                self,
                &pending,
                config_mode,
                credential_created,
                backup.as_deref(),
            );
            return Err(error.into());
        }
        if !verify_fragment(&pending.patched, &pending.fragment) {
            rollback_configuration_files(
                self,
                &pending,
                config_mode,
                credential_created,
                backup.as_deref(),
            );
            return Err(PiCodingAgentIntegrationError::VerificationFailed);
        }

        let credential = stored_client_credential(
            PI_CODING_AGENT_INTEGRATION.credential_id,
            PI_CODING_AGENT_INTEGRATION.client_app_id,
            PI_CODING_AGENT_INTEGRATION.display_name,
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
            id: PI_CODING_AGENT_INTEGRATION.integration_id.to_owned(),
            kind: "pi-model-provider".to_owned(),
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
            integration_id = PI_CODING_AGENT_INTEGRATION.integration_id,
            action = "configure",
            result = "succeeded",
            model_profile_revision = %pending.profile_revision,
            "external_agent_configuration_applied"
        );
        Ok(ExternalAgentConfigurationResult {
            configured: true,
            integration_id: PI_CODING_AGENT_INTEGRATION.integration_id.to_owned(),
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
    ) -> Result<ExternalAgentDisconnectPlan, PiCodingAgentIntegrationError> {
        reject_managed_file_symlink(&self.paths.config_path)?;
        reject_managed_file_symlink(&self.paths.credential_path)?;
        let integration = self
            .database
            .managed_integration(PI_CODING_AGENT_INTEGRATION.integration_id)?
            .ok_or(PiCodingAgentIntegrationError::NotConfigured)?;
        if Path::new(&integration.config_path) != self.paths.config_path
            || Path::new(&integration.credential_path) != self.paths.credential_path
        {
            return Err(PiCodingAgentIntegrationError::ManagedProviderModified);
        }
        let original = read_managed_file(&self.paths.config_path, MAX_CONFIG_BYTES)?;
        let source = strict_json_source(&original)?;
        let config: Value = serde_json::from_str(source)
            .map_err(|error| PiCodingAgentIntegrationError::InvalidStrictJson(error.to_string()))?;
        let fragment = nested_object_member(&config, PROVIDER_PARENT_KEY, PROVIDER_KEY)
            .ok_or(PiCodingAgentIntegrationError::ManagedProviderModified)?;
        if value_hash(fragment) != integration.managed_fragment_hash {
            return Err(PiCodingAgentIntegrationError::ManagedProviderModified);
        }
        let plaintext_key = read_credential(&self.paths.credential_path)?;
        let credential = self
            .stored_credential()?
            .ok_or(PiCodingAgentIntegrationError::ManagedCredentialModified)?;
        if hash_client_key(&plaintext_key) != credential.key_hash {
            return Err(PiCodingAgentIntegrationError::ManagedCredentialModified);
        }
        let patched = remove_nested_object_member(source, PROVIDER_PARENT_KEY, PROVIDER_KEY)?
            .output
            .into_bytes();
        let resources = self
            .database
            .managed_integration_resources(PI_CODING_AGENT_INTEGRATION.integration_id)?;
        let pending = PendingDisconnect {
            original_digest: managed_content_hash(&original),
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
            .map_err(|_| PiCodingAgentIntegrationError::InvalidPlan)?;

        Ok(ExternalAgentDisconnectPlan {
            plan_id: ticket.plan_id,
            integration_id: PI_CODING_AGENT_INTEGRATION.integration_id.to_owned(),
            expires_at_ms: ticket.expires_at_ms,
            config_path: display_path(&self.paths.config_path, &self.paths.home_directory),
            credential_path: display_path(&self.paths.credential_path, &self.paths.home_directory),
            changes: vec![
                ExternalAgentManagedChange {
                    path: "providers.hal100".to_owned(),
                    action: ExternalAgentManagedChangeAction::RemoveManagedFragment,
                },
                ExternalAgentManagedChange {
                    path: "pi-coding-agent-gateway-key".to_owned(),
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
    ) -> Result<bool, PiCodingAgentIntegrationError> {
        self.pending_disconnect
            .discard(plan_id)
            .map_err(|_| PiCodingAgentIntegrationError::InvalidPlan)
    }

    pub fn apply_disconnection(
        &self,
        plan_id: &str,
    ) -> Result<ExternalAgentDisconnectResult, PiCodingAgentIntegrationError> {
        let pending = self
            .pending_disconnect
            .take(plan_id)
            .map_err(|_| PiCodingAgentIntegrationError::InvalidPlan)?;
        let current = read_managed_file(&self.paths.config_path, MAX_CONFIG_BYTES)?;
        if managed_content_hash(&current) != pending.original_digest {
            return Err(PiCodingAgentIntegrationError::ChangedAfterPreview);
        }
        let current_key = read_credential(&self.paths.credential_path)?;
        if hash_client_key(&current_key) != pending.credential_digest {
            return Err(PiCodingAgentIntegrationError::ChangedAfterPreview);
        }
        let config_mode = managed_file_mode(&self.paths.config_path)?;
        let credential_mode = managed_file_mode(&self.paths.credential_path)?;
        let backup = managed_backup_path(&self.paths.config_path);
        write_new_managed_file(&backup, &pending.original, config_mode)?;
        if let Err(error) =
            atomic_write_managed_file(&self.paths.config_path, &pending.patched, config_mode)
        {
            let _ = fs::remove_file(&backup);
            return Err(error.into());
        }
        if let Err(error) = fs::remove_file(&self.paths.credential_path) {
            rollback_disconnection_files(self, &pending, config_mode, credential_mode, &backup);
            return Err(error.into());
        }
        if let Some(parent) = self.paths.credential_path.parent() {
            let _ = sync_managed_directory(parent);
        }
        if !verify_absent(&pending.patched) {
            rollback_disconnection_files(self, &pending, config_mode, credential_mode, &backup);
            return Err(PiCodingAgentIntegrationError::VerificationFailed);
        }
        if let Err(error) = self
            .credentials
            .remove_client(PI_CODING_AGENT_INTEGRATION.client_app_id)
        {
            rollback_disconnection_files(self, &pending, config_mode, credential_mode, &backup);
            return Err(error.into());
        }
        let database_result = self.database.remove_managed_integration_and_client(
            PI_CODING_AGENT_INTEGRATION.integration_id,
            PI_CODING_AGENT_INTEGRATION.client_app_id,
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
                Ok(false) => Err(PiCodingAgentIntegrationError::NotConfigured),
                Err(error) => Err(error.into()),
                Ok(true) => unreachable!(),
            };
        }

        tracing::info!(
            integration_id = PI_CODING_AGENT_INTEGRATION.integration_id,
            action = "disconnect",
            result = "succeeded",
            "external_agent_disconnected"
        );
        Ok(ExternalAgentDisconnectResult {
            disconnected: true,
            integration_id: PI_CODING_AGENT_INTEGRATION.integration_id.to_owned(),
            config_path: display_path(&self.paths.config_path, &self.paths.home_directory),
            backup_path: Some(display_path(&backup, &self.paths.home_directory)),
            credential_revoked: true,
        })
    }

    fn integration_state(
        &self,
        config: &Value,
        prior: Option<&ManagedIntegrationRecord>,
        profile: &ExternalAgentModelProfile,
    ) -> ExternalAgentIntegrationState {
        if prior.is_some_and(|record| {
            Path::new(&record.config_path) != self.paths.config_path
                || Path::new(&record.credential_path) != self.paths.credential_path
        }) {
            return ExternalAgentIntegrationState::ModifiedOutsideHal100;
        }
        let fragment = nested_object_member(config, PROVIDER_PARENT_KEY, PROVIDER_KEY);
        match (fragment, prior) {
            (None, None) => ExternalAgentIntegrationState::InstalledNotConfigured,
            (Some(_), None) => ExternalAgentIntegrationState::Conflict,
            (None, Some(_)) => ExternalAgentIntegrationState::ModifiedOutsideHal100,
            (Some(fragment), Some(record)) => {
                let actual_hash = value_hash(fragment);
                if actual_hash != record.managed_fragment_hash {
                    return ExternalAgentIntegrationState::ModifiedOutsideHal100;
                }
                let Ok(command) = credential_command(&self.paths.credential_path) else {
                    return ExternalAgentIntegrationState::Blocked;
                };
                let expected = provider_fragment(&command, &self.gateway_base_url, profile);
                if value_hash(&expected) != record.managed_fragment_hash {
                    ExternalAgentIntegrationState::NeedsRefresh
                } else if !self.credential_is_owned_and_valid() {
                    ExternalAgentIntegrationState::ModifiedOutsideHal100
                } else {
                    ExternalAgentIntegrationState::Configured
                }
            }
        }
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
    ) -> Result<Option<StoredClientCredential>, PiCodingAgentIntegrationError> {
        Ok(self
            .database
            .load_client_credentials()?
            .into_iter()
            .find(|credential| {
                credential.key_id == PI_CODING_AGENT_INTEGRATION.credential_id
                    && credential.client_app_id == PI_CODING_AGENT_INTEGRATION.client_app_id
            }))
    }

    fn read_strict_config(&self) -> Result<(Vec<u8>, Value), PiCodingAgentIntegrationError> {
        let bytes = read_managed_file(&self.paths.config_path, MAX_CONFIG_BYTES)?;
        let source = strict_json_source(&bytes)?;
        let value = serde_json::from_str(source)
            .map_err(|error| PiCodingAgentIntegrationError::InvalidStrictJson(error.to_string()))?;
        Ok((bytes, value))
    }

    fn add_path_warnings(&self, warnings: &mut Vec<String>) {
        if let Some(custom) = std::env::var_os("PI_CODING_AGENT_DIR") {
            let custom = PathBuf::from(custom);
            if custom != self.paths.agent_directory {
                warnings.push(format!(
                    "检测到PI_CODING_AGENT_DIR={}；HAL100默认不会写入自定义目录",
                    custom.to_string_lossy()
                ));
            }
        }
    }
}

fn provider_fragment(
    credential_command: &str,
    gateway_base_url: &str,
    profile: &ExternalAgentModelProfile,
) -> Value {
    let input = profile
        .input_modalities
        .iter()
        .map(|modality| match modality {
            ExternalAgentInputModality::Text => "text",
            ExternalAgentInputModality::Image => "image",
        })
        .collect::<Vec<_>>();
    json!({
        "baseUrl": gateway_base_url,
        "api": "openai-completions",
        "apiKey": credential_command,
        "authHeader": true,
        "compat": {
            "supportsDeveloperRole": false,
            "supportsReasoningEffort": false,
            "supportsUsageInStreaming": true,
            "maxTokensField": "max_tokens"
        },
        "models": [{
            "id": profile.model_id,
            "name": profile.display_name,
            "reasoning": profile.supports_reasoning,
            "input": input,
            "contextWindow": profile.context_window_tokens,
            "maxTokens": profile.max_output_tokens,
            "cost": {
                "input": 0,
                "output": 0,
                "cacheRead": 0,
                "cacheWrite": 0
            }
        }]
    })
}

fn credential_command(path: &Path) -> Result<String, PiCodingAgentIntegrationError> {
    let path = path.to_string_lossy();
    if path.contains(['\0', '\n', '\r']) {
        return Err(PiCodingAgentIntegrationError::UnsafeCredentialPath);
    }
    let quoted = path.replace('\'', "'\"'\"'");
    Ok(format!("!/bin/cat '{quoted}'"))
}

fn strict_json_source(bytes: &[u8]) -> Result<&str, PiCodingAgentIntegrationError> {
    std::str::from_utf8(bytes)
        .map_err(|error| PiCodingAgentIntegrationError::InvalidStrictJson(error.to_string()))
}

fn read_credential(path: &Path) -> Result<String, PiCodingAgentIntegrationError> {
    let bytes = read_managed_file(path, 256)?;
    if !(24..=256).contains(&bytes.len()) {
        return Err(PiCodingAgentIntegrationError::InvalidCredentialFile);
    }
    let key = String::from_utf8(bytes)
        .map_err(|_| PiCodingAgentIntegrationError::InvalidCredentialFile)?;
    if key.contains(['\n', '\r']) {
        return Err(PiCodingAgentIntegrationError::InvalidCredentialFile);
    }
    #[cfg(unix)]
    if managed_file_mode(path)? & 0o077 != 0 {
        return Err(PiCodingAgentIntegrationError::InvalidCredentialFile);
    }
    Ok(key)
}

fn verify_fragment(bytes: &[u8], expected: &Value) -> bool {
    strict_json_source(bytes)
        .ok()
        .and_then(|source| serde_json::from_str::<Value>(source).ok())
        .and_then(|config| {
            nested_object_member(&config, PROVIDER_PARENT_KEY, PROVIDER_KEY).cloned()
        })
        .is_some_and(|fragment| fragment == *expected)
}

fn verify_absent(bytes: &[u8]) -> bool {
    strict_json_source(bytes)
        .ok()
        .and_then(|source| serde_json::from_str::<Value>(source).ok())
        .is_some_and(|config| {
            nested_object_member(&config, PROVIDER_PARENT_KEY, PROVIDER_KEY).is_none()
        })
}

fn rollback_configuration_files(
    adapter: &PiCodingAgentIntegrationAdapter,
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

fn rollback_registry(
    adapter: &PiCodingAgentIntegrationAdapter,
    prior: Option<StoredClientCredential>,
) {
    let _ = adapter
        .credentials
        .remove_client(PI_CODING_AGENT_INTEGRATION.client_app_id);
    if let Some(prior) = prior {
        let _ = adapter.credentials.upsert(prior);
    }
}

fn rollback_disconnection_files(
    adapter: &PiCodingAgentIntegrationAdapter,
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
    if let Some(parent) = backup.parent() {
        let _ = sync_managed_directory(parent);
    }
}

fn generate_client_key() -> String {
    format!(
        "hal100_pi_{}{}",
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

fn pi_version(binary: &Path) -> Option<String> {
    let output = BoundedCommandRunner::new(Duration::from_secs(2), 128)
        .run_utf8(binary, &["--version"])
        .ok()?;
    let version = output.trim();
    (!version.is_empty()).then(|| version.to_owned())
}

fn stable_version_triplet(version: &str) -> Option<(u64, u64, u64)> {
    let version = version
        .strip_prefix("pi ")
        .or_else(|| version.strip_prefix("pi-coding-agent "))
        .unwrap_or(version);
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

    #[test]
    fn user_pi_candidates_always_precede_the_hal100_private_runtime() {
        let paths = PiCodingAgentPaths::for_macos(
            Path::new("/Users/example"),
            Path::new("/Users/example/Library/Application Support/HAL100"),
        );
        assert_eq!(
            paths.binary_candidates.last(),
            Some(&PathBuf::from(
                "/Users/example/Library/Application Support/HAL100/external-agents/pi-coding-agent/runtime/node_modules/.bin/pi"
            ))
        );
        assert!(
            paths.binary_candidates[..paths.binary_candidates.len() - 1]
                .iter()
                .all(|candidate| !candidate.to_string_lossy().contains("external-agents"))
        );
    }

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("hal100-pi-agent-{}", Uuid::new_v4()));
            fs::create_dir(&path).expect("create test directory");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct Fixture {
        root: TestDirectory,
        adapter: PiCodingAgentIntegrationAdapter,
        database: Arc<Database>,
        credentials: CredentialRegistry,
        config_path: PathBuf,
        credential_path: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = TestDirectory::new();
            let binary = root.0.join("bin/pi");
            fs::create_dir_all(binary.parent().expect("binary parent")).expect("binary directory");
            fs::write(&binary, "#!/bin/sh\nprintf '0.84.2\\n'\n").expect("fake Pi CLI");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&binary, fs::Permissions::from_mode(0o700))
                    .expect("fake Pi executable mode");
            }
            let database =
                Arc::new(Database::open(root.0.join("hal100.sqlite")).expect("database"));
            let credentials = CredentialRegistry::new(Vec::new());
            let config_path = root.0.join(".pi/agent/models.json");
            let credential_path = root
                .0
                .join("app-data/credentials/pi-coding-agent-gateway.key");
            let paths = PiCodingAgentPaths {
                home_directory: root.0.clone(),
                agent_directory: root.0.join(".pi/agent"),
                config_path: config_path.clone(),
                credential_path: credential_path.clone(),
                binary_candidates: vec![binary],
            };
            let adapter = PiCodingAgentIntegrationAdapter::new(
                database.clone(),
                credentials.clone(),
                ExternalModelProfileRegistry::conservative_managed_route(),
                paths,
            );
            Self {
                root,
                adapter,
                database,
                credentials,
                config_path,
                credential_path,
            }
        }

        fn write_config(&self, source: &str) {
            fs::create_dir_all(self.config_path.parent().expect("config parent"))
                .expect("config directory");
            fs::write(&self.config_path, source).expect("write Pi config");
        }
    }

    #[test]
    fn provider_fragment_matches_the_official_pi_models_contract() {
        let profile = ExternalModelProfileRegistry::conservative_managed_route()
            .snapshot()
            .expect("profile");
        let fragment = provider_fragment(
            "!/bin/cat '/tmp/pi.key'",
            "http://127.0.0.1:10100/v1",
            &profile,
        );

        assert_eq!(fragment["api"], "openai-completions");
        assert_eq!(fragment["authHeader"], true);
        assert_eq!(fragment["models"][0]["id"], "hal100-active");
        assert_eq!(fragment["models"][0]["input"][0], "text");
        assert_eq!(fragment["models"][0]["contextWindow"], 4_096);
        assert_eq!(fragment["models"][0]["maxTokens"], 1_024);
        assert!(fragment.get("supportsTools").is_none());
    }

    #[test]
    fn configuration_preserves_user_defaults_and_isolates_the_credential() {
        let fixture = Fixture::new();
        fixture.write_config(
            r#"{
  "defaultProvider": "anthropic",
  "providers": {
    "personal": { "baseUrl": "https://example.test" }
  }
}
"#,
        );
        let plan = fixture
            .adapter
            .plan_configuration()
            .expect("configuration plan");
        assert!(plan.preserves_default_model);
        assert_eq!(plan.model_profile_revision, "managed-route-v1");
        let result = fixture
            .adapter
            .apply_configuration(&plan.plan_id)
            .expect("apply Pi configuration");

        let config: Value =
            serde_json::from_slice(&fs::read(&fixture.config_path).expect("read patched config"))
                .expect("strict JSON");
        assert_eq!(config["defaultProvider"], "anthropic");
        assert_eq!(
            config["providers"]["personal"]["baseUrl"],
            "https://example.test"
        );
        assert_eq!(
            config["providers"]["hal100"]["models"][0]["id"],
            "hal100-active"
        );
        let credential = fs::read_to_string(&fixture.credential_path).expect("credential");
        assert!(
            !fs::read_to_string(&fixture.config_path)
                .expect("config source")
                .contains(&credential)
        );
        assert!(
            fixture
                .credentials
                .authenticate(&credential)
                .is_some_and(|client| {
                    client.client_app_id == PI_CODING_AGENT_INTEGRATION.client_app_id
                })
        );
        assert!(result.backup_path.is_some());
        assert!(
            fixture
                .database
                .managed_integration(PI_CODING_AGENT_INTEGRATION.integration_id)
                .expect("managed integration")
                .is_some()
        );
        assert_eq!(
            fixture
                .database
                .managed_integration_resources(PI_CODING_AGENT_INTEGRATION.integration_id,)
                .expect("resources")
                .len(),
            2
        );
    }

    #[test]
    fn disconnection_removes_only_hal100_and_revokes_only_the_pi_key() {
        let fixture = Fixture::new();
        fixture.write_config(r#"{"providers":{"personal":{"baseUrl":"https://example.test"}}}"#);
        let configure = fixture
            .adapter
            .plan_configuration()
            .expect("configuration plan");
        fixture
            .adapter
            .apply_configuration(&configure.plan_id)
            .expect("configure");
        let key = fs::read_to_string(&fixture.credential_path).expect("credential");
        let disconnect = fixture
            .adapter
            .plan_disconnection()
            .expect("disconnect plan");
        let result = fixture
            .adapter
            .apply_disconnection(&disconnect.plan_id)
            .expect("disconnect");

        let config: Value = serde_json::from_slice(
            &fs::read(&fixture.config_path).expect("read disconnected config"),
        )
        .expect("strict JSON");
        assert_eq!(
            config["providers"]["personal"]["baseUrl"],
            "https://example.test"
        );
        assert!(config["providers"].get("hal100").is_none());
        assert!(!fixture.credential_path.exists());
        assert!(fixture.credentials.authenticate(&key).is_none());
        assert!(result.backup_path.is_some());
        assert!(
            fixture
                .database
                .managed_integration(PI_CODING_AGENT_INTEGRATION.integration_id)
                .expect("managed integration")
                .is_none()
        );
    }

    #[test]
    fn detects_external_modification_and_refuses_to_overwrite() {
        let fixture = Fixture::new();
        fixture.write_config("{}");
        let plan = fixture
            .adapter
            .plan_configuration()
            .expect("configuration plan");
        fixture
            .adapter
            .apply_configuration(&plan.plan_id)
            .expect("configure");
        let mut config: Value =
            serde_json::from_slice(&fs::read(&fixture.config_path).expect("read config"))
                .expect("strict JSON");
        config["providers"]["hal100"]["baseUrl"] = json!("https://tampered.invalid");
        fs::write(
            &fixture.config_path,
            serde_json::to_vec_pretty(&config).expect("serialize tampered config"),
        )
        .expect("tamper config");

        let detection = fixture.adapter.detect().expect("detect");
        assert_eq!(
            detection.integration_state,
            ExternalAgentIntegrationState::ModifiedOutsideHal100
        );
        assert!(matches!(
            fixture.adapter.plan_configuration(),
            Err(PiCodingAgentIntegrationError::ManagedProviderModified)
        ));
    }

    #[test]
    fn rejects_jsonc_because_pi_requires_strict_json() {
        let fixture = Fixture::new();
        fixture.write_config("{ // comment\n  \"providers\": {}\n}\n");
        assert!(matches!(
            fixture.adapter.plan_configuration(),
            Err(PiCodingAgentIntegrationError::InvalidStrictJson(_))
        ));
    }

    #[test]
    fn shell_quotes_a_credential_path_without_allowing_command_injection() {
        let command = credential_command(Path::new("/tmp/O'Brien/$(touch nope).key"))
            .expect("credential command");
        assert_eq!(command, "!/bin/cat '/tmp/O'\"'\"'Brien/$(touch nope).key'");
    }

    #[test]
    fn stale_and_discarded_plans_do_not_mutate_files() {
        let fixture = Fixture::new();
        fixture.write_config("{}");
        let discarded = fixture
            .adapter
            .plan_configuration()
            .expect("discarded plan");
        assert!(
            fixture
                .adapter
                .discard_configuration_plan(&discarded.plan_id)
                .expect("discard")
        );
        assert!(matches!(
            fixture.adapter.apply_configuration(&discarded.plan_id),
            Err(PiCodingAgentIntegrationError::InvalidPlan)
        ));
        assert_eq!(
            fs::read_to_string(&fixture.config_path).expect("config"),
            "{}"
        );
        assert!(!fixture.credential_path.exists());
        assert!(fixture.root.0.exists());
    }

    #[test]
    fn parses_only_stable_pi_versions() {
        assert_eq!(stable_version_triplet("0.84.2"), Some((0, 84, 2)));
        assert_eq!(stable_version_triplet("pi 0.84.3"), Some((0, 84, 3)));
        assert_eq!(
            stable_version_triplet("pi-coding-agent 0.85.0-beta.1"),
            Some((0, 85, 0))
        );
        assert_eq!(stable_version_triplet("dev"), None);
    }
}
