use std::{collections::HashMap, str, sync::Arc, time::Duration};

use hal100_core::{SecretStore, SecretStoreError};
use hal100_protocol::{
    BackendAuthMethod, BackendCatalog, BackendDraft, BackendKind, BackendProbeResult,
    BackendRouteDraft, BackendRouteSummary, BackendSummary,
};
use thiserror::Error;
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use crate::{
    BackendAuthStyle, BackendConfig, Database, DatabaseError, GatewayBuildError, GatewayProbeError,
    GatewayRouteError, GatewayRouteSwitchError, GatewayState, StoredBackendRecord,
    StoredModelRouteRecord,
};

const MAX_DISPLAY_NAME_BYTES: usize = 256;
const MAX_API_KEY_BYTES: usize = 16 * 1024;
const ROUTE_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);
const INTERNAL_AGENT_MODEL_ALIAS: &str = "hal100-agent";
const INTERNAL_CLOUD_AGENT_ALIAS_PREFIX: &str = "hal100-agent-cloud-";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendRestoreReport {
    pub loaded_backends: usize,
    pub loaded_routes: usize,
    pub skipped_backend_ids: Vec<String>,
    pub skipped_route_aliases: Vec<String>,
    pub active_backend_restored: bool,
}

#[derive(Debug, Error)]
pub enum BackendManagerError {
    #[error(transparent)]
    Database(#[from] DatabaseError),
    #[error("系统凭据库操作失败")]
    CredentialStore(#[from] SecretStoreError),
    #[error(transparent)]
    GatewayBuild(#[from] GatewayBuildError),
    #[error(transparent)]
    GatewayRoute(#[from] GatewayRouteError),
    #[error(transparent)]
    GatewaySwitch(#[from] GatewayRouteSwitchError),
    #[error(transparent)]
    GatewayProbe(#[from] GatewayProbeError),
    #[error("后端名称无效")]
    InvalidDisplayName,
    #[error("托管 llama.cpp 后端不能通过外部后端编辑器创建")]
    ManagedBackendNotEditable,
    #[error("后端不存在")]
    BackendNotFound,
    #[error("当前后端需要 API Key")]
    MissingApiKey,
    #[error("API Key 超出安全长度限制")]
    ApiKeyTooLarge,
    #[error("API Key 不是有效文本")]
    InvalidApiKey,
    #[error("无认证后端不能同时提交 API Key")]
    UnexpectedApiKey,
    #[error("后端正在被托管模型占用；请先停止本地模型")]
    ManagedBackendActive,
    #[error("模型别名不存在")]
    RouteNotFound,
    #[error("该模型别名属于 HAL100 Agent 内部保留命名空间")]
    ReservedAgentRoute,
    #[error("数据库包含无法识别的后端类型或认证方式")]
    InvalidStoredBackend,
    #[error("后端切换后的持久化失败，且运行态回滚也未完成")]
    PersistenceRollbackFailed,
}

pub struct BackendManager {
    database: Arc<Database>,
    gateway: GatewayState,
    secrets: Arc<dyn SecretStore>,
    mutations: AsyncMutex<()>,
}

impl BackendManager {
    pub fn new(
        database: Arc<Database>,
        gateway: GatewayState,
        secrets: Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            database,
            gateway,
            secrets,
            mutations: AsyncMutex::new(()),
        }
    }

    pub fn restore(&self) -> Result<BackendRestoreReport, BackendManagerError> {
        let records = self.database.backends()?;
        let mut loaded = HashMap::new();
        let mut skipped_backend_ids = Vec::new();
        for record in records.iter().filter(|record| record.enabled) {
            match self.config_for_record(record) {
                Ok(config) => {
                    self.gateway.upsert_routed_backend(config.clone())?;
                    loaded.insert(record.id.clone(), config);
                }
                Err(_) => skipped_backend_ids.push(record.id.clone()),
            }
        }

        let requested_active = self.database.active_backend_id()?;
        let active_backend_restored = requested_active
            .as_ref()
            .and_then(|backend_id| loaded.get(backend_id))
            .map(|backend| {
                self.gateway.replace_backend(Some(backend.clone()));
            })
            .is_some();

        let mut loaded_routes = 0;
        let mut skipped_route_aliases = Vec::new();
        for route in self.database.model_routes()? {
            if is_internal_agent_alias(&route.alias)
                || !loaded.contains_key(&route.backend_id)
                || self
                    .gateway
                    .set_model_route(&route.alias, &route.backend_id, &route.resolved_model)
                    .is_err()
            {
                skipped_route_aliases.push(route.alias);
            } else {
                loaded_routes += 1;
            }
        }
        skipped_backend_ids.sort();
        skipped_route_aliases.sort();
        Ok(BackendRestoreReport {
            loaded_backends: loaded.len(),
            loaded_routes,
            skipped_backend_ids,
            skipped_route_aliases,
            active_backend_restored,
        })
    }

    pub fn catalog(&self) -> Result<BackendCatalog, BackendManagerError> {
        let routing = self.gateway.routing_snapshot();
        let runtime_backends = routing
            .backend_ids
            .iter()
            .map(String::as_str)
            .collect::<std::collections::HashSet<_>>();
        let active_backend_id = routing.active_backend_id.clone();
        let backends = self
            .database
            .backends()?
            .into_iter()
            .map(|record| {
                let health = routing
                    .backend_health
                    .iter()
                    .find(|health| health.backend_id == record.id);
                Ok(BackendSummary {
                    id: record.id.clone(),
                    display_name: record.display_name,
                    kind: parse_kind(&record.kind)?,
                    api_root: record.api_root,
                    auth_method: parse_auth_method(&record.auth_style)?,
                    credential_configured: record.credential_id.is_some(),
                    enabled: record.enabled,
                    runtime_available: runtime_backends.contains(record.id.as_str()),
                    is_active: active_backend_id.as_deref() == Some(record.id.as_str()),
                    consecutive_failures: health.map_or(0, |health| health.consecutive_failures),
                    circuit_open: health.is_some_and(|health| health.circuit_open),
                })
            })
            .collect::<Result<Vec<_>, BackendManagerError>>()?;
        let model_routes = self
            .database
            .model_routes()?
            .into_iter()
            .map(|route| {
                let runtime_available = routing.model_routes.iter().any(|runtime| {
                    runtime.alias == route.alias
                        && runtime.backend_id == route.backend_id
                        && runtime.resolved_model == route.resolved_model
                });
                BackendRouteSummary {
                    alias: route.alias,
                    backend_id: route.backend_id,
                    resolved_model: route.resolved_model,
                    runtime_available,
                }
            })
            .collect();
        Ok(BackendCatalog {
            active_backend_id,
            backends,
            model_routes,
        })
    }

    pub async fn save_backend(
        &self,
        draft: BackendDraft,
    ) -> Result<BackendCatalog, BackendManagerError> {
        let _guard = self.mutations.lock().await;
        validate_display_name(&draft.display_name)?;
        if draft.kind == BackendKind::ManagedLlamaCpp {
            return Err(BackendManagerError::ManagedBackendNotEditable);
        }
        let existing = match &draft.id {
            Some(id) => Some(
                self.database
                    .backends()?
                    .into_iter()
                    .find(|backend| backend.id == *id)
                    .ok_or(BackendManagerError::BackendNotFound)?,
            ),
            None => None,
        };
        let backend_id = existing
            .as_ref()
            .map(|backend| backend.id.clone())
            .unwrap_or_else(|| format!("backend-{}", Uuid::new_v4().simple()));
        let old_credential_id = existing
            .as_ref()
            .and_then(|backend| backend.credential_id.clone());
        let old_secret = match &old_credential_id {
            Some(credential_id) => self.secrets.read(credential_id)?,
            None => None,
        };
        let submitted_api_key = draft.api_key.filter(|value| !value.is_empty());
        if submitted_api_key
            .as_ref()
            .is_some_and(|value| value.len() > MAX_API_KEY_BYTES)
        {
            return Err(BackendManagerError::ApiKeyTooLarge);
        }

        let (credential_id, api_key) = match draft.auth_method {
            BackendAuthMethod::None => {
                if submitted_api_key.is_some() {
                    return Err(BackendManagerError::UnexpectedApiKey);
                }
                (None, None)
            }
            BackendAuthMethod::Bearer | BackendAuthMethod::AnthropicApiKey => {
                let api_key = submitted_api_key.or_else(|| {
                    old_secret
                        .as_ref()
                        .and_then(|secret| str::from_utf8(secret).ok())
                        .map(str::to_owned)
                });
                let api_key = api_key.ok_or(BackendManagerError::MissingApiKey)?;
                if api_key.len() > MAX_API_KEY_BYTES {
                    return Err(BackendManagerError::ApiKeyTooLarge);
                }
                let credential_id = old_credential_id
                    .clone()
                    .unwrap_or_else(|| format!("hal100.backend.{backend_id}"));
                (Some(credential_id), Some(api_key))
            }
        };

        let config = build_config(
            &backend_id,
            &draft.api_root,
            draft.auth_method,
            api_key.clone(),
        )?;
        let now = now_ms();
        let record = StoredBackendRecord {
            id: backend_id.clone(),
            display_name: draft.display_name.trim().to_owned(),
            kind: kind_key(draft.kind).to_owned(),
            api_root: config.api_root().as_str().to_owned(),
            auth_style: auth_method_key(draft.auth_method).to_owned(),
            credential_id: credential_id.clone(),
            enabled: true,
            created_at_ms: existing
                .as_ref()
                .map_or(now, |backend| backend.created_at_ms),
            updated_at_ms: now,
        };

        self.apply_secret_change(
            old_credential_id.as_deref(),
            credential_id.as_deref(),
            api_key.as_deref(),
        )?;
        if let Err(error) = self.database.upsert_backend(&record) {
            self.rollback_secret_change(
                old_credential_id.as_deref(),
                old_secret.as_deref(),
                credential_id.as_deref(),
            );
            return Err(error.into());
        }
        let runtime_result = if self
            .gateway
            .backend_config()
            .as_ref()
            .is_some_and(|backend| backend.id() == backend_id)
        {
            self.gateway
                .replace_backend_when_idle(Some(config.clone()), ROUTE_DRAIN_TIMEOUT)
                .await
                .map(|_| ())
                .map_err(BackendManagerError::from)
        } else {
            self.gateway
                .upsert_routed_backend(config.clone())
                .map_err(BackendManagerError::from)
        };
        if let Err(error) = runtime_result {
            self.rollback_backend_record(existing.as_ref(), &backend_id);
            self.rollback_secret_change(
                old_credential_id.as_deref(),
                old_secret.as_deref(),
                credential_id.as_deref(),
            );
            return Err(error);
        }
        self.gateway.upsert_routed_backend(config)?;
        self.catalog()
    }

    pub async fn activate_backend(
        &self,
        backend_id: &str,
    ) -> Result<BackendCatalog, BackendManagerError> {
        let _guard = self.mutations.lock().await;
        let record = self
            .database
            .backends()?
            .into_iter()
            .find(|backend| backend.id == backend_id && backend.enabled)
            .ok_or(BackendManagerError::BackendNotFound)?;
        let current = self.gateway.backend_config();
        if current.as_ref().is_some_and(|backend| {
            backend.id() == "managed-llama-cpp" && backend.id() != backend_id
        }) {
            return Err(BackendManagerError::ManagedBackendActive);
        }
        if current
            .as_ref()
            .is_some_and(|backend| backend.id() == backend_id)
        {
            self.database
                .set_active_backend_id(Some(backend_id), now_ms())?;
            return self.catalog();
        }
        let next = self.config_for_record(&record)?;
        let previous = self
            .gateway
            .replace_backend_when_idle(Some(next), ROUTE_DRAIN_TIMEOUT)
            .await?;
        if let Err(error) = self
            .database
            .set_active_backend_id(Some(backend_id), now_ms())
        {
            if self
                .gateway
                .replace_backend_when_idle(previous, ROUTE_DRAIN_TIMEOUT)
                .await
                .is_err()
            {
                return Err(BackendManagerError::PersistenceRollbackFailed);
            }
            return Err(error.into());
        }
        self.catalog()
    }

    pub async fn force_activate_backend(
        &self,
        backend_id: &str,
    ) -> Result<BackendCatalog, BackendManagerError> {
        let _guard = self.mutations.lock().await;
        let record = self
            .database
            .backends()?
            .into_iter()
            .find(|backend| backend.id == backend_id && backend.enabled)
            .ok_or(BackendManagerError::BackendNotFound)?;
        let current = self.gateway.backend_config();
        if current.as_ref().is_some_and(|backend| {
            backend.id() == "managed-llama-cpp" && backend.id() != backend_id
        }) {
            return Err(BackendManagerError::ManagedBackendActive);
        }
        if current
            .as_ref()
            .is_some_and(|backend| backend.id() == backend_id)
        {
            self.database
                .set_active_backend_id(Some(backend_id), now_ms())?;
            return self.catalog();
        }
        let next = self.config_for_record(&record)?;
        let persisted_active = self.database.active_backend_id()?;
        self.database
            .set_active_backend_id(Some(backend_id), now_ms())?;
        if let Err(error) = self.gateway.force_replace_backend(Some(next)).await {
            let _ = self
                .database
                .set_active_backend_id(persisted_active.as_deref(), now_ms());
            return Err(error.into());
        }
        self.catalog()
    }

    pub async fn probe_backend(
        &self,
        backend_id: &str,
    ) -> Result<BackendProbeResult, BackendManagerError> {
        let _guard = self.mutations.lock().await;
        if !self
            .database
            .backends()?
            .iter()
            .any(|backend| backend.id == backend_id && backend.enabled)
        {
            return Err(BackendManagerError::BackendNotFound);
        }
        self.gateway
            .probe_backend(backend_id)
            .await
            .map_err(Into::into)
    }

    pub async fn save_route(
        &self,
        draft: BackendRouteDraft,
    ) -> Result<BackendCatalog, BackendManagerError> {
        let _guard = self.mutations.lock().await;
        if is_internal_agent_alias(&draft.alias) {
            return Err(BackendManagerError::ReservedAgentRoute);
        }
        let existing = self
            .database
            .model_routes()?
            .into_iter()
            .find(|route| route.alias == draft.alias);
        self.gateway
            .set_model_route(&draft.alias, &draft.backend_id, &draft.resolved_model)?;
        let now = now_ms();
        let route = StoredModelRouteRecord {
            alias: draft.alias.clone(),
            backend_id: draft.backend_id,
            resolved_model: draft.resolved_model,
            created_at_ms: existing.as_ref().map_or(now, |route| route.created_at_ms),
            updated_at_ms: now,
        };
        if let Err(error) = self.database.upsert_model_route(&route) {
            self.rollback_route(existing.as_ref(), &draft.alias);
            return Err(error.into());
        }
        self.catalog()
    }

    pub async fn delete_route(&self, alias: &str) -> Result<BackendCatalog, BackendManagerError> {
        let _guard = self.mutations.lock().await;
        if is_internal_agent_alias(alias) {
            return Err(BackendManagerError::ReservedAgentRoute);
        }
        let existing = self
            .database
            .model_routes()?
            .into_iter()
            .find(|route| route.alias == alias)
            .ok_or(BackendManagerError::RouteNotFound)?;
        self.gateway.remove_model_route(alias)?;
        if let Err(error) = self.database.delete_model_route(alias) {
            self.rollback_route(Some(&existing), alias);
            return Err(error.into());
        }
        self.catalog()
    }

    pub async fn delete_backend(
        &self,
        backend_id: &str,
    ) -> Result<BackendCatalog, BackendManagerError> {
        let _guard = self.mutations.lock().await;
        let existing = self
            .database
            .backends()?
            .into_iter()
            .find(|backend| backend.id == backend_id)
            .ok_or(BackendManagerError::BackendNotFound)?;
        let config = self.config_for_record(&existing)?;
        let old_secret = match &existing.credential_id {
            Some(credential_id) => self.secrets.read(credential_id)?,
            None => None,
        };
        let persisted_active = self.database.active_backend_id()?;
        self.gateway.remove_routed_backend(backend_id)?;
        if let Some(credential_id) = &existing.credential_id
            && let Err(error) = self.secrets.delete(credential_id)
        {
            let _ = self.gateway.upsert_routed_backend(config);
            return Err(error.into());
        }
        if persisted_active.as_deref() == Some(backend_id)
            && let Err(error) = self.database.set_active_backend_id(None, now_ms())
        {
            self.restore_secret(existing.credential_id.as_deref(), old_secret.as_deref());
            let _ = self.gateway.upsert_routed_backend(config);
            return Err(error.into());
        }
        if let Err(error) = self.database.delete_backend(backend_id) {
            if persisted_active.as_deref() == Some(backend_id) {
                let _ = self
                    .database
                    .set_active_backend_id(Some(backend_id), now_ms());
            }
            self.restore_secret(existing.credential_id.as_deref(), old_secret.as_deref());
            let _ = self.gateway.upsert_routed_backend(config);
            return Err(error.into());
        }
        self.catalog()
    }

    fn config_for_record(
        &self,
        record: &StoredBackendRecord,
    ) -> Result<BackendConfig, BackendManagerError> {
        if parse_kind(&record.kind)? == BackendKind::ManagedLlamaCpp {
            return Err(BackendManagerError::ManagedBackendNotEditable);
        }
        let auth_method = parse_auth_method(&record.auth_style)?;
        let api_key = match auth_method {
            BackendAuthMethod::None => None,
            BackendAuthMethod::Bearer | BackendAuthMethod::AnthropicApiKey => {
                let credential_id = record
                    .credential_id
                    .as_deref()
                    .ok_or(BackendManagerError::MissingApiKey)?;
                let secret = self
                    .secrets
                    .read(credential_id)?
                    .ok_or(BackendManagerError::MissingApiKey)?;
                if secret.len() > MAX_API_KEY_BYTES {
                    return Err(BackendManagerError::ApiKeyTooLarge);
                }
                Some(String::from_utf8(secret).map_err(|_| BackendManagerError::InvalidApiKey)?)
            }
        };
        build_config(&record.id, &record.api_root, auth_method, api_key)
    }

    fn apply_secret_change(
        &self,
        old_credential_id: Option<&str>,
        credential_id: Option<&str>,
        api_key: Option<&str>,
    ) -> Result<(), BackendManagerError> {
        match (credential_id, api_key) {
            (Some(credential_id), Some(api_key)) => {
                self.secrets.write(credential_id, api_key.as_bytes())?;
                if let Some(old_credential_id) = old_credential_id
                    && old_credential_id != credential_id
                    && let Err(error) = self.secrets.delete(old_credential_id)
                {
                    let _ = self.secrets.delete(credential_id);
                    return Err(error.into());
                }
            }
            (None, None) => {
                if let Some(old_credential_id) = old_credential_id {
                    self.secrets.delete(old_credential_id)?;
                }
            }
            _ => return Err(BackendManagerError::MissingApiKey),
        }
        Ok(())
    }

    fn restore_secret(&self, credential_id: Option<&str>, secret: Option<&[u8]>) {
        if let Some(credential_id) = credential_id {
            match secret {
                Some(secret) => {
                    let _ = self.secrets.write(credential_id, secret);
                }
                None => {
                    let _ = self.secrets.delete(credential_id);
                }
            }
        }
    }

    fn rollback_secret_change(
        &self,
        old_credential_id: Option<&str>,
        old_secret: Option<&[u8]>,
        new_credential_id: Option<&str>,
    ) {
        if let Some(new_credential_id) = new_credential_id
            && old_credential_id != Some(new_credential_id)
        {
            let _ = self.secrets.delete(new_credential_id);
        }
        self.restore_secret(old_credential_id, old_secret);
    }

    fn rollback_backend_record(&self, existing: Option<&StoredBackendRecord>, backend_id: &str) {
        match existing {
            Some(existing) => {
                let _ = self.database.upsert_backend(existing);
            }
            None => {
                let _ = self.database.delete_backend(backend_id);
            }
        }
    }

    fn rollback_route(&self, existing: Option<&StoredModelRouteRecord>, alias: &str) {
        match existing {
            Some(existing) => {
                let _ = self.gateway.set_model_route(
                    &existing.alias,
                    &existing.backend_id,
                    &existing.resolved_model,
                );
            }
            None => {
                let _ = self.gateway.remove_model_route(alias);
            }
        }
    }
}

fn is_internal_agent_alias(alias: &str) -> bool {
    alias == INTERNAL_AGENT_MODEL_ALIAS || alias.starts_with(INTERNAL_CLOUD_AGENT_ALIAS_PREFIX)
}

fn validate_display_name(display_name: &str) -> Result<(), BackendManagerError> {
    if display_name.trim().is_empty()
        || display_name.len() > MAX_DISPLAY_NAME_BYTES
        || display_name.chars().any(char::is_control)
    {
        return Err(BackendManagerError::InvalidDisplayName);
    }
    Ok(())
}

fn build_config(
    backend_id: &str,
    api_root: &str,
    auth_method: BackendAuthMethod,
    api_key: Option<String>,
) -> Result<BackendConfig, BackendManagerError> {
    let mut config = BackendConfig::new(backend_id, api_root, api_key)?;
    if auth_method == BackendAuthMethod::AnthropicApiKey {
        config = config.with_auth_style(BackendAuthStyle::AnthropicApiKey);
    }
    Ok(config)
}

fn kind_key(kind: BackendKind) -> &'static str {
    match kind {
        BackendKind::ManagedLlamaCpp => "managed_llama_cpp",
        BackendKind::ExternalOpenAi => "external_openai",
        BackendKind::ExternalAnthropic => "external_anthropic",
        BackendKind::ExternalOllama => "external_ollama",
        BackendKind::ExternalVllm => "external_vllm",
        BackendKind::ExternalLlamaCpp => "external_llama_cpp",
    }
}

fn parse_kind(value: &str) -> Result<BackendKind, BackendManagerError> {
    match value {
        "managed_llama_cpp" => Ok(BackendKind::ManagedLlamaCpp),
        "external_openai" => Ok(BackendKind::ExternalOpenAi),
        "external_anthropic" => Ok(BackendKind::ExternalAnthropic),
        "external_ollama" => Ok(BackendKind::ExternalOllama),
        "external_vllm" => Ok(BackendKind::ExternalVllm),
        "external_llama_cpp" => Ok(BackendKind::ExternalLlamaCpp),
        _ => Err(BackendManagerError::InvalidStoredBackend),
    }
}

fn auth_method_key(auth_method: BackendAuthMethod) -> &'static str {
    match auth_method {
        BackendAuthMethod::None => "none",
        BackendAuthMethod::Bearer => "bearer",
        BackendAuthMethod::AnthropicApiKey => "anthropic_api_key",
    }
}

fn parse_auth_method(value: &str) -> Result<BackendAuthMethod, BackendManagerError> {
    match value {
        "none" => Ok(BackendAuthMethod::None),
        "bearer" => Ok(BackendAuthMethod::Bearer),
        "anthropic_api_key" => Ok(BackendAuthMethod::AnthropicApiKey),
        _ => Err(BackendManagerError::InvalidStoredBackend),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use hal100_core::{SecretStoreError, SecretStoreOperation};

    use super::*;
    use crate::{CredentialRegistry, UsageWriter};

    #[derive(Default)]
    struct MemorySecretStore {
        values: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl SecretStore for MemorySecretStore {
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

    #[tokio::test]
    async fn persists_non_secret_metadata_and_restores_routes_with_keychain_secret() {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        let secrets = Arc::new(MemorySecretStore::default());
        let gateway = GatewayState::new(
            None,
            CredentialRegistry::new(Vec::new()),
            UsageWriter::start(database.clone()),
        )
        .expect("gateway");
        let manager = BackendManager::new(database.clone(), gateway, secrets.clone());

        let catalog = manager
            .save_backend(BackendDraft {
                id: None,
                display_name: "本地 vLLM".to_owned(),
                kind: BackendKind::ExternalVllm,
                api_root: "http://127.0.0.1:8000/v1".to_owned(),
                auth_method: BackendAuthMethod::Bearer,
                api_key: Some("keychain-only-secret".to_owned()),
            })
            .await
            .expect("save backend");
        let backend_id = catalog.backends[0].id.clone();
        assert!(
            !format!("{:?}", database.backends().expect("records"))
                .contains("keychain-only-secret")
        );

        manager
            .save_route(BackendRouteDraft {
                alias: "qwen-local".to_owned(),
                backend_id: backend_id.clone(),
                resolved_model: "Qwen/Qwen3.5-2B".to_owned(),
            })
            .await
            .expect("save route");
        manager
            .activate_backend(&backend_id)
            .await
            .expect("activate");

        let restored_gateway = GatewayState::new(
            None,
            CredentialRegistry::new(Vec::new()),
            UsageWriter::start(database.clone()),
        )
        .expect("restored gateway");
        let restored = BackendManager::new(database, restored_gateway.clone(), secrets);
        let report = restored.restore().expect("restore");
        assert_eq!(report.loaded_backends, 1);
        assert_eq!(report.loaded_routes, 1);
        assert!(report.active_backend_restored);
        let snapshot = restored_gateway.routing_snapshot();
        assert_eq!(
            snapshot.active_backend_id.as_deref(),
            Some(backend_id.as_str())
        );
        assert_eq!(snapshot.model_routes[0].alias, "qwen-local");
        assert_eq!(snapshot.model_routes[0].resolved_model, "Qwen/Qwen3.5-2B");
    }

    #[tokio::test]
    async fn force_activation_updates_the_runtime_and_persisted_active_backend() {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        let gateway = GatewayState::new(
            None,
            CredentialRegistry::new(Vec::new()),
            UsageWriter::start(database.clone()),
        )
        .expect("gateway");
        let manager = BackendManager::new(
            database.clone(),
            gateway.clone(),
            Arc::new(MemorySecretStore::default()),
        );

        let first = manager
            .save_backend(BackendDraft {
                id: None,
                display_name: "第一个本地后端".to_owned(),
                kind: BackendKind::ExternalVllm,
                api_root: "http://127.0.0.1:8000/v1".to_owned(),
                auth_method: BackendAuthMethod::None,
                api_key: None,
            })
            .await
            .expect("save first backend")
            .backends
            .into_iter()
            .find(|backend| backend.display_name == "第一个本地后端")
            .expect("first backend");
        let second = manager
            .save_backend(BackendDraft {
                id: None,
                display_name: "第二个本地后端".to_owned(),
                kind: BackendKind::ExternalLlamaCpp,
                api_root: "http://127.0.0.1:8080/v1".to_owned(),
                auth_method: BackendAuthMethod::None,
                api_key: None,
            })
            .await
            .expect("save second backend")
            .backends
            .into_iter()
            .find(|backend| backend.display_name == "第二个本地后端")
            .expect("second backend");

        manager
            .activate_backend(&first.id)
            .await
            .expect("activate first backend");
        let catalog = manager
            .force_activate_backend(&second.id)
            .await
            .expect("force activate second backend");

        assert_eq!(
            catalog.active_backend_id.as_deref(),
            Some(second.id.as_str())
        );
        assert_eq!(
            gateway.routing_snapshot().active_backend_id.as_deref(),
            Some(second.id.as_str())
        );
        assert_eq!(
            database
                .active_backend_id()
                .expect("persisted active backend"),
            Some(second.id)
        );
    }

    #[test]
    fn missing_keychain_secret_fails_closed_during_restore() {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        database
            .upsert_backend(&StoredBackendRecord {
                id: "locked-backend".to_owned(),
                display_name: "凭据缺失后端".to_owned(),
                kind: "external_openai".to_owned(),
                api_root: "http://127.0.0.1:8000/v1/".to_owned(),
                auth_style: "bearer".to_owned(),
                credential_id: Some("hal100.backend.locked-backend".to_owned()),
                enabled: true,
                created_at_ms: 1,
                updated_at_ms: 1,
            })
            .expect("backend metadata");
        database
            .upsert_model_route(&StoredModelRouteRecord {
                alias: "locked-model".to_owned(),
                backend_id: "locked-backend".to_owned(),
                resolved_model: "model".to_owned(),
                created_at_ms: 1,
                updated_at_ms: 1,
            })
            .expect("route metadata");
        database
            .set_active_backend_id(Some("locked-backend"), 1)
            .expect("active metadata");
        let gateway = GatewayState::new(
            None,
            CredentialRegistry::new(Vec::new()),
            UsageWriter::start(database.clone()),
        )
        .expect("gateway");
        let manager = BackendManager::new(
            database,
            gateway.clone(),
            Arc::new(MemorySecretStore::default()),
        );

        let report = manager.restore().expect("restore report");
        assert_eq!(report.loaded_backends, 0);
        assert_eq!(report.loaded_routes, 0);
        assert_eq!(report.skipped_backend_ids, vec!["locked-backend"]);
        assert_eq!(report.skipped_route_aliases, vec!["locked-model"]);
        assert!(!report.active_backend_restored);
        assert!(!gateway.has_backend());
        assert!(gateway.routing_snapshot().model_routes.is_empty());
    }

    #[tokio::test]
    async fn agent_route_alias_cannot_be_created_deleted_or_restored_by_user_configuration() {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        database
            .upsert_backend(&StoredBackendRecord {
                id: "attempted-user-backend".to_owned(),
                display_name: "保留路由测试后端".to_owned(),
                kind: "external_vllm".to_owned(),
                api_root: "http://127.0.0.1:8000/v1/".to_owned(),
                auth_style: "none".to_owned(),
                credential_id: None,
                enabled: true,
                created_at_ms: 1,
                updated_at_ms: 1,
            })
            .expect("backend fixture");
        database
            .upsert_model_route(&StoredModelRouteRecord {
                alias: INTERNAL_AGENT_MODEL_ALIAS.to_owned(),
                backend_id: "attempted-user-backend".to_owned(),
                resolved_model: "attempted-user-model".to_owned(),
                created_at_ms: 1,
                updated_at_ms: 1,
            })
            .expect("reserved route fixture");
        database
            .upsert_model_route(&StoredModelRouteRecord {
                alias: "hal100-agent-cloud-persisted".to_owned(),
                backend_id: "attempted-user-backend".to_owned(),
                resolved_model: "attempted-cloud-model".to_owned(),
                created_at_ms: 1,
                updated_at_ms: 1,
            })
            .expect("reserved cloud route fixture");
        let gateway = GatewayState::new(
            None,
            CredentialRegistry::new(Vec::new()),
            UsageWriter::start(database.clone()),
        )
        .expect("gateway");
        let manager = BackendManager::new(
            database,
            gateway.clone(),
            Arc::new(MemorySecretStore::default()),
        );

        assert!(matches!(
            manager
                .save_route(BackendRouteDraft {
                    alias: INTERNAL_AGENT_MODEL_ALIAS.to_owned(),
                    backend_id: "attempted-user-backend".to_owned(),
                    resolved_model: "attempted-user-model".to_owned(),
                })
                .await,
            Err(BackendManagerError::ReservedAgentRoute)
        ));
        assert!(matches!(
            manager.delete_route(INTERNAL_AGENT_MODEL_ALIAS).await,
            Err(BackendManagerError::ReservedAgentRoute)
        ));
        assert!(matches!(
            manager
                .save_route(BackendRouteDraft {
                    alias: "hal100-agent-cloud-forged".to_owned(),
                    backend_id: "attempted-user-backend".to_owned(),
                    resolved_model: "attempted-user-model".to_owned(),
                })
                .await,
            Err(BackendManagerError::ReservedAgentRoute)
        ));
        assert!(matches!(
            manager.delete_route("hal100-agent-cloud-persisted").await,
            Err(BackendManagerError::ReservedAgentRoute)
        ));

        let report = manager.restore().expect("restore report");
        assert_eq!(
            report.skipped_route_aliases,
            vec![INTERNAL_AGENT_MODEL_ALIAS, "hal100-agent-cloud-persisted"]
        );
        assert!(gateway.routing_snapshot().model_routes.is_empty());
    }
}
