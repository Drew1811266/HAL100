use std::{path::Path, sync::Mutex};

use hal100_protocol::{
    AuditDetail, AuditEventSummary, AuditLog, DataCleanupPreview, DataCleanupResult,
    DownloadSource, GenericClientSummary, LocalModelState, LocalModelSummary, ModelDownloadState,
    ModelLibrary, ModelOwnership, ModelRemovalKind, ModelSource, RetentionSettingsDraft,
    RuntimeProfileSupportCell, UsageDailySummary, UsageDashboard, UsageDimensionSummary,
    UsageFilterOption, UsageFilterOptions, UsageHourlySummary, UsageRequestSummary,
    UsageScopeQuery, UsageScopeSummary, UsageTotals,
};
use rusqlite::{Connection, Transaction, params};
use rusqlite_migration::{M, Migrations};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

const DEFAULT_USAGE_RETENTION_DAYS: Option<u16> = Some(90);
const DEFAULT_AUDIT_RETENTION_DAYS: Option<u16> = Some(365);
const MAX_AUDIT_EVENTS: u32 = 200;

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("SQLite operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("database migration failed: {0}")]
    Migration(#[from] rusqlite_migration::Error),
    #[error("database connection lock was poisoned")]
    LockPoisoned,
    #[error("database contains invalid HAL100 data: {0}")]
    InvalidData(String),
}

pub struct Database {
    connection: Mutex<Connection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredClientCredential {
    pub key_id: String,
    pub client_app_id: String,
    pub display_name: String,
    pub display_prefix: String,
    pub key_hash: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBackendRecord {
    pub id: String,
    pub display_name: String,
    pub kind: String,
    pub engine_kind: Option<String>,
    pub adapter_variant: Option<String>,
    pub api_root: String,
    pub auth_style: String,
    pub credential_id: Option<String>,
    pub enabled: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBackendEngineBinding {
    pub engine_kind: String,
    pub adapter_variant: String,
    pub deployment: String,
    pub config_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredModelRouteRecord {
    pub alias: String,
    pub backend_id: String,
    pub resolved_model: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoredActiveGatewayRoute {
    pub backend_id: String,
    pub resolved_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRuntimeProfileRecord {
    pub id: String,
    pub name: String,
    pub description: String,
    pub spec_version: u16,
    pub ownership: String,
    pub backend_id: Option<String>,
    pub backend_api_root: Option<String>,
    pub model_id: String,
    pub model_display_name: String,
    pub model_digest: String,
    pub model_digest_kind: String,
    pub engine: String,
    pub engine_version: String,
    pub capacity_tier: Option<String>,
    pub context_window_tokens: Option<u32>,
    pub capacity_revision: Option<String>,
    pub adapter_variant: String,
    pub adapter_contract_revision: String,
    pub backend_config_revision: Option<u64>,
    pub origin_fingerprint: Option<String>,
    pub evidence_kind: String,
    pub evidence_algorithm: String,
    pub evidence_value: String,
    pub protocol_capability_hash: String,
    pub support_cell: Option<RuntimeProfileSupportCell>,
    pub verified_at_ms: i64,
    pub last_activated_at_ms: Option<i64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRuntimeProfileVerification {
    pub model_digest: String,
    pub evidence_kind: String,
    pub evidence_algorithm: String,
    pub evidence_value: String,
    pub engine_version: String,
    pub capacity_tier: Option<String>,
    pub context_window_tokens: Option<u32>,
    pub capacity_revision: Option<String>,
    pub support_cell: Option<RuntimeProfileSupportCell>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeActivationPhase {
    Journaled,
    Quiesced,
    RouteSwitched,
    Compensating,
    RecoveryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRuntimeActivationJournal {
    pub id: String,
    pub profile_id: String,
    pub phase: RuntimeActivationPhase,
    pub previous_route: Option<StoredActiveGatewayRoute>,
    pub previous_managed_model_id: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageRequestRecord {
    pub request_id: String,
    pub client_app_id: String,
    pub protocol: String,
    pub requested_model: String,
    pub resolved_model: String,
    pub backend_id: String,
    pub started_at_ms: i64,
    pub first_token_at_ms: Option<i64>,
    pub completed_at_ms: i64,
    pub input_tokens: Option<i64>,
    pub cached_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub status: String,
    pub error_category: Option<String>,
    pub usage_accuracy: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedIntegrationRecord {
    pub id: String,
    pub kind: String,
    pub config_path: String,
    pub credential_path: String,
    pub managed_fragment_hash: [u8; 32],
    pub backup_path: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ManagedIntegrationResourceRole {
    Configuration,
    Credential,
    AuxiliaryConfiguration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedIntegrationResourceRecord {
    pub integration_id: String,
    pub role: ManagedIntegrationResourceRole,
    pub path: String,
    pub managed_content_hash: [u8; 32],
    pub backup_path: Option<String>,
    pub contains_secret: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadRecord {
    pub id: String,
    pub source: DownloadSource,
    pub repository: String,
    pub revision: String,
    pub file_name: String,
    pub state: ModelDownloadState,
    pub expected_size_bytes: u64,
    pub downloaded_bytes: u64,
    pub expected_sha256: [u8; 32],
    pub temporary_path: String,
    pub destination_path: String,
    pub error_code: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelIntegrityRecord {
    pub path: String,
    pub sha256: Option<[u8; 32]>,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DatabaseError> {
        let connection = Connection::open(path)?;
        Self::configure_and_migrate(connection)
    }

    pub fn open_in_memory() -> Result<Self, DatabaseError> {
        let connection = Connection::open_in_memory()?;
        Self::configure_and_migrate(connection)
    }

    pub fn schema_version(&self) -> Result<u32, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let version = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        Ok(version)
    }

    pub fn upsert_client_credential(
        &self,
        credential: &StoredClientCredential,
        now_ms: i64,
    ) -> Result<(), DatabaseError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO client_apps (id, display_name, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?3)
             ON CONFLICT(id) DO UPDATE SET
                display_name = excluded.display_name,
                updated_at_ms = excluded.updated_at_ms",
            params![credential.client_app_id, credential.display_name, now_ms],
        )?;
        transaction.execute(
            "INSERT INTO api_key_hashes
                (id, client_app_id, display_prefix, key_hash, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
                client_app_id = excluded.client_app_id,
                display_prefix = excluded.display_prefix,
                key_hash = excluded.key_hash",
            params![
                credential.key_id,
                credential.client_app_id,
                credential.display_prefix,
                credential.key_hash.as_slice(),
                now_ms
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn load_client_credentials(&self) -> Result<Vec<StoredClientCredential>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT k.id, k.client_app_id, c.display_name, k.display_prefix, k.key_hash
             FROM api_key_hashes k
             JOIN client_apps c ON c.id = k.client_app_id
             ORDER BY k.id",
        )?;
        let rows = statement.query_map([], |row| {
            let hash: Vec<u8> = row.get(4)?;
            let key_hash: [u8; 32] = hash.try_into().map_err(|_| {
                rusqlite::Error::FromSqlConversionFailure(
                    32,
                    rusqlite::types::Type::Blob,
                    "client key hash must contain 32 bytes".into(),
                )
            })?;
            Ok(StoredClientCredential {
                key_id: row.get(0)?,
                client_app_id: row.get(1)?,
                display_name: row.get(2)?,
                display_prefix: row.get(3)?,
                key_hash,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn generic_clients(&self) -> Result<Vec<GenericClientSummary>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT c.id, c.display_name, k.display_prefix, c.created_at_ms
             FROM client_apps c
             JOIN api_key_hashes k ON k.client_app_id = c.id
             WHERE c.id LIKE 'generic-%'
             ORDER BY c.created_at_ms DESC, c.id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(GenericClientSummary {
                client_app_id: row.get(0)?,
                display_name: row.get(1)?,
                display_prefix: row.get(2)?,
                created_at_ms: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn insert_generic_client_credential(
        &self,
        credential: &StoredClientCredential,
        now_ms: i64,
    ) -> Result<(), DatabaseError> {
        if !credential.client_app_id.starts_with("generic-") {
            return Err(DatabaseError::InvalidData(
                "generic client id must use the reserved prefix".to_owned(),
            ));
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO client_apps (id, display_name, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?3)",
            params![credential.client_app_id, credential.display_name, now_ms],
        )?;
        transaction.execute(
            "INSERT INTO api_key_hashes
                (id, client_app_id, display_prefix, key_hash, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                credential.key_id,
                credential.client_app_id,
                credential.display_prefix,
                credential.key_hash.as_slice(),
                now_ms
            ],
        )?;
        transaction.execute(
            "INSERT INTO audit_events (
                id, event_type, target_type, target_id, summary_json, created_at_ms
             ) VALUES (?1, 'generic_client_created', 'client', ?2, ?3, ?4)",
            params![
                Uuid::new_v4().to_string(),
                credential.client_app_id,
                json!({"displayName": credential.display_name}).to_string(),
                now_ms
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn revoke_generic_client(
        &self,
        client_app_id: &str,
        display_name: &str,
        now_ms: i64,
    ) -> Result<bool, DatabaseError> {
        if !client_app_id.starts_with("generic-") {
            return Err(DatabaseError::InvalidData(
                "only generic client credentials can be revoked here".to_owned(),
            ));
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        let deleted = transaction.execute(
            "DELETE FROM client_apps WHERE id = ?1 AND id LIKE 'generic-%'",
            [client_app_id],
        )?;
        if deleted > 0 {
            transaction.execute(
                "INSERT INTO audit_events (
                    id, event_type, target_type, target_id, summary_json, created_at_ms
                 ) VALUES (?1, 'generic_client_revoked', 'client', ?2, ?3, ?4)",
                params![
                    Uuid::new_v4().to_string(),
                    client_app_id,
                    json!({"displayName": display_name}).to_string(),
                    now_ms
                ],
            )?;
        }
        transaction.commit()?;
        Ok(deleted > 0)
    }

    pub fn upsert_backend(&self, backend: &StoredBackendRecord) -> Result<(), DatabaseError> {
        let (legacy_engine_kind, legacy_adapter_variant) = stored_backend_binding(&backend.kind);
        let engine_kind = backend.engine_kind.as_deref().or(legacy_engine_kind);
        let adapter_variant = backend
            .adapter_variant
            .as_deref()
            .or(legacy_adapter_variant);
        if engine_kind.is_some() != adapter_variant.is_some() {
            return Err(DatabaseError::InvalidData(
                "backend engine binding must contain both engine kind and adapter variant"
                    .to_owned(),
            ));
        }
        let deployment = reqwest::Url::parse(&backend.api_root)
            .ok()
            .filter(|url| url.scheme() == "http" && url.host_str() == Some("127.0.0.1"))
            .map_or("remote", |_| "local");
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection.execute(
            "INSERT INTO backends (
                id, display_name, kind, api_root, auth_style, credential_id,
                enabled, created_at_ms, updated_at_ms, engine_kind,
                adapter_variant, deployment, config_revision
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 1)
             ON CONFLICT(id) DO UPDATE SET
                display_name = excluded.display_name,
                kind = excluded.kind,
                api_root = excluded.api_root,
                auth_style = excluded.auth_style,
                credential_id = excluded.credential_id,
                enabled = excluded.enabled,
                engine_kind = excluded.engine_kind,
                adapter_variant = excluded.adapter_variant,
                deployment = excluded.deployment,
                config_revision = CASE WHEN
                    backends.kind IS NOT excluded.kind
                    OR backends.api_root IS NOT excluded.api_root
                    OR backends.auth_style IS NOT excluded.auth_style
                    OR backends.credential_id IS NOT excluded.credential_id
                    OR backends.enabled IS NOT excluded.enabled
                    OR backends.engine_kind IS NOT excluded.engine_kind
                    OR backends.adapter_variant IS NOT excluded.adapter_variant
                    OR backends.deployment IS NOT excluded.deployment
                    THEN backends.config_revision + 1
                    ELSE backends.config_revision
                END,
                updated_at_ms = excluded.updated_at_ms",
            params![
                backend.id,
                backend.display_name,
                backend.kind,
                backend.api_root,
                backend.auth_style,
                backend.credential_id,
                backend.enabled,
                backend.created_at_ms,
                backend.updated_at_ms,
                engine_kind,
                adapter_variant,
                deployment,
            ],
        )?;
        Ok(())
    }

    pub fn backend_config_revision(&self, backend_id: &str) -> Result<u64, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let revision = connection.query_row(
            "SELECT config_revision FROM backends WHERE id = ?1",
            [backend_id],
            |row| row.get::<_, i64>(0),
        )?;
        u64::try_from(revision).map_err(|_| {
            DatabaseError::InvalidData("backend config revision is invalid".to_owned())
        })
    }

    pub fn backend_engine_binding(
        &self,
        backend_id: &str,
    ) -> Result<Option<StoredBackendEngineBinding>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let result = connection.query_row(
            "SELECT engine_kind, adapter_variant, deployment, config_revision
             FROM backends WHERE id = ?1",
            [backend_id],
            |row| {
                let engine_kind = row.get::<_, Option<String>>(0)?;
                let adapter_variant = row.get::<_, Option<String>>(1)?;
                let deployment = row.get::<_, String>(2)?;
                let revision = row.get::<_, i64>(3)?;
                let config_revision = u64::try_from(revision)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(3, revision))?;
                match (engine_kind, adapter_variant) {
                    (Some(engine_kind), Some(adapter_variant)) => {
                        Ok(Some(StoredBackendEngineBinding {
                            engine_kind,
                            adapter_variant,
                            deployment,
                            config_revision,
                        }))
                    }
                    (None, None) => Ok(None),
                    _ => Err(invalid_column("backend engine binding")),
                }
            },
        );
        match result {
            Ok(binding) => Ok(binding),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn backends(&self) -> Result<Vec<StoredBackendRecord>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT id, display_name, kind, api_root, auth_style, credential_id,
                    enabled, created_at_ms, updated_at_ms, engine_kind, adapter_variant
             FROM backends ORDER BY display_name COLLATE NOCASE, id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(StoredBackendRecord {
                id: row.get(0)?,
                display_name: row.get(1)?,
                kind: row.get(2)?,
                engine_kind: row.get(9)?,
                adapter_variant: row.get(10)?,
                api_root: row.get(3)?,
                auth_style: row.get(4)?,
                credential_id: row.get(5)?,
                enabled: row.get(6)?,
                created_at_ms: row.get(7)?,
                updated_at_ms: row.get(8)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn delete_backend(&self, backend_id: &str) -> Result<bool, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        Ok(connection.execute("DELETE FROM backends WHERE id = ?1", [backend_id])? > 0)
    }

    pub fn upsert_model_route(&self, route: &StoredModelRouteRecord) -> Result<(), DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection.execute(
            "INSERT INTO model_routes (
                alias, backend_id, resolved_model, created_at_ms, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(alias) DO UPDATE SET
                backend_id = excluded.backend_id,
                resolved_model = excluded.resolved_model,
                updated_at_ms = excluded.updated_at_ms",
            params![
                route.alias,
                route.backend_id,
                route.resolved_model,
                route.created_at_ms,
                route.updated_at_ms,
            ],
        )?;
        Ok(())
    }

    pub fn model_routes(&self) -> Result<Vec<StoredModelRouteRecord>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT alias, backend_id, resolved_model, created_at_ms, updated_at_ms
             FROM model_routes ORDER BY alias",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(StoredModelRouteRecord {
                alias: row.get(0)?,
                backend_id: row.get(1)?,
                resolved_model: row.get(2)?,
                created_at_ms: row.get(3)?,
                updated_at_ms: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn delete_model_route(&self, alias: &str) -> Result<bool, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        Ok(connection.execute("DELETE FROM model_routes WHERE alias = ?1", [alias])? > 0)
    }

    pub fn active_backend_id(&self) -> Result<Option<String>, DatabaseError> {
        Ok(self.active_gateway_route()?.map(|route| route.backend_id))
    }

    pub fn active_gateway_route(&self) -> Result<Option<StoredActiveGatewayRoute>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let result = connection.query_row(
            "SELECT value_json FROM settings WHERE key = 'gateway.active_route'",
            [],
            |row| row.get::<_, String>(0),
        );
        match result {
            Ok(value) => {
                let route = serde_json::from_str::<Option<StoredActiveGatewayRoute>>(&value)
                    .map_err(|_| {
                        DatabaseError::InvalidData(
                            "gateway.active_route is not a valid route or null".to_owned(),
                        )
                    })?;
                validate_stored_active_route(route.as_ref())?;
                Ok(route)
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                let legacy = connection.query_row(
                    "SELECT value_json FROM settings WHERE key = 'gateway.active_backend_id'",
                    [],
                    |row| row.get::<_, String>(0),
                );
                match legacy {
                    Ok(value) => {
                        let backend_id =
                            serde_json::from_str::<Option<String>>(&value).map_err(|_| {
                                DatabaseError::InvalidData(
                                    "gateway.active_backend_id is not a string or null".to_owned(),
                                )
                            })?;
                        let route = backend_id.map(|backend_id| StoredActiveGatewayRoute {
                            backend_id,
                            resolved_model: None,
                        });
                        validate_stored_active_route(route.as_ref())?;
                        Ok(route)
                    }
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(error) => Err(error.into()),
                }
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn set_active_backend_id(
        &self,
        backend_id: Option<&str>,
        now_ms: i64,
    ) -> Result<(), DatabaseError> {
        let route = backend_id.map(|backend_id| StoredActiveGatewayRoute {
            backend_id: backend_id.to_owned(),
            resolved_model: None,
        });
        self.set_active_gateway_route(route.as_ref(), now_ms)
    }

    pub fn set_active_gateway_route(
        &self,
        route: Option<&StoredActiveGatewayRoute>,
        now_ms: i64,
    ) -> Result<(), DatabaseError> {
        validate_stored_active_route(route)?;
        let route_value = serde_json::to_string(&route).map_err(|_| {
            DatabaseError::InvalidData("gateway active route could not be encoded".to_owned())
        })?;
        let legacy_backend_id = route.map(|route| route.backend_id.as_str());
        let legacy_value = serde_json::to_string(&legacy_backend_id).map_err(|_| {
            DatabaseError::InvalidData("gateway active backend could not be encoded".to_owned())
        })?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO settings (key, value_json, updated_at)
             VALUES ('gateway.active_route', ?1, ?2)
             ON CONFLICT(key) DO UPDATE SET
                value_json = excluded.value_json,
                updated_at = excluded.updated_at",
            params![route_value, now_ms.to_string()],
        )?;
        transaction.execute(
            "INSERT INTO settings (key, value_json, updated_at)
             VALUES ('gateway.active_backend_id', ?1, ?2)
             ON CONFLICT(key) DO UPDATE SET
                value_json = excluded.value_json,
                updated_at = excluded.updated_at",
            params![legacy_value, now_ms.to_string()],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn insert_runtime_profile(
        &self,
        profile: &StoredRuntimeProfileRecord,
    ) -> Result<(), DatabaseError> {
        let context_window_tokens = profile.context_window_tokens.map(i64::from);
        let backend_config_revision = profile
            .backend_config_revision
            .map(|revision| sqlite_u64(revision, "backend config revision"))
            .transpose()?;
        let spec_version = i64::from(profile.spec_version);
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO runtime_profiles (
                id, name, description, spec_version, ownership, backend_id,
                backend_api_root, model_id, model_display_name, model_digest,
                model_digest_kind, engine, engine_version, capacity_tier,
                context_window_tokens, capacity_revision, adapter_variant,
                adapter_contract_revision, backend_config_revision, origin_fingerprint,
                evidence_kind, evidence_algorithm, evidence_value,
                protocol_capability_hash, support_platform, support_architecture,
                support_accelerator, support_deployment, verified_at_ms,
                last_activated_at_ms, created_at_ms, updated_at_ms
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20,
                ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30,
                ?31, ?32
             )",
            params![
                profile.id,
                profile.name,
                profile.description,
                spec_version,
                profile.ownership,
                profile.backend_id,
                profile.backend_api_root,
                profile.model_id,
                profile.model_display_name,
                profile.model_digest,
                profile.model_digest_kind,
                profile.engine,
                profile.engine_version,
                profile.capacity_tier,
                context_window_tokens,
                profile.capacity_revision,
                profile.adapter_variant,
                profile.adapter_contract_revision,
                backend_config_revision,
                profile.origin_fingerprint,
                profile.evidence_kind,
                profile.evidence_algorithm,
                profile.evidence_value,
                profile.protocol_capability_hash,
                profile.support_cell.map(|cell| cell.platform.storage_key()),
                profile
                    .support_cell
                    .map(|cell| cell.architecture.storage_key()),
                profile
                    .support_cell
                    .map(|cell| cell.accelerator.storage_key()),
                profile
                    .support_cell
                    .map(|cell| cell.deployment.storage_key()),
                profile.verified_at_ms,
                profile.last_activated_at_ms,
                profile.created_at_ms,
                profile.updated_at_ms,
            ],
        )?;
        transaction.execute(
            "INSERT INTO audit_events (
                id, event_type, target_type, target_id, summary_json, created_at_ms
             ) VALUES (?1, 'runtime_profile_saved', 'runtime_profile', ?2, ?3, ?4)",
            params![
                Uuid::new_v4().to_string(),
                profile.id,
                json!({
                    "name": profile.name,
                    "modelId": profile.model_id,
                    "engine": profile.engine,
                    "ownership": profile.ownership,
                    "backendId": profile.backend_id,
                    "capacityTier": profile.capacity_tier,
                    "supportCell": profile.support_cell,
                })
                .to_string(),
                profile.created_at_ms,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn runtime_profiles(&self) -> Result<Vec<StoredRuntimeProfileRecord>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT id, name, description, spec_version, ownership, backend_id,
                    backend_api_root, model_id, model_display_name, model_digest,
                    model_digest_kind, engine, engine_version, capacity_tier,
                    context_window_tokens, capacity_revision, adapter_variant,
                    adapter_contract_revision, backend_config_revision, origin_fingerprint,
                    evidence_kind, evidence_algorithm, evidence_value,
                    protocol_capability_hash, support_platform, support_architecture,
                    support_accelerator, support_deployment, verified_at_ms,
                    last_activated_at_ms, created_at_ms, updated_at_ms
             FROM runtime_profiles
             ORDER BY COALESCE(last_activated_at_ms, 0) DESC, updated_at_ms DESC, id",
        )?;
        let rows = statement.query_map([], runtime_profile_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn runtime_profile(
        &self,
        id: &str,
    ) -> Result<Option<StoredRuntimeProfileRecord>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let result = connection.query_row(
            "SELECT id, name, description, spec_version, ownership, backend_id,
                    backend_api_root, model_id, model_display_name, model_digest,
                    model_digest_kind, engine, engine_version, capacity_tier,
                    context_window_tokens, capacity_revision, adapter_variant,
                    adapter_contract_revision, backend_config_revision, origin_fingerprint,
                    evidence_kind, evidence_algorithm, evidence_value,
                    protocol_capability_hash, support_platform, support_architecture,
                    support_accelerator, support_deployment, verified_at_ms,
                    last_activated_at_ms, created_at_ms, updated_at_ms
             FROM runtime_profiles WHERE id = ?1",
            [id],
            runtime_profile_from_row,
        );
        match result {
            Ok(profile) => Ok(Some(profile)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn update_runtime_profile_metadata(
        &self,
        id: &str,
        name: &str,
        description: &str,
        now_ms: i64,
    ) -> Result<bool, DatabaseError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        let updated = transaction.execute(
            "UPDATE runtime_profiles
             SET name = ?1, description = ?2, updated_at_ms = ?3
             WHERE id = ?4",
            params![name, description, now_ms, id],
        )?;
        if updated > 0 {
            transaction.execute(
                "INSERT INTO audit_events (
                    id, event_type, target_type, target_id, summary_json, created_at_ms
                 ) VALUES (?1, 'runtime_profile_updated', 'runtime_profile', ?2, ?3, ?4)",
                params![
                    Uuid::new_v4().to_string(),
                    id,
                    json!({"name": name}).to_string(),
                    now_ms,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(updated > 0)
    }

    pub fn mark_runtime_profile_activated(
        &self,
        id: &str,
        verification: &StoredRuntimeProfileVerification,
        now_ms: i64,
    ) -> Result<bool, DatabaseError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        let updated = transaction.execute(
            "UPDATE runtime_profiles
             SET model_digest = ?1, engine_version = ?2, capacity_tier = ?3,
                 context_window_tokens = ?4, capacity_revision = ?5,
                 evidence_kind = ?6, evidence_algorithm = ?7, evidence_value = ?8,
                 support_platform = COALESCE(?9, support_platform),
                 support_architecture = COALESCE(?10, support_architecture),
                 support_accelerator = COALESCE(?11, support_accelerator),
                 support_deployment = COALESCE(?12, support_deployment),
                 verified_at_ms = ?13, last_activated_at_ms = ?13, updated_at_ms = ?13
             WHERE id = ?14",
            params![
                verification.model_digest,
                verification.engine_version,
                verification.capacity_tier,
                verification.context_window_tokens.map(i64::from),
                verification.capacity_revision,
                verification.evidence_kind,
                verification.evidence_algorithm,
                verification.evidence_value,
                verification
                    .support_cell
                    .map(|cell| cell.platform.storage_key()),
                verification
                    .support_cell
                    .map(|cell| cell.architecture.storage_key()),
                verification
                    .support_cell
                    .map(|cell| cell.accelerator.storage_key()),
                verification
                    .support_cell
                    .map(|cell| cell.deployment.storage_key()),
                now_ms,
                id,
            ],
        )?;
        if updated > 0 {
            transaction.execute(
                "INSERT INTO audit_events (
                    id, event_type, target_type, target_id, summary_json, created_at_ms
                 ) VALUES (?1, 'runtime_profile_activated', 'runtime_profile', ?2, ?3, ?4)",
                params![
                    Uuid::new_v4().to_string(),
                    id,
                    json!({
                        "engineVersion": verification.engine_version,
                        "capacityTier": verification.capacity_tier,
                        "contextWindowTokens": verification.context_window_tokens,
                    })
                    .to_string(),
                    now_ms,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(updated > 0)
    }

    pub fn reverify_runtime_profile(
        &self,
        id: &str,
        verification: &StoredRuntimeProfileVerification,
        now_ms: i64,
    ) -> Result<bool, DatabaseError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        let updated = transaction.execute(
            "UPDATE runtime_profiles
             SET model_digest = ?1, engine_version = ?2, capacity_tier = ?3,
                 context_window_tokens = ?4, capacity_revision = ?5,
                 evidence_kind = ?6, evidence_algorithm = ?7, evidence_value = ?8,
                 support_platform = COALESCE(?9, support_platform),
                 support_architecture = COALESCE(?10, support_architecture),
                 support_accelerator = COALESCE(?11, support_accelerator),
                 support_deployment = COALESCE(?12, support_deployment),
                 verified_at_ms = ?13, updated_at_ms = ?13
             WHERE id = ?14",
            params![
                verification.model_digest,
                verification.engine_version,
                verification.capacity_tier,
                verification.context_window_tokens.map(i64::from),
                verification.capacity_revision,
                verification.evidence_kind,
                verification.evidence_algorithm,
                verification.evidence_value,
                verification
                    .support_cell
                    .map(|cell| cell.platform.storage_key()),
                verification
                    .support_cell
                    .map(|cell| cell.architecture.storage_key()),
                verification
                    .support_cell
                    .map(|cell| cell.accelerator.storage_key()),
                verification
                    .support_cell
                    .map(|cell| cell.deployment.storage_key()),
                now_ms,
                id,
            ],
        )?;
        if updated > 0 {
            transaction.execute(
                "INSERT INTO audit_events (
                    id, event_type, target_type, target_id, summary_json, created_at_ms
                 ) VALUES (?1, 'runtime_profile_reverified', 'runtime_profile', ?2, ?3, ?4)",
                params![
                    Uuid::new_v4().to_string(),
                    id,
                    json!({
                        "engineVersion": verification.engine_version,
                        "capacityTier": verification.capacity_tier,
                        "contextWindowTokens": verification.context_window_tokens,
                    })
                    .to_string(),
                    now_ms,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(updated > 0)
    }

    pub fn delete_runtime_profile(
        &self,
        id: &str,
        name: &str,
        now_ms: i64,
    ) -> Result<bool, DatabaseError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        let deleted = transaction.execute("DELETE FROM runtime_profiles WHERE id = ?1", [id])?;
        if deleted > 0 {
            transaction.execute(
                "INSERT INTO audit_events (
                    id, event_type, target_type, target_id, summary_json, created_at_ms
                 ) VALUES (?1, 'runtime_profile_deleted', 'runtime_profile', ?2, ?3, ?4)",
                params![
                    Uuid::new_v4().to_string(),
                    id,
                    json!({"name": name}).to_string(),
                    now_ms,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(deleted > 0)
    }

    pub fn begin_runtime_activation(
        &self,
        journal: &StoredRuntimeActivationJournal,
    ) -> Result<(), DatabaseError> {
        if journal.phase != RuntimeActivationPhase::Journaled {
            return Err(DatabaseError::InvalidData(
                "runtime activation journal must begin in journaled phase".to_owned(),
            ));
        }
        validate_stored_active_route(journal.previous_route.as_ref())?;
        let previous_route_json = journal
            .previous_route
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|_| {
                DatabaseError::InvalidData(
                    "runtime activation previous route cannot be serialized".to_owned(),
                )
            })?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection.execute(
            "INSERT INTO runtime_activation_journal (
                id, profile_id, phase, previous_route_json,
                previous_managed_model_id, created_at_ms, updated_at_ms
             ) VALUES (?1, ?2, 'journaled', ?3, ?4, ?5, ?5)",
            params![
                journal.id,
                journal.profile_id,
                previous_route_json,
                journal.previous_managed_model_id,
                journal.created_at_ms,
            ],
        )?;
        Ok(())
    }

    pub fn runtime_activation_journals(
        &self,
    ) -> Result<Vec<StoredRuntimeActivationJournal>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT id, profile_id, phase, previous_route_json,
                    previous_managed_model_id, created_at_ms, updated_at_ms
             FROM runtime_activation_journal
             ORDER BY created_at_ms, id",
        )?;
        let rows = statement.query_map([], runtime_activation_journal_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn transition_runtime_activation(
        &self,
        id: &str,
        expected: RuntimeActivationPhase,
        next: RuntimeActivationPhase,
        now_ms: i64,
    ) -> Result<bool, DatabaseError> {
        if !valid_runtime_activation_transition(expected, next) {
            return Err(DatabaseError::InvalidData(
                "runtime activation journal transition is invalid".to_owned(),
            ));
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let updated = connection.execute(
            "UPDATE runtime_activation_journal
             SET phase = ?1, updated_at_ms = ?2
             WHERE id = ?3 AND phase = ?4",
            params![
                runtime_activation_phase_key(next),
                now_ms,
                id,
                runtime_activation_phase_key(expected),
            ],
        )?;
        Ok(updated == 1)
    }

    pub fn finish_runtime_activation(
        &self,
        id: &str,
        expected: RuntimeActivationPhase,
    ) -> Result<bool, DatabaseError> {
        if expected == RuntimeActivationPhase::RecoveryRequired {
            return Err(DatabaseError::InvalidData(
                "recovery-required activation cannot be silently completed".to_owned(),
            ));
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let deleted = connection.execute(
            "DELETE FROM runtime_activation_journal WHERE id = ?1 AND phase = ?2",
            params![id, runtime_activation_phase_key(expected)],
        )?;
        Ok(deleted == 1)
    }

    pub fn onboarding_state(&self) -> Result<(bool, u8, bool), DatabaseError> {
        let completed = self
            .setting_json("desktop.onboarding_completed")?
            .map(|value| decode_setting::<bool>(&value, "desktop.onboarding_completed"))
            .transpose()?
            .unwrap_or(false);
        let step = self
            .setting_json("desktop.onboarding_step")?
            .map(|value| decode_setting::<u8>(&value, "desktop.onboarding_step"))
            .transpose()?
            .unwrap_or(1);
        if !(1..=5).contains(&step) {
            return Err(DatabaseError::InvalidData(
                "desktop.onboarding_step is outside the supported range".to_owned(),
            ));
        }
        let launch_at_login_asked = self
            .setting_json("desktop.launch_at_login_asked")?
            .map(|value| decode_setting::<bool>(&value, "desktop.launch_at_login_asked"))
            .transpose()?
            .unwrap_or(false);
        Ok((completed, step, launch_at_login_asked))
    }

    pub fn set_onboarding_step(&self, step: u8, now_ms: i64) -> Result<(), DatabaseError> {
        if !(1..=5).contains(&step) {
            return Err(DatabaseError::InvalidData(
                "onboarding step must be between 1 and 5".to_owned(),
            ));
        }
        self.set_setting_json("desktop.onboarding_step", &step.to_string(), now_ms)
    }

    pub fn complete_onboarding(&self, now_ms: i64) -> Result<(), DatabaseError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        upsert_setting(&transaction, "desktop.onboarding_completed", "true", now_ms)?;
        upsert_setting(&transaction, "desktop.onboarding_step", "5", now_ms)?;
        transaction.execute(
            "INSERT INTO audit_events (
                id, event_type, target_type, target_id, summary_json, created_at_ms
             ) VALUES (?1, 'onboarding_completed', 'desktop', 'onboarding', '{}', ?2)",
            params![Uuid::new_v4().to_string(), now_ms],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn mark_launch_at_login_asked(
        &self,
        enabled: bool,
        now_ms: i64,
    ) -> Result<(), DatabaseError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        upsert_setting(
            &transaction,
            "desktop.launch_at_login_asked",
            "true",
            now_ms,
        )?;
        transaction.execute(
            "INSERT INTO audit_events (
                id, event_type, target_type, target_id, summary_json, created_at_ms
             ) VALUES (?1, 'launch_at_login_changed', 'desktop', 'launch-at-login', ?2, ?3)",
            params![
                Uuid::new_v4().to_string(),
                json!({"enabled": enabled}).to_string(),
                now_ms
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn retention_settings(&self) -> Result<RetentionSettingsDraft, DatabaseError> {
        let usage_retention_days = self
            .setting_json("data.usage_retention_days")?
            .map(|value| decode_setting::<Option<u16>>(&value, "data.usage_retention_days"))
            .transpose()?
            .unwrap_or(DEFAULT_USAGE_RETENTION_DAYS);
        let audit_retention_days = self
            .setting_json("data.audit_retention_days")?
            .map(|value| decode_setting::<Option<u16>>(&value, "data.audit_retention_days"))
            .transpose()?
            .unwrap_or(DEFAULT_AUDIT_RETENTION_DAYS);
        validate_retention_days(usage_retention_days)?;
        validate_retention_days(audit_retention_days)?;
        Ok(RetentionSettingsDraft {
            usage_retention_days,
            audit_retention_days,
        })
    }

    pub fn set_retention_settings(
        &self,
        draft: RetentionSettingsDraft,
        now_ms: i64,
    ) -> Result<RetentionSettingsDraft, DatabaseError> {
        validate_retention_days(draft.usage_retention_days)?;
        validate_retention_days(draft.audit_retention_days)?;
        let usage_json = serde_json::to_string(&draft.usage_retention_days).map_err(|_| {
            DatabaseError::InvalidData("usage retention could not be encoded".to_owned())
        })?;
        let audit_json = serde_json::to_string(&draft.audit_retention_days).map_err(|_| {
            DatabaseError::InvalidData("audit retention could not be encoded".to_owned())
        })?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        upsert_setting(
            &transaction,
            "data.usage_retention_days",
            &usage_json,
            now_ms,
        )?;
        upsert_setting(
            &transaction,
            "data.audit_retention_days",
            &audit_json,
            now_ms,
        )?;
        transaction.execute(
            "INSERT INTO audit_events (
                id, event_type, target_type, target_id, summary_json, created_at_ms
             ) VALUES (?1, 'retention_settings_changed', 'data', 'retention', ?2, ?3)",
            params![
                Uuid::new_v4().to_string(),
                json!({
                    "usageRetentionDays": draft.usage_retention_days,
                    "auditRetentionDays": draft.audit_retention_days,
                })
                .to_string(),
                now_ms
            ],
        )?;
        transaction.commit()?;
        Ok(draft)
    }

    pub fn data_cleanup_preview(&self, now_ms: i64) -> Result<DataCleanupPreview, DatabaseError> {
        let settings = self.retention_settings()?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let usage_request_count = count_expired(
            &connection,
            "usage_requests",
            "completed_at_ms",
            settings.usage_retention_days,
            now_ms,
        )?;
        let audit_event_count = count_expired(
            &connection,
            "audit_events",
            "created_at_ms",
            settings.audit_retention_days,
            now_ms,
        )?;
        Ok(DataCleanupPreview {
            usage_request_count,
            audit_event_count,
            usage_retention_days: settings.usage_retention_days,
            audit_retention_days: settings.audit_retention_days,
        })
    }

    pub fn apply_data_retention(&self, now_ms: i64) -> Result<DataCleanupResult, DatabaseError> {
        let settings = self.retention_settings()?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        let usage_requests_deleted = delete_expired(
            &transaction,
            "usage_requests",
            "completed_at_ms",
            settings.usage_retention_days,
            now_ms,
        )?;
        let audit_events_deleted = delete_expired(
            &transaction,
            "audit_events",
            "created_at_ms",
            settings.audit_retention_days,
            now_ms,
        )?;
        transaction.execute(
            "INSERT INTO audit_events (
                id, event_type, target_type, target_id, summary_json, created_at_ms
             ) VALUES (?1, 'data_retention_applied', 'data', 'retention', ?2, ?3)",
            params![
                Uuid::new_v4().to_string(),
                json!({
                    "usageDeleted": usage_requests_deleted,
                    "auditDeleted": audit_events_deleted,
                })
                .to_string(),
                now_ms
            ],
        )?;
        transaction.commit()?;
        Ok(DataCleanupResult {
            usage_requests_deleted,
            audit_events_deleted,
        })
    }

    pub fn audit_log(&self, limit: u32) -> Result<AuditLog, DatabaseError> {
        let limit = limit.clamp(1, MAX_AUDIT_EVENTS);
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let count: i64 =
            connection.query_row("SELECT COUNT(*) FROM audit_events", [], |row| row.get(0))?;
        let mut statement = connection.prepare(
            "SELECT id, event_type, target_type, target_id, summary_json, created_at_ms
             FROM audit_events ORDER BY created_at_ms DESC, id DESC LIMIT ?1",
        )?;
        let events = statement
            .query_map([limit], |row| {
                let summary_json = row.get::<_, String>(4)?;
                Ok(AuditEventSummary {
                    id: row.get(0)?,
                    event_type: row.get(1)?,
                    target_type: row.get(2)?,
                    target_id: row.get(3)?,
                    details: safe_audit_details(&summary_json),
                    created_at_ms: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(AuditLog {
            total_count: u64::try_from(count).unwrap_or(0),
            events,
        })
    }

    fn setting_json(&self, key: &str) -> Result<Option<String>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let result = connection.query_row(
            "SELECT value_json FROM settings WHERE key = ?1",
            [key],
            |row| row.get::<_, String>(0),
        );
        match result {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn set_setting_json(
        &self,
        key: &str,
        value_json: &str,
        now_ms: i64,
    ) -> Result<(), DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        upsert_setting(&connection, key, value_json, now_ms)?;
        Ok(())
    }

    pub fn managed_integration(
        &self,
        id: &str,
    ) -> Result<Option<ManagedIntegrationRecord>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let result = connection.query_row(
            "SELECT id, kind, config_path, credential_path, managed_fragment_hash,
                    backup_path, created_at_ms, updated_at_ms
             FROM integrations WHERE id = ?1",
            [id],
            |row| {
                let hash: Vec<u8> = row.get(4)?;
                let managed_fragment_hash: [u8; 32] = hash.try_into().map_err(|_| {
                    rusqlite::Error::FromSqlConversionFailure(
                        32,
                        rusqlite::types::Type::Blob,
                        "managed fragment hash must contain 32 bytes".into(),
                    )
                })?;
                Ok(ManagedIntegrationRecord {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    config_path: row.get(2)?,
                    credential_path: row.get(3)?,
                    managed_fragment_hash,
                    backup_path: row.get(5)?,
                    created_at_ms: row.get(6)?,
                    updated_at_ms: row.get(7)?,
                })
            },
        );
        match result {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn managed_integration_resources(
        &self,
        integration_id: &str,
    ) -> Result<Vec<ManagedIntegrationResourceRecord>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT integration_id, role, path, managed_content_hash, backup_path,
                    contains_secret
             FROM integration_resources
             WHERE integration_id = ?1
             ORDER BY role",
        )?;
        let rows = statement.query_map([integration_id], |row| {
            let hash: Vec<u8> = row.get(3)?;
            let managed_content_hash: [u8; 32] = hash
                .try_into()
                .map_err(|_| invalid_column("managed integration resource hash"))?;
            Ok(ManagedIntegrationResourceRecord {
                integration_id: row.get(0)?,
                role: parse_managed_resource_role(&row.get::<_, String>(1)?)?,
                path: row.get(2)?,
                managed_content_hash,
                backup_path: row.get(4)?,
                contains_secret: row.get::<_, bool>(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn upsert_integration_and_credential(
        &self,
        integration: &ManagedIntegrationRecord,
        credential: &StoredClientCredential,
    ) -> Result<(), DatabaseError> {
        let resources = [
            ManagedIntegrationResourceRecord {
                integration_id: integration.id.clone(),
                role: ManagedIntegrationResourceRole::Configuration,
                path: integration.config_path.clone(),
                managed_content_hash: integration.managed_fragment_hash,
                backup_path: integration.backup_path.clone(),
                contains_secret: false,
            },
            ManagedIntegrationResourceRecord {
                integration_id: integration.id.clone(),
                role: ManagedIntegrationResourceRole::Credential,
                path: integration.credential_path.clone(),
                managed_content_hash: credential.key_hash,
                backup_path: None,
                contains_secret: true,
            },
        ];
        self.upsert_integration_resources_and_credential(integration, &resources, credential)
    }

    pub fn upsert_integration_resources_and_credential(
        &self,
        integration: &ManagedIntegrationRecord,
        resources: &[ManagedIntegrationResourceRecord],
        credential: &StoredClientCredential,
    ) -> Result<(), DatabaseError> {
        if resources.is_empty() {
            return Err(DatabaseError::InvalidData(
                "managed integration must own at least one resource".to_owned(),
            ));
        }
        let mut roles = std::collections::HashSet::new();
        for resource in resources {
            if resource.integration_id != integration.id {
                return Err(DatabaseError::InvalidData(
                    "managed integration resource belongs to another integration".to_owned(),
                ));
            }
            if !roles.insert(resource.role) {
                return Err(DatabaseError::InvalidData(
                    "managed integration resource roles must be unique".to_owned(),
                ));
            }
            if resource.contains_secret && resource.backup_path.is_some() {
                return Err(DatabaseError::InvalidData(
                    "secret resources cannot have plaintext backups".to_owned(),
                ));
            }
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO client_apps (id, display_name, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?3)
             ON CONFLICT(id) DO UPDATE SET
                display_name = excluded.display_name,
                updated_at_ms = excluded.updated_at_ms",
            params![
                credential.client_app_id,
                credential.display_name,
                integration.updated_at_ms
            ],
        )?;
        transaction.execute(
            "INSERT INTO api_key_hashes
                (id, client_app_id, display_prefix, key_hash, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
                client_app_id = excluded.client_app_id,
                display_prefix = excluded.display_prefix,
                key_hash = excluded.key_hash",
            params![
                credential.key_id,
                credential.client_app_id,
                credential.display_prefix,
                credential.key_hash.as_slice(),
                integration.updated_at_ms
            ],
        )?;
        transaction.execute(
            "INSERT INTO integrations (
                id, kind, config_path, credential_path, managed_fragment_hash,
                backup_path, created_at_ms, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                kind = excluded.kind,
                config_path = excluded.config_path,
                credential_path = excluded.credential_path,
                managed_fragment_hash = excluded.managed_fragment_hash,
                backup_path = excluded.backup_path,
                updated_at_ms = excluded.updated_at_ms",
            params![
                integration.id,
                integration.kind,
                integration.config_path,
                integration.credential_path,
                integration.managed_fragment_hash.as_slice(),
                integration.backup_path,
                integration.created_at_ms,
                integration.updated_at_ms,
            ],
        )?;
        transaction.execute(
            "DELETE FROM integration_resources WHERE integration_id = ?1",
            [&integration.id],
        )?;
        for resource in resources {
            transaction.execute(
                "INSERT INTO integration_resources (
                    integration_id, role, path, managed_content_hash, backup_path,
                    contains_secret
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    resource.integration_id,
                    managed_resource_role_key(resource.role),
                    resource.path,
                    resource.managed_content_hash.as_slice(),
                    resource.backup_path,
                    resource.contains_secret,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn remove_managed_integration_and_client(
        &self,
        integration_id: &str,
        client_app_id: &str,
        now_ms: i64,
    ) -> Result<bool, DatabaseError> {
        if integration_id.trim().is_empty() || client_app_id.trim().is_empty() {
            return Err(DatabaseError::InvalidData(
                "managed integration and client identifiers must be non-empty".to_owned(),
            ));
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        let removed =
            transaction.execute("DELETE FROM integrations WHERE id = ?1", [integration_id])?;
        if removed == 0 {
            transaction.rollback()?;
            return Ok(false);
        }
        transaction.execute("DELETE FROM client_apps WHERE id = ?1", [client_app_id])?;
        transaction.execute(
            "INSERT INTO audit_events (
                id, event_type, target_type, target_id, summary_json, created_at_ms
             ) VALUES (?1, 'external_integration_disconnected', 'integration', ?2, ?3, ?4)",
            params![
                Uuid::new_v4().to_string(),
                integration_id,
                json!({"clientAppId": client_app_id}).to_string(),
                now_ms,
            ],
        )?;
        transaction.commit()?;
        Ok(true)
    }

    pub fn insert_usage_request(&self, usage: &UsageRequestRecord) -> Result<(), DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection.execute(
            "INSERT INTO usage_requests (
                request_id, client_app_id, protocol, requested_model, resolved_model,
                backend_id, started_at_ms, first_token_at_ms, completed_at_ms,
                input_tokens, cached_tokens, output_tokens, total_tokens,
                status, error_category, usage_accuracy
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                ?10, ?11, ?12, ?13, ?14, ?15, ?16
             )",
            params![
                usage.request_id,
                usage.client_app_id,
                usage.protocol,
                usage.requested_model,
                usage.resolved_model,
                usage.backend_id,
                usage.started_at_ms,
                usage.first_token_at_ms,
                usage.completed_at_ms,
                usage.input_tokens,
                usage.cached_tokens,
                usage.output_tokens,
                usage.total_tokens,
                usage.status,
                usage.error_category,
                usage.usage_accuracy,
            ],
        )?;
        Ok(())
    }

    pub fn usage_request_count(&self) -> Result<u64, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let count: i64 =
            connection.query_row("SELECT COUNT(*) FROM usage_requests", [], |row| row.get(0))?;
        Ok(u64::try_from(count).unwrap_or(0))
    }

    pub fn usage_request_count_for_client(
        &self,
        client_app_id: &str,
    ) -> Result<u64, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM usage_requests WHERE client_app_id = ?1",
            [client_app_id],
            |row| row.get(0),
        )?;
        Ok(u64::try_from(count).unwrap_or(0))
    }

    pub fn usage_dashboard(
        &self,
        limit: u32,
        activity_since_ms: i64,
    ) -> Result<UsageDashboard, DatabaseError> {
        let limit = limit.clamp(1, 100);
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let totals = connection.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(input_tokens), 0),
                    COALESCE(SUM(cached_tokens), 0),
                    COALESCE(SUM(output_tokens), 0),
                    COALESCE(SUM(total_tokens), 0)
             FROM usage_requests",
            [],
            |row| {
                Ok(UsageTotals {
                    request_count: nonnegative_i64(row.get(0)?, 0)?,
                    input_tokens: nonnegative_i64(row.get(1)?, 1)?,
                    cached_tokens: nonnegative_i64(row.get(2)?, 2)?,
                    output_tokens: nonnegative_i64(row.get(3)?, 3)?,
                    total_tokens: nonnegative_i64(row.get(4)?, 4)?,
                })
            },
        )?;
        let mut statement = connection.prepare(
            "SELECT u.request_id, u.client_app_id,
                    COALESCE(c.display_name, u.client_app_id),
                    u.requested_model, u.resolved_model, u.backend_id,
                    u.started_at_ms, u.completed_at_ms,
                    u.input_tokens, u.cached_tokens, u.output_tokens, u.total_tokens,
                    u.status, u.usage_accuracy
             FROM usage_requests u
             LEFT JOIN client_apps c ON c.id = u.client_app_id
             ORDER BY u.started_at_ms DESC, u.request_id
             LIMIT ?1",
        )?;
        let requests = statement.query_map([limit], |row| {
            Ok(UsageRequestSummary {
                request_id: row.get(0)?,
                client_app_id: row.get(1)?,
                client_display_name: row.get(2)?,
                requested_model: row.get(3)?,
                resolved_model: row.get(4)?,
                backend_id: row.get(5)?,
                started_at_ms: row.get(6)?,
                completed_at_ms: row.get(7)?,
                input_tokens: optional_nonnegative_i64(row.get(8)?, 8)?,
                cached_tokens: optional_nonnegative_i64(row.get(9)?, 9)?,
                output_tokens: optional_nonnegative_i64(row.get(10)?, 10)?,
                total_tokens: optional_nonnegative_i64(row.get(11)?, 11)?,
                status: row.get(12)?,
                usage_accuracy: row.get(13)?,
            })
        })?;
        let recent_requests = requests.collect::<Result<Vec<_>, _>>()?;
        let mut daily_statement = connection.prepare(
            "SELECT strftime('%Y-%m-%d', started_at_ms / 1000, 'unixepoch', 'localtime'),
                    COUNT(*),
                    COALESCE(SUM(input_tokens), 0),
                    COALESCE(SUM(cached_tokens), 0),
                    COALESCE(SUM(output_tokens), 0),
                    COALESCE(SUM(total_tokens), 0)
             FROM usage_requests
             WHERE started_at_ms >= ?1
             GROUP BY 1
             ORDER BY 1 ASC",
        )?;
        let daily_usage = daily_statement
            .query_map([activity_since_ms.max(0)], |row| {
                Ok(UsageDailySummary {
                    date: row.get(0)?,
                    request_count: nonnegative_i64(row.get(1)?, 1)?,
                    input_tokens: nonnegative_i64(row.get(2)?, 2)?,
                    cached_tokens: nonnegative_i64(row.get(3)?, 3)?,
                    output_tokens: nonnegative_i64(row.get(4)?, 4)?,
                    total_tokens: nonnegative_i64(row.get(5)?, 5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(UsageDashboard {
            totals,
            recent_requests,
            daily_usage,
        })
    }

    pub fn usage_scope(&self, query: &UsageScopeQuery) -> Result<UsageScopeSummary, DatabaseError> {
        validate_usage_scope_query(query)?;
        let limit = query.limit.clamp(1, 100);
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let (
            request_count,
            input_tokens,
            cached_tokens,
            output_tokens,
            total_tokens,
            measured_request_count,
            succeeded_request_count,
        ) = connection.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(u.input_tokens), 0),
                    COALESCE(SUM(u.cached_tokens), 0),
                    COALESCE(SUM(u.output_tokens), 0),
                    COALESCE(SUM(u.total_tokens), 0),
                    COUNT(u.total_tokens),
                    COALESCE(SUM(CASE WHEN u.status = 'succeeded' THEN 1 ELSE 0 END), 0)
             FROM usage_requests u
             WHERE u.started_at_ms >= ?1 AND u.started_at_ms < ?2
               AND (?3 IS NULL OR u.client_app_id = ?3)
               AND (?4 IS NULL OR u.resolved_model = ?4)
               AND (?5 IS NULL OR u.backend_id = ?5)
               AND (?6 IS NULL OR u.status = ?6)",
            params![
                query.start_at_ms,
                query.end_at_ms_exclusive,
                query.client_app_id.as_deref(),
                query.resolved_model.as_deref(),
                query.backend_id.as_deref(),
                query.status.as_deref(),
            ],
            |row| {
                Ok((
                    nonnegative_i64(row.get(0)?, 0)?,
                    nonnegative_i64(row.get(1)?, 1)?,
                    nonnegative_i64(row.get(2)?, 2)?,
                    nonnegative_i64(row.get(3)?, 3)?,
                    nonnegative_i64(row.get(4)?, 4)?,
                    nonnegative_i64(row.get(5)?, 5)?,
                    nonnegative_i64(row.get(6)?, 6)?,
                ))
            },
        )?;
        let totals = UsageTotals {
            request_count,
            input_tokens,
            cached_tokens,
            output_tokens,
            total_tokens,
        };

        let mut client_statement = connection.prepare(
            "SELECT u.client_app_id, COALESCE(c.display_name, u.client_app_id), COUNT(*),
                    COALESCE(SUM(u.total_tokens), 0)
             FROM usage_requests u
             LEFT JOIN client_apps c ON c.id = u.client_app_id
             WHERE u.started_at_ms >= ?1 AND u.started_at_ms < ?2
               AND (?3 IS NULL OR u.client_app_id = ?3)
               AND (?4 IS NULL OR u.resolved_model = ?4)
               AND (?5 IS NULL OR u.backend_id = ?5)
               AND (?6 IS NULL OR u.status = ?6)
             GROUP BY u.client_app_id, COALESCE(c.display_name, u.client_app_id)
             ORDER BY 4 DESC, 3 DESC, 2 ASC
             LIMIT 5",
        )?;
        let client_usage = client_statement
            .query_map(
                params![
                    query.start_at_ms,
                    query.end_at_ms_exclusive,
                    query.client_app_id.as_deref(),
                    query.resolved_model.as_deref(),
                    query.backend_id.as_deref(),
                    query.status.as_deref(),
                ],
                |row| {
                    Ok(UsageDimensionSummary {
                        id: row.get(0)?,
                        display_name: row.get(1)?,
                        request_count: nonnegative_i64(row.get(2)?, 2)?,
                        total_tokens: nonnegative_i64(row.get(3)?, 3)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;

        let mut request_statement = connection.prepare(
            "SELECT u.request_id, u.client_app_id,
                    COALESCE(c.display_name, u.client_app_id),
                    u.requested_model, u.resolved_model, u.backend_id,
                    u.started_at_ms, u.completed_at_ms,
                    u.input_tokens, u.cached_tokens, u.output_tokens, u.total_tokens,
                    u.status, u.usage_accuracy
             FROM usage_requests u
             LEFT JOIN client_apps c ON c.id = u.client_app_id
             WHERE u.started_at_ms >= ?1 AND u.started_at_ms < ?2
               AND (?3 IS NULL OR u.client_app_id = ?3)
               AND (?4 IS NULL OR u.resolved_model = ?4)
               AND (?5 IS NULL OR u.backend_id = ?5)
               AND (?6 IS NULL OR u.status = ?6)
             ORDER BY u.started_at_ms DESC, u.request_id
             LIMIT ?7",
        )?;
        let recent_requests = request_statement
            .query_map(
                params![
                    query.start_at_ms,
                    query.end_at_ms_exclusive,
                    query.client_app_id.as_deref(),
                    query.resolved_model.as_deref(),
                    query.backend_id.as_deref(),
                    query.status.as_deref(),
                    limit,
                ],
                |row| {
                    Ok(UsageRequestSummary {
                        request_id: row.get(0)?,
                        client_app_id: row.get(1)?,
                        client_display_name: row.get(2)?,
                        requested_model: row.get(3)?,
                        resolved_model: row.get(4)?,
                        backend_id: row.get(5)?,
                        started_at_ms: row.get(6)?,
                        completed_at_ms: row.get(7)?,
                        input_tokens: optional_nonnegative_i64(row.get(8)?, 8)?,
                        cached_tokens: optional_nonnegative_i64(row.get(9)?, 9)?,
                        output_tokens: optional_nonnegative_i64(row.get(10)?, 10)?,
                        total_tokens: optional_nonnegative_i64(row.get(11)?, 11)?,
                        status: row.get(12)?,
                        usage_accuracy: row.get(13)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;

        let mut daily_statement = connection.prepare(
            "SELECT strftime('%Y-%m-%d', u.started_at_ms / 1000, 'unixepoch', 'localtime'),
                    COUNT(*), COALESCE(SUM(u.input_tokens), 0),
                    COALESCE(SUM(u.cached_tokens), 0), COALESCE(SUM(u.output_tokens), 0),
                    COALESCE(SUM(u.total_tokens), 0)
             FROM usage_requests u
             WHERE u.started_at_ms >= ?1 AND u.started_at_ms < ?2
               AND (?3 IS NULL OR u.client_app_id = ?3)
               AND (?4 IS NULL OR u.resolved_model = ?4)
               AND (?5 IS NULL OR u.backend_id = ?5)
               AND (?6 IS NULL OR u.status = ?6)
             GROUP BY 1 ORDER BY 1 ASC",
        )?;
        let daily_usage = daily_statement
            .query_map(
                params![
                    query.series_start_at_ms,
                    query.series_end_at_ms_exclusive,
                    query.client_app_id.as_deref(),
                    query.resolved_model.as_deref(),
                    query.backend_id.as_deref(),
                    query.status.as_deref(),
                ],
                |row| {
                    Ok(UsageDailySummary {
                        date: row.get(0)?,
                        request_count: nonnegative_i64(row.get(1)?, 1)?,
                        input_tokens: nonnegative_i64(row.get(2)?, 2)?,
                        cached_tokens: nonnegative_i64(row.get(3)?, 3)?,
                        output_tokens: nonnegative_i64(row.get(4)?, 4)?,
                        total_tokens: nonnegative_i64(row.get(5)?, 5)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;

        let mut hourly_statement = connection.prepare(
            "SELECT CAST(strftime('%H', u.started_at_ms / 1000, 'unixepoch', 'localtime') AS INTEGER),
                    COUNT(*), COALESCE(SUM(u.input_tokens), 0),
                    COALESCE(SUM(u.cached_tokens), 0), COALESCE(SUM(u.output_tokens), 0),
                    COALESCE(SUM(u.total_tokens), 0)
             FROM usage_requests u
             WHERE u.started_at_ms >= ?1 AND u.started_at_ms < ?2
               AND (?3 IS NULL OR u.client_app_id = ?3)
               AND (?4 IS NULL OR u.resolved_model = ?4)
               AND (?5 IS NULL OR u.backend_id = ?5)
               AND (?6 IS NULL OR u.status = ?6)
             GROUP BY 1 ORDER BY 1 ASC",
        )?;
        let hourly_usage = hourly_statement
            .query_map(
                params![
                    query.start_at_ms,
                    query.end_at_ms_exclusive,
                    query.client_app_id.as_deref(),
                    query.resolved_model.as_deref(),
                    query.backend_id.as_deref(),
                    query.status.as_deref(),
                ],
                |row| {
                    let hour = u8::try_from(row.get::<_, i64>(0)?)
                        .ok()
                        .filter(|hour| *hour < 24)
                        .ok_or_else(|| invalid_column("usage hour"))?;
                    Ok(UsageHourlySummary {
                        hour,
                        request_count: nonnegative_i64(row.get(1)?, 1)?,
                        input_tokens: nonnegative_i64(row.get(2)?, 2)?,
                        cached_tokens: nonnegative_i64(row.get(3)?, 3)?,
                        output_tokens: nonnegative_i64(row.get(4)?, 4)?,
                        total_tokens: nonnegative_i64(row.get(5)?, 5)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(UsageScopeSummary {
            totals,
            measured_request_count,
            succeeded_request_count,
            client_usage,
            recent_requests,
            daily_usage,
            hourly_usage,
        })
    }

    pub fn usage_filter_options(&self) -> Result<UsageFilterOptions, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let (earliest_usage_at_ms, latest_usage_at_ms) = connection.query_row(
            "SELECT MIN(started_at_ms), MAX(started_at_ms) FROM usage_requests",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let mut client_statement = connection.prepare(
            "SELECT DISTINCT u.client_app_id, COALESCE(c.display_name, u.client_app_id)
             FROM usage_requests u LEFT JOIN client_apps c ON c.id = u.client_app_id
             ORDER BY 2 ASC",
        )?;
        let clients = client_statement
            .query_map([], |row| {
                Ok(UsageFilterOption {
                    value: row.get(0)?,
                    label: row.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let distinct_options = |column: &str| -> Result<Vec<UsageFilterOption>, DatabaseError> {
            let mut statement = connection.prepare(&format!(
                "SELECT DISTINCT {column} FROM usage_requests ORDER BY {column} ASC"
            ))?;
            Ok(statement
                .query_map([], |row| {
                    let value = row.get::<_, String>(0)?;
                    Ok(UsageFilterOption {
                        label: value.clone(),
                        value,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?)
        };
        Ok(UsageFilterOptions {
            earliest_usage_at_ms,
            latest_usage_at_ms,
            clients,
            models: distinct_options("resolved_model")?,
            backends: distinct_options("backend_id")?,
        })
    }

    pub fn usage_hourly(&self, date: &str) -> Result<Vec<UsageHourlySummary>, DatabaseError> {
        if !is_valid_date_key(date) {
            return Err(DatabaseError::InvalidData(
                "usage date must be a valid YYYY-MM-DD calendar date".to_owned(),
            ));
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "WITH bounds AS (
                SELECT
                    CAST(strftime('%s', ?1 || ' 00:00:00', 'utc') AS INTEGER) * 1000 AS start_ms,
                    CAST(strftime('%s', date(?1, '+1 day') || ' 00:00:00', 'utc') AS INTEGER) * 1000 AS end_ms
             )
             SELECT CAST(strftime('%H', u.started_at_ms / 1000, 'unixepoch', 'localtime') AS INTEGER),
                    COUNT(*),
                    COALESCE(SUM(u.input_tokens), 0),
                    COALESCE(SUM(u.cached_tokens), 0),
                    COALESCE(SUM(u.output_tokens), 0),
                    COALESCE(SUM(u.total_tokens), 0)
             FROM usage_requests u
             CROSS JOIN bounds
             WHERE u.started_at_ms >= bounds.start_ms
               AND u.started_at_ms < bounds.end_ms
             GROUP BY 1
             ORDER BY 1 ASC",
        )?;
        statement
            .query_map([date], |row| {
                let raw_hour = row.get::<_, i64>(0)?;
                let hour = u8::try_from(raw_hour)
                    .ok()
                    .filter(|hour| *hour < 24)
                    .ok_or_else(|| invalid_column("usage hour"))?;
                Ok(UsageHourlySummary {
                    hour,
                    request_count: nonnegative_i64(row.get(1)?, 1)?,
                    input_tokens: nonnegative_i64(row.get(2)?, 2)?,
                    cached_tokens: nonnegative_i64(row.get(3)?, 3)?,
                    output_tokens: nonnegative_i64(row.get(4)?, 4)?,
                    total_tokens: nonnegative_i64(row.get(5)?, 5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn usage_request(&self, request_id: &str) -> Result<UsageRequestRecord, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection
            .query_row(
                "SELECT request_id, client_app_id, protocol, requested_model, resolved_model,
                        backend_id, started_at_ms, first_token_at_ms, completed_at_ms,
                        input_tokens, cached_tokens, output_tokens, total_tokens,
                        status, error_category, usage_accuracy
                 FROM usage_requests WHERE request_id = ?1",
                [request_id],
                |row| {
                    Ok(UsageRequestRecord {
                        request_id: row.get(0)?,
                        client_app_id: row.get(1)?,
                        protocol: row.get(2)?,
                        requested_model: row.get(3)?,
                        resolved_model: row.get(4)?,
                        backend_id: row.get(5)?,
                        started_at_ms: row.get(6)?,
                        first_token_at_ms: row.get(7)?,
                        completed_at_ms: row.get(8)?,
                        input_tokens: row.get(9)?,
                        cached_tokens: row.get(10)?,
                        output_tokens: row.get(11)?,
                        total_tokens: row.get(12)?,
                        status: row.get(13)?,
                        error_category: row.get(14)?,
                        usage_accuracy: row.get(15)?,
                    })
                },
            )
            .map_err(Into::into)
    }

    pub fn create_download(&self, download: &DownloadRecord) -> Result<(), DatabaseError> {
        let expected_size = sqlite_u64(download.expected_size_bytes, "download expected size")?;
        let downloaded = sqlite_u64(download.downloaded_bytes, "download progress")?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection.execute(
            "INSERT INTO downloads (
                id, source, repository, revision, file_name, state,
                expected_size_bytes, downloaded_bytes, expected_sha256,
                temporary_path, destination_path, error_code, created_at_ms, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                download.id,
                download_source_key(download.source),
                download.repository,
                download.revision,
                download.file_name,
                download_state_key(download.state),
                expected_size,
                downloaded,
                download.expected_sha256.as_slice(),
                download.temporary_path,
                download.destination_path,
                download.error_code,
                download.created_at_ms,
                download.updated_at_ms,
            ],
        )?;
        Ok(())
    }

    pub fn update_download(
        &self,
        id: &str,
        state: ModelDownloadState,
        downloaded_bytes: u64,
        error_code: Option<&str>,
        now_ms: i64,
    ) -> Result<(), DatabaseError> {
        let downloaded = sqlite_u64(downloaded_bytes, "download progress")?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let changed = connection.execute(
            "UPDATE downloads
             SET state = ?1, downloaded_bytes = ?2, error_code = ?3, updated_at_ms = ?4
             WHERE id = ?5",
            params![
                download_state_key(state),
                downloaded,
                error_code,
                now_ms,
                id
            ],
        )?;
        if changed == 0 {
            return Err(DatabaseError::InvalidData(
                "download record does not exist".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn download(&self, id: &str) -> Result<Option<DownloadRecord>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let result = connection.query_row(
            "SELECT id, source, repository, revision, file_name, state,
                    expected_size_bytes, downloaded_bytes, expected_sha256,
                    temporary_path, destination_path, error_code, created_at_ms, updated_at_ms
             FROM downloads WHERE id = ?1",
            [id],
            download_record_from_row,
        );
        match result {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn downloads(&self) -> Result<Vec<DownloadRecord>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT id, source, repository, revision, file_name, state,
                    expected_size_bytes, downloaded_bytes, expected_sha256,
                    temporary_path, destination_path, error_code, created_at_ms, updated_at_ms
             FROM downloads ORDER BY updated_at_ms DESC, id",
        )?;
        let records = statement.query_map([], download_record_from_row)?;
        records.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn pause_interrupted_downloads(&self, now_ms: i64) -> Result<(), DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection.execute(
            "UPDATE downloads SET state = 'paused', error_code = 'app_restarted', updated_at_ms = ?1
             WHERE state IN ('pending', 'resolving', 'downloading', 'verifying', 'installing')",
            [now_ms],
        )?;
        Ok(())
    }

    pub fn model_library(&self, model_storage_path: &Path) -> Result<ModelLibrary, DatabaseError> {
        Ok(ModelLibrary {
            default_download_source: self.default_download_source()?,
            model_storage_path: model_storage_path.display().to_string(),
            models: self.local_models()?,
        })
    }

    pub fn default_download_source(&self) -> Result<Option<DownloadSource>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let result = connection.query_row(
            "SELECT value_json FROM settings WHERE key = 'models.default_download_source'",
            [],
            |row| row.get::<_, String>(0),
        );
        match result {
            Ok(value) => match value.as_str() {
                "\"huggingFace\"" => Ok(Some(DownloadSource::HuggingFace)),
                "\"modelScope\"" => Ok(Some(DownloadSource::ModelScope)),
                _ => Err(DatabaseError::InvalidData(
                    "models.default_download_source is not recognized".to_owned(),
                )),
            },
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn set_default_download_source(
        &self,
        source: DownloadSource,
        now_ms: i64,
    ) -> Result<(), DatabaseError> {
        let value = match source {
            DownloadSource::HuggingFace => "\"huggingFace\"",
            DownloadSource::ModelScope => "\"modelScope\"",
        };
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection.execute(
            "INSERT INTO settings (key, value_json, updated_at)
             VALUES ('models.default_download_source', ?1, ?2)
             ON CONFLICT(key) DO UPDATE SET
                value_json = excluded.value_json,
                updated_at = excluded.updated_at",
            params![value, now_ms.to_string()],
        )?;
        Ok(())
    }

    pub fn upsert_local_model(
        &self,
        model: &LocalModelSummary,
        now_ms: i64,
    ) -> Result<(), DatabaseError> {
        self.upsert_local_model_snapshot(model, None, None, false, now_ms)
    }

    pub fn upsert_external_model(
        &self,
        model: &LocalModelSummary,
        modified_at_ms: i64,
        sha256: &[u8; 32],
        now_ms: i64,
    ) -> Result<(), DatabaseError> {
        if model.ownership != ModelOwnership::External {
            return Err(DatabaseError::InvalidData(
                "external import must retain external ownership".to_owned(),
            ));
        }
        self.upsert_local_model_snapshot(model, Some(modified_at_ms), Some(sha256), true, now_ms)
    }

    fn upsert_local_model_snapshot(
        &self,
        model: &LocalModelSummary,
        modified_at_ms: Option<i64>,
        sha256: Option<&[u8; 32]>,
        audit_import: bool,
        now_ms: i64,
    ) -> Result<(), DatabaseError> {
        let size_bytes = i64::try_from(model.size_bytes).map_err(|_| {
            DatabaseError::InvalidData("model file size exceeds SQLite INTEGER range".to_owned())
        })?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        write_local_model_snapshot(
            &transaction,
            model,
            size_bytes,
            modified_at_ms,
            sha256,
            now_ms,
        )?;
        if audit_import {
            let summary = json!({
                "displayName": model.display_name,
                "fileName": model.file_name,
                "format": model.format,
                "ownership": "external",
                "sizeBytes": model.size_bytes,
            })
            .to_string();
            transaction.execute(
                "INSERT INTO audit_events (
                    id, event_type, target_type, target_id, summary_json, created_at_ms
                 ) VALUES (?1, 'model_imported', 'model', ?2, ?3, ?4)",
                params![Uuid::new_v4().to_string(), model.id, summary, now_ms],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn complete_model_download(
        &self,
        download_id: &str,
        model: &LocalModelSummary,
        sha256: &[u8; 32],
        now_ms: i64,
    ) -> Result<(), DatabaseError> {
        if model.ownership != ModelOwnership::Managed {
            return Err(DatabaseError::InvalidData(
                "downloaded model must retain managed ownership".to_owned(),
            ));
        }
        let size_bytes = sqlite_u64(model.size_bytes, "model file size")?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        write_local_model_snapshot(&transaction, model, size_bytes, None, Some(sha256), now_ms)?;
        let changed = transaction.execute(
            "UPDATE downloads
             SET state = 'ready', downloaded_bytes = expected_size_bytes,
                 error_code = NULL, updated_at_ms = ?1
             WHERE id = ?2",
            params![now_ms, download_id],
        )?;
        if changed == 0 {
            return Err(DatabaseError::InvalidData(
                "download record does not exist".to_owned(),
            ));
        }
        let summary = json!({
            "displayName": model.display_name,
            "fileName": model.file_name,
            "format": model.format,
            "ownership": "managed",
            "repository": model.repository,
            "revision": model.revision,
            "sizeBytes": model.size_bytes,
            "source": model_source_key(model.source),
        })
        .to_string();
        transaction.execute(
            "INSERT INTO audit_events (
                id, event_type, target_type, target_id, summary_json, created_at_ms
             ) VALUES (?1, 'model_downloaded', 'model', ?2, ?3, ?4)",
            params![Uuid::new_v4().to_string(), model.id, summary, now_ms],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn audit_event_count(&self) -> Result<u64, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let count: i64 =
            connection.query_row("SELECT COUNT(*) FROM audit_events", [], |row| row.get(0))?;
        Ok(u64::try_from(count).unwrap_or(0))
    }

    pub fn model_integrity(
        &self,
        model_id: &str,
    ) -> Result<Option<ModelIntegrityRecord>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let result = connection.query_row(
            "SELECT l.path, l.sha256
             FROM model_locations l
             WHERE l.model_id = ?1",
            [model_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<Vec<u8>>>(1)?)),
        );
        let (path, sha256) = match result {
            Ok(value) => value,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let sha256 = sha256
            .map(|value| {
                value.try_into().map_err(|_| {
                    DatabaseError::InvalidData(
                        "model SHA-256 must contain exactly 32 bytes".to_owned(),
                    )
                })
            })
            .transpose()?;
        Ok(Some(ModelIntegrityRecord { path, sha256 }))
    }

    pub fn mark_model_verification_failed(&self, model_id: &str) -> Result<(), DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection.execute(
            "UPDATE models SET state = 'verification_failed' WHERE id = ?1",
            [model_id],
        )?;
        Ok(())
    }

    pub fn insert_audit_event(
        &self,
        event_type: &str,
        target_type: &str,
        target_id: &str,
        summary_json: &str,
        now_ms: i64,
    ) -> Result<(), DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection.execute(
            "INSERT INTO audit_events (
                id, event_type, target_type, target_id, summary_json, created_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                Uuid::new_v4().to_string(),
                event_type,
                target_type,
                target_id,
                summary_json,
                now_ms
            ],
        )?;
        Ok(())
    }

    pub fn model_path_is_indexed(&self, path: &Path) -> Result<bool, DatabaseError> {
        let path = path.to_str().ok_or_else(|| {
            DatabaseError::InvalidData("model path is not valid UTF-8".to_owned())
        })?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM model_locations WHERE path = ?1",
            [path],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn refresh_local_model_states(&self) -> Result<(), DatabaseError> {
        let snapshots = {
            let connection = self
                .connection
                .lock()
                .map_err(|_| DatabaseError::LockPoisoned)?;
            let mut statement = connection.prepare(
                "SELECT m.id, m.state, l.path, l.size_bytes, l.modified_at_ms, l.sha256
                 FROM models m JOIN model_locations l ON l.model_id = m.id",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<Vec<u8>>>(5)?,
                ))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let changes = snapshots
            .into_iter()
            .filter_map(
                |(id, current_state, path, expected_size, expected_modified, expected_sha256)| {
                    if current_state == "verification_failed" {
                        return None;
                    }
                    let next_state = if expected_sha256
                        .as_ref()
                        .is_some_and(|hash| hash.len() == 32)
                    {
                        file_snapshot_state(Path::new(&path), expected_size, expected_modified)
                    } else {
                        "verification_failed".to_owned()
                    };
                    (next_state != current_state).then_some((id, next_state))
                },
            )
            .collect::<Vec<_>>();
        if changes.is_empty() {
            return Ok(());
        }

        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        for (id, state) in changes {
            transaction.execute(
                "UPDATE models SET state = ?1 WHERE id = ?2",
                params![state, id],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn local_models(&self) -> Result<Vec<LocalModelSummary>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT m.id, m.display_name, m.format, m.quantization, m.source,
                    m.repository, m.revision, m.file_name, m.ownership, m.license,
                    m.state, l.path, l.size_bytes
             FROM models m
             JOIN model_locations l ON l.model_id = m.id
             ORDER BY m.display_name COLLATE NOCASE, m.id",
        )?;
        let rows = statement.query_map([], local_model_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn local_model(&self, id: &str) -> Result<Option<LocalModelSummary>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let result = connection.query_row(
            "SELECT m.id, m.display_name, m.format, m.quantization, m.source,
                    m.repository, m.revision, m.file_name, m.ownership, m.license,
                    m.state, l.path, l.size_bytes
             FROM models m
             JOIN model_locations l ON l.model_id = m.id
             WHERE m.id = ?1",
            [id],
            local_model_from_row,
        );
        match result {
            Ok(model) => Ok(Some(model)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn remove_local_model(
        &self,
        model_id: &str,
        expected_ownership: ModelOwnership,
        expected_path: &Path,
        removal_kind: ModelRemovalKind,
        now_ms: i64,
    ) -> Result<bool, DatabaseError> {
        let expected_path = expected_path.to_str().ok_or_else(|| {
            DatabaseError::InvalidData("model path is not valid UTF-8".to_owned())
        })?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        let snapshot = transaction.query_row(
            "SELECT m.display_name, m.ownership, l.path
             FROM models m JOIN model_locations l ON l.model_id = m.id
             WHERE m.id = ?1",
            [model_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        );
        let (display_name, ownership, path) = match snapshot {
            Ok(snapshot) => snapshot,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        if parse_model_ownership(&ownership)? != expected_ownership || path != expected_path {
            return Err(DatabaseError::InvalidData(
                "model ownership or path changed after removal preview".to_owned(),
            ));
        }
        let changed = transaction.execute("DELETE FROM models WHERE id = ?1", [model_id])?;
        if changed != 1 {
            return Ok(false);
        }
        let summary = json!({
            "displayName": display_name,
            "ownership": model_ownership_key(expected_ownership),
            "removalKind": match removal_kind {
                ModelRemovalKind::MoveManagedFileToTrash => "move_managed_file_to_trash",
                ModelRemovalKind::RemoveMissingManagedIndex => "remove_missing_managed_index",
                ModelRemovalKind::RemoveExternalIndex => "remove_external_index",
            },
            "sourceFilePreserved": expected_ownership == ModelOwnership::External,
        })
        .to_string();
        transaction.execute(
            "INSERT INTO audit_events (
                id, event_type, target_type, target_id, summary_json, created_at_ms
             ) VALUES (?1, 'model_removed', 'model', ?2, ?3, ?4)",
            params![Uuid::new_v4().to_string(), model_id, summary, now_ms],
        )?;
        transaction.commit()?;
        Ok(true)
    }

    fn configure_and_migrate(mut connection: Connection) -> Result<Self, DatabaseError> {
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;

        migrations().to_latest(&mut connection)?;
        upgrade_runtime_profiles_to_v3(&mut connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
}

fn upgrade_runtime_profiles_to_v3(connection: &mut Connection) -> Result<(), DatabaseError> {
    let transaction = connection.transaction()?;
    let profiles = {
        let mut statement = transaction.prepare(
            "SELECT id, ownership, backend_id, backend_api_root, model_digest,
                    model_digest_kind, engine
             FROM runtime_profiles WHERE spec_version IN (1, 2)
             ORDER BY id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for (id, ownership, backend_id, backend_api_root, digest, digest_kind, engine) in profiles {
        let managed = ownership == "managed";
        let adapter_variant = match (managed, engine.as_str()) {
            (true, "llama.cpp") => "hal100-managed-metal",
            (false, "ollama") => "official-loopback-api",
            (false, _) => "official-openai-server",
            (true, _) => "hal100-managed",
        };
        let (backend_config_revision, origin_fingerprint) = if managed {
            (None, None)
        } else {
            let backend_id = backend_id.as_deref().ok_or_else(|| {
                DatabaseError::InvalidData(
                    "external runtime profile is missing its backend identity".to_owned(),
                )
            })?;
            let api_root = backend_api_root.as_deref().ok_or_else(|| {
                DatabaseError::InvalidData(
                    "external runtime profile is missing its origin".to_owned(),
                )
            })?;
            let revision = match transaction.query_row(
                "SELECT config_revision FROM backends WHERE id = ?1",
                [backend_id],
                |row| row.get::<_, i64>(0),
            ) {
                Ok(revision) => revision,
                Err(rusqlite::Error::QueryReturnedNoRows) => 1,
                Err(error) => return Err(error.into()),
            };
            let fingerprint = hex_sha256(api_root.as_bytes());
            (Some(revision), Some(fingerprint))
        };
        let evidence_algorithm = match digest_kind.as_str() {
            "sha256" => "sha256",
            "ollama_digest" => "ollama-digest",
            _ => {
                return Err(DatabaseError::InvalidData(
                    "runtime profile has an unknown legacy digest kind".to_owned(),
                ));
            }
        };
        let protocol_capability_hash = if engine == "ollama" {
            "8dc12d2fe05570e519c66dc34734f42d086049b6f0987cba9f1b2f20ac7381eb"
        } else {
            "1b3e385cbb7f30878cba8eaccf7d5f5e6e1f18b2861a44bc79b18d963cbdd258"
        };
        transaction.execute(
            "UPDATE runtime_profiles SET
                spec_version = 3,
                adapter_variant = ?1,
                adapter_contract_revision = 'engine-contract-v1',
                backend_config_revision = ?2,
                origin_fingerprint = ?3,
                evidence_kind = 'content_digest',
                evidence_algorithm = ?4,
                evidence_value = ?5,
                protocol_capability_hash = ?6
             WHERE id = ?7 AND spec_version IN (1, 2)",
            params![
                adapter_variant,
                backend_config_revision,
                origin_fingerprint,
                evidence_algorithm,
                digest,
                protocol_capability_hash,
                id,
            ],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

fn hex_sha256(value: &[u8]) -> String {
    Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn validate_stored_active_route(
    route: Option<&StoredActiveGatewayRoute>,
) -> Result<(), DatabaseError> {
    let Some(route) = route else {
        return Ok(());
    };
    if route.backend_id.is_empty()
        || route.backend_id.len() > 128
        || !route
            .backend_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(DatabaseError::InvalidData(
            "gateway active route has an invalid backend identifier".to_owned(),
        ));
    }
    if route.resolved_model.as_ref().is_some_and(|model| {
        model.trim().is_empty() || model.len() > 256 || model.chars().any(char::is_control)
    }) {
        return Err(DatabaseError::InvalidData(
            "gateway active route has an invalid resolved model".to_owned(),
        ));
    }
    Ok(())
}

fn sqlite_u64(value: u64, field: &str) -> Result<i64, DatabaseError> {
    i64::try_from(value)
        .map_err(|_| DatabaseError::InvalidData(format!("{field} exceeds SQLite INTEGER range")))
}

fn decode_setting<T: serde::de::DeserializeOwned>(
    value: &str,
    key: &str,
) -> Result<T, DatabaseError> {
    serde_json::from_str(value)
        .map_err(|_| DatabaseError::InvalidData(format!("{key} contains invalid JSON")))
}

fn upsert_setting(
    connection: &Connection,
    key: &str,
    value_json: &str,
    now_ms: i64,
) -> Result<(), rusqlite::Error> {
    connection.execute(
        "INSERT INTO settings (key, value_json, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET
            value_json = excluded.value_json,
            updated_at = excluded.updated_at",
        params![key, value_json, now_ms.to_string()],
    )?;
    Ok(())
}

fn validate_retention_days(days: Option<u16>) -> Result<(), DatabaseError> {
    if days.is_none_or(|days| matches!(days, 30 | 90 | 180 | 365)) {
        Ok(())
    } else {
        Err(DatabaseError::InvalidData(
            "retention days must be 30, 90, 180, 365 or forever".to_owned(),
        ))
    }
}

fn retention_cutoff(days: Option<u16>, now_ms: i64) -> Option<i64> {
    days.map(|days| now_ms.saturating_sub(i64::from(days).saturating_mul(24 * 60 * 60 * 1_000)))
}

fn count_expired(
    connection: &Connection,
    table: &str,
    timestamp_column: &str,
    days: Option<u16>,
    now_ms: i64,
) -> Result<u64, DatabaseError> {
    let Some(cutoff) = retention_cutoff(days, now_ms) else {
        return Ok(0);
    };
    let query = format!("SELECT COUNT(*) FROM {table} WHERE {timestamp_column} < ?1");
    let count: i64 = connection.query_row(&query, [cutoff], |row| row.get(0))?;
    Ok(u64::try_from(count).unwrap_or(0))
}

fn delete_expired(
    connection: &Connection,
    table: &str,
    timestamp_column: &str,
    days: Option<u16>,
    now_ms: i64,
) -> Result<u64, DatabaseError> {
    let Some(cutoff) = retention_cutoff(days, now_ms) else {
        return Ok(0);
    };
    let query = format!("DELETE FROM {table} WHERE {timestamp_column} < ?1");
    let deleted = connection.execute(&query, [cutoff])?;
    Ok(u64::try_from(deleted).unwrap_or(u64::MAX))
}

fn safe_audit_details(summary_json: &str) -> Vec<AuditDetail> {
    const SAFE_KEYS: &[&str] = &[
        "action",
        "alias",
        "auditDeleted",
        "auditRetentionDays",
        "backendId",
        "displayName",
        "enabled",
        "engine",
        "errorCode",
        "fileName",
        "format",
        "model",
        "modelId",
        "ownership",
        "provider",
        "reason",
        "repository",
        "resolvedModel",
        "revision",
        "sizeBytes",
        "source",
        "toolCalls",
        "toolPolicy",
        "usageDeleted",
        "usageRetentionDays",
        "version",
    ];
    let Ok(serde_json::Value::Object(values)) = serde_json::from_str(summary_json) else {
        return Vec::new();
    };
    SAFE_KEYS
        .iter()
        .filter_map(|key| {
            let value = values.get(*key)?;
            let rendered = match value {
                serde_json::Value::String(value) => value.clone(),
                serde_json::Value::Number(value) => value.to_string(),
                serde_json::Value::Bool(value) => value.to_string(),
                serde_json::Value::Null => "永久保留".to_owned(),
                serde_json::Value::Array(_) | serde_json::Value::Object(_) => return None,
            };
            Some(AuditDetail {
                key: (*key).to_owned(),
                value: rendered.chars().take(512).collect(),
            })
        })
        .collect()
}

fn nonnegative_i64(value: i64, column: usize) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, value))
}

fn optional_nonnegative_i64(value: Option<i64>, column: usize) -> rusqlite::Result<Option<u64>> {
    value
        .map(|value| nonnegative_i64(value, column))
        .transpose()
}

fn validate_usage_scope_query(query: &UsageScopeQuery) -> Result<(), DatabaseError> {
    const MAX_RANGE_MS: i64 = 5 * 366 * 24 * 60 * 60 * 1_000;
    if query.start_at_ms < 0
        || query.end_at_ms_exclusive <= query.start_at_ms
        || query.end_at_ms_exclusive - query.start_at_ms > MAX_RANGE_MS
    {
        return Err(DatabaseError::InvalidData(
            "usage scope must be a positive range no longer than five years".to_owned(),
        ));
    }
    if query.series_start_at_ms < 0
        || query.series_end_at_ms_exclusive <= query.series_start_at_ms
        || query.series_end_at_ms_exclusive - query.series_start_at_ms > MAX_RANGE_MS
    {
        return Err(DatabaseError::InvalidData(
            "usage series scope must be a positive range no longer than five years".to_owned(),
        ));
    }
    for (name, value) in [
        ("client", query.client_app_id.as_deref()),
        ("model", query.resolved_model.as_deref()),
        ("backend", query.backend_id.as_deref()),
    ] {
        if value.is_some_and(|value| {
            value.is_empty() || value.len() > 128 || value.chars().any(char::is_control)
        }) {
            return Err(DatabaseError::InvalidData(format!(
                "usage {name} filter is invalid"
            )));
        }
    }
    if query
        .status
        .as_deref()
        .is_some_and(|status| !matches!(status, "succeeded" | "failed" | "cancelled"))
    {
        return Err(DatabaseError::InvalidData(
            "usage status filter is invalid".to_owned(),
        ));
    }
    Ok(())
}

fn is_valid_date_key(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| index != 4 && index != 7 && !byte.is_ascii_digit())
    {
        return false;
    }
    let Some(year) = value[0..4].parse::<u16>().ok() else {
        return false;
    };
    let Some(month) = value[5..7].parse::<u8>().ok() else {
        return false;
    };
    let Some(day) = value[8..10].parse::<u8>().ok() else {
        return false;
    };
    if year < 1970 || !(1..=12).contains(&month) {
        return false;
    }
    let leap_year = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        2 if leap_year => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (1..=days_in_month).contains(&day)
}

fn write_local_model_snapshot(
    transaction: &Transaction<'_>,
    model: &LocalModelSummary,
    size_bytes: i64,
    modified_at_ms: Option<i64>,
    sha256: Option<&[u8; 32]>,
    now_ms: i64,
) -> Result<(), rusqlite::Error> {
    transaction.execute(
        "INSERT INTO models (
            id, display_name, format, quantization, source, repository, revision,
            file_name, ownership, license, capabilities_json, state, created_at_ms,
            updated_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, '[]', ?11, ?12, ?12)
         ON CONFLICT(id) DO UPDATE SET
            display_name = excluded.display_name,
            format = excluded.format,
            quantization = excluded.quantization,
            source = excluded.source,
            repository = excluded.repository,
            revision = excluded.revision,
            file_name = excluded.file_name,
            ownership = excluded.ownership,
            license = excluded.license,
            state = excluded.state,
            updated_at_ms = excluded.updated_at_ms",
        params![
            model.id,
            model.display_name,
            model.format,
            model.quantization,
            model_source_key(model.source),
            model.repository,
            model.revision,
            model.file_name,
            model_ownership_key(model.ownership),
            model.license,
            model_state_key(model.state),
            now_ms,
        ],
    )?;
    transaction.execute(
        "INSERT INTO model_locations (
            model_id, path, size_bytes, modified_at_ms, sha256, updated_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(model_id) DO UPDATE SET
            path = excluded.path,
            size_bytes = excluded.size_bytes,
            modified_at_ms = excluded.modified_at_ms,
            sha256 = excluded.sha256,
            updated_at_ms = excluded.updated_at_ms",
        params![
            model.id,
            model.path,
            size_bytes,
            modified_at_ms,
            sha256.map(|value| value.as_slice()),
            now_ms
        ],
    )?;
    Ok(())
}

fn download_record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DownloadRecord> {
    let expected_size_i64 = row
        .get::<_, Option<i64>>(6)?
        .ok_or_else(|| invalid_column("download expected size"))?;
    let expected_size_bytes = u64::try_from(expected_size_i64)
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(6, expected_size_i64))?;
    let downloaded_i64 = row.get::<_, i64>(7)?;
    let downloaded_bytes = u64::try_from(downloaded_i64)
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(7, downloaded_i64))?;
    let hash = row
        .get::<_, Option<Vec<u8>>>(8)?
        .ok_or_else(|| invalid_column("download SHA-256"))?;
    let expected_sha256: [u8; 32] = hash
        .try_into()
        .map_err(|_| invalid_column("download SHA-256"))?;
    Ok(DownloadRecord {
        id: row.get(0)?,
        source: parse_download_source(&row.get::<_, String>(1)?)?,
        repository: row.get(2)?,
        revision: row
            .get::<_, Option<String>>(3)?
            .ok_or_else(|| invalid_column("download revision"))?,
        file_name: row.get(4)?,
        state: parse_download_state(&row.get::<_, String>(5)?)?,
        expected_size_bytes,
        downloaded_bytes,
        expected_sha256,
        temporary_path: row.get(9)?,
        destination_path: row.get(10)?,
        error_code: row.get(11)?,
        created_at_ms: row.get(12)?,
        updated_at_ms: row.get(13)?,
    })
}

fn local_model_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LocalModelSummary> {
    let source = parse_model_source(&row.get::<_, String>(4)?)?;
    let ownership = parse_model_ownership(&row.get::<_, String>(8)?)?;
    let state = parse_model_state(&row.get::<_, String>(10)?)?;
    let size_i64 = row.get::<_, i64>(12)?;
    let size_bytes = u64::try_from(size_i64)
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(12, size_i64))?;
    Ok(LocalModelSummary {
        id: row.get(0)?,
        display_name: row.get(1)?,
        format: row.get(2)?,
        quantization: row.get(3)?,
        source,
        repository: row.get(5)?,
        revision: row.get(6)?,
        file_name: row.get(7)?,
        ownership,
        license: row.get(9)?,
        state,
        path: row.get(11)?,
        size_bytes,
    })
}

fn runtime_profile_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<StoredRuntimeProfileRecord> {
    let spec_version_value = row.get::<_, i64>(3)?;
    let spec_version = u16::try_from(spec_version_value)
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(3, spec_version_value))?;
    let context_value = row.get::<_, Option<i64>>(14)?;
    let context_window_tokens = context_value
        .map(|value| {
            u32::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(14, value))
        })
        .transpose()?;
    let backend_revision_value = row.get::<_, Option<i64>>(18)?;
    let backend_config_revision = backend_revision_value
        .map(|value| {
            u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(18, value))
        })
        .transpose()?;
    let support_cell = runtime_profile_support_cell_from_row(row)?;
    Ok(StoredRuntimeProfileRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        spec_version,
        ownership: row.get(4)?,
        backend_id: row.get(5)?,
        backend_api_root: row.get(6)?,
        model_id: row.get(7)?,
        model_display_name: row.get(8)?,
        model_digest: row.get(9)?,
        model_digest_kind: row.get(10)?,
        engine: row.get(11)?,
        engine_version: row.get(12)?,
        capacity_tier: row.get(13)?,
        context_window_tokens,
        capacity_revision: row.get(15)?,
        adapter_variant: row.get(16)?,
        adapter_contract_revision: row.get(17)?,
        backend_config_revision,
        origin_fingerprint: row.get(19)?,
        evidence_kind: row.get(20)?,
        evidence_algorithm: row.get(21)?,
        evidence_value: row.get(22)?,
        protocol_capability_hash: row.get(23)?,
        support_cell,
        verified_at_ms: row.get(28)?,
        last_activated_at_ms: row.get(29)?,
        created_at_ms: row.get(30)?,
        updated_at_ms: row.get(31)?,
    })
}

fn runtime_profile_support_cell_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<Option<RuntimeProfileSupportCell>> {
    let platform = row.get::<_, Option<String>>(24)?;
    let architecture = row.get::<_, Option<String>>(25)?;
    let accelerator = row.get::<_, Option<String>>(26)?;
    let deployment = row.get::<_, Option<String>>(27)?;
    let values = [&platform, &architecture, &accelerator, &deployment];
    if values.iter().all(|value| value.is_none()) {
        return Ok(None);
    }
    if values.iter().any(|value| value.is_none()) {
        return Err(invalid_support_cell_from_sqlite());
    }
    let platform = platform
        .as_deref()
        .and_then(hal100_protocol::InferencePlatform::from_storage_key)
        .ok_or_else(invalid_support_cell_from_sqlite)?;
    let architecture = architecture
        .as_deref()
        .and_then(hal100_protocol::InferenceArchitecture::from_storage_key)
        .ok_or_else(invalid_support_cell_from_sqlite)?;
    let accelerator = accelerator
        .as_deref()
        .and_then(hal100_protocol::InferenceAccelerator::from_storage_key)
        .ok_or_else(invalid_support_cell_from_sqlite)?;
    let deployment = deployment
        .as_deref()
        .and_then(hal100_protocol::InferenceDeployment::from_storage_key)
        .ok_or_else(invalid_support_cell_from_sqlite)?;
    Ok(Some(RuntimeProfileSupportCell {
        platform,
        architecture,
        accelerator,
        deployment,
    }))
}

fn invalid_support_cell_from_sqlite() -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        24,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "runtime profile support cell is invalid or partial",
        )),
    )
}

fn runtime_activation_journal_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<StoredRuntimeActivationJournal> {
    let previous_route = row
        .get::<_, Option<String>>(3)?
        .map(|value| {
            serde_json::from_str::<StoredActiveGatewayRoute>(&value)
                .map_err(|_| invalid_column("runtime activation previous route"))
        })
        .transpose()?;
    Ok(StoredRuntimeActivationJournal {
        id: row.get(0)?,
        profile_id: row.get(1)?,
        phase: parse_runtime_activation_phase(&row.get::<_, String>(2)?)?,
        previous_route,
        previous_managed_model_id: row.get(4)?,
        created_at_ms: row.get(5)?,
        updated_at_ms: row.get(6)?,
    })
}

fn runtime_activation_phase_key(phase: RuntimeActivationPhase) -> &'static str {
    match phase {
        RuntimeActivationPhase::Journaled => "journaled",
        RuntimeActivationPhase::Quiesced => "quiesced",
        RuntimeActivationPhase::RouteSwitched => "route_switched",
        RuntimeActivationPhase::Compensating => "compensating",
        RuntimeActivationPhase::RecoveryRequired => "recovery_required",
    }
}

fn parse_runtime_activation_phase(value: &str) -> rusqlite::Result<RuntimeActivationPhase> {
    match value {
        "journaled" => Ok(RuntimeActivationPhase::Journaled),
        "quiesced" => Ok(RuntimeActivationPhase::Quiesced),
        "route_switched" => Ok(RuntimeActivationPhase::RouteSwitched),
        "compensating" => Ok(RuntimeActivationPhase::Compensating),
        "recovery_required" => Ok(RuntimeActivationPhase::RecoveryRequired),
        _ => Err(invalid_column("runtime activation phase")),
    }
}

fn valid_runtime_activation_transition(
    current: RuntimeActivationPhase,
    next: RuntimeActivationPhase,
) -> bool {
    matches!(
        (current, next),
        (
            RuntimeActivationPhase::Journaled,
            RuntimeActivationPhase::Quiesced
                | RuntimeActivationPhase::RouteSwitched
                | RuntimeActivationPhase::Compensating
                | RuntimeActivationPhase::RecoveryRequired
        ) | (
            RuntimeActivationPhase::Quiesced,
            RuntimeActivationPhase::RouteSwitched
                | RuntimeActivationPhase::Compensating
                | RuntimeActivationPhase::RecoveryRequired
        ) | (
            RuntimeActivationPhase::RouteSwitched,
            RuntimeActivationPhase::Compensating | RuntimeActivationPhase::RecoveryRequired
        ) | (
            RuntimeActivationPhase::Compensating,
            RuntimeActivationPhase::RecoveryRequired
        ) | (
            RuntimeActivationPhase::RecoveryRequired,
            RuntimeActivationPhase::Compensating
        )
    )
}

fn stored_backend_binding(kind: &str) -> (Option<&'static str>, Option<&'static str>) {
    match kind {
        "managed_llama_cpp" => (Some("llama.cpp"), Some("hal100-managed-metal")),
        "external_llama_cpp" => (Some("llama.cpp"), Some("official-openai-server")),
        "external_ollama" => (Some("ollama"), Some("official-loopback-api")),
        "external_vllm" => (Some("vllm"), Some("official-openai-server")),
        "external_openai" | "external_anthropic" => (None, None),
        _ => (None, None),
    }
}

fn migrations<'a>() -> Migrations<'a> {
    Migrations::new(vec![
        M::up(
            "CREATE TABLE settings (
            key TEXT PRIMARY KEY NOT NULL,
            value_json TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );",
        ),
        M::up(
            "CREATE TABLE client_apps (
                id TEXT PRIMARY KEY NOT NULL,
                display_name TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );

            CREATE TABLE api_key_hashes (
                id TEXT PRIMARY KEY NOT NULL,
                client_app_id TEXT NOT NULL REFERENCES client_apps(id) ON DELETE CASCADE,
                display_prefix TEXT NOT NULL,
                key_hash BLOB NOT NULL UNIQUE CHECK(length(key_hash) = 32),
                created_at_ms INTEGER NOT NULL
            );

            CREATE TABLE usage_requests (
                request_id TEXT PRIMARY KEY NOT NULL,
                client_app_id TEXT NOT NULL,
                protocol TEXT NOT NULL,
                requested_model TEXT NOT NULL,
                resolved_model TEXT NOT NULL,
                backend_id TEXT NOT NULL,
                started_at_ms INTEGER NOT NULL,
                first_token_at_ms INTEGER,
                completed_at_ms INTEGER NOT NULL,
                input_tokens INTEGER,
                cached_tokens INTEGER,
                output_tokens INTEGER,
                total_tokens INTEGER,
                status TEXT NOT NULL,
                error_category TEXT,
                usage_accuracy TEXT NOT NULL,
                CHECK(status IN ('succeeded', 'failed', 'cancelled')),
                CHECK(usage_accuracy IN ('exact_backend_response', 'unavailable'))
            );

            CREATE INDEX usage_requests_client_started
                ON usage_requests(client_app_id, started_at_ms DESC);
            CREATE INDEX usage_requests_backend_started
                ON usage_requests(backend_id, started_at_ms DESC);",
        ),
        M::up(
            "CREATE TABLE integrations (
                id TEXT PRIMARY KEY NOT NULL,
                kind TEXT NOT NULL,
                config_path TEXT NOT NULL,
                credential_path TEXT NOT NULL,
                managed_fragment_hash BLOB NOT NULL CHECK(length(managed_fragment_hash) = 32),
                backup_path TEXT,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );",
        ),
        M::up(
            "CREATE TABLE models (
                id TEXT PRIMARY KEY NOT NULL,
                display_name TEXT NOT NULL,
                format TEXT NOT NULL CHECK(format = 'gguf'),
                quantization TEXT,
                source TEXT NOT NULL CHECK(source IN ('hugging_face', 'modelscope', 'local_file')),
                repository TEXT,
                revision TEXT,
                file_name TEXT NOT NULL,
                ownership TEXT NOT NULL CHECK(ownership IN ('managed', 'external')),
                license TEXT,
                capabilities_json TEXT NOT NULL,
                state TEXT NOT NULL CHECK(state IN ('ready', 'missing', 'changed', 'verification_failed')),
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );

            CREATE TABLE model_locations (
                model_id TEXT PRIMARY KEY NOT NULL REFERENCES models(id) ON DELETE CASCADE,
                path TEXT NOT NULL UNIQUE,
                size_bytes INTEGER NOT NULL CHECK(size_bytes >= 0),
                modified_at_ms INTEGER,
                sha256 BLOB CHECK(sha256 IS NULL OR length(sha256) = 32),
                updated_at_ms INTEGER NOT NULL
            );

            CREATE TABLE downloads (
                id TEXT PRIMARY KEY NOT NULL,
                source TEXT NOT NULL CHECK(source IN ('hugging_face', 'modelscope')),
                repository TEXT NOT NULL,
                revision TEXT,
                file_name TEXT NOT NULL,
                state TEXT NOT NULL CHECK(state IN (
                    'pending', 'resolving', 'downloading', 'paused', 'verifying',
                    'installing', 'ready', 'failed', 'cancelled'
                )),
                expected_size_bytes INTEGER,
                downloaded_bytes INTEGER NOT NULL DEFAULT 0 CHECK(downloaded_bytes >= 0),
                expected_sha256 BLOB CHECK(expected_sha256 IS NULL OR length(expected_sha256) = 32),
                temporary_path TEXT NOT NULL,
                destination_path TEXT NOT NULL,
                error_code TEXT,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );

            CREATE INDEX downloads_state_updated ON downloads(state, updated_at_ms DESC);",
        ),
        M::up(
            "CREATE TABLE audit_events (
                id TEXT PRIMARY KEY NOT NULL,
                event_type TEXT NOT NULL,
                target_type TEXT NOT NULL,
                target_id TEXT NOT NULL,
                summary_json TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL
            );

            CREATE INDEX audit_events_created ON audit_events(created_at_ms DESC);",
        ),
        M::up(
            "DROP INDEX usage_requests_client_started;
            DROP INDEX usage_requests_backend_started;
            ALTER TABLE usage_requests RENAME TO usage_requests_v5;

            CREATE TABLE usage_requests (
                request_id TEXT PRIMARY KEY NOT NULL,
                client_app_id TEXT NOT NULL,
                protocol TEXT NOT NULL,
                requested_model TEXT NOT NULL,
                resolved_model TEXT NOT NULL,
                backend_id TEXT NOT NULL,
                started_at_ms INTEGER NOT NULL,
                first_token_at_ms INTEGER,
                completed_at_ms INTEGER NOT NULL,
                input_tokens INTEGER,
                cached_tokens INTEGER,
                output_tokens INTEGER,
                total_tokens INTEGER,
                status TEXT NOT NULL,
                error_category TEXT,
                usage_accuracy TEXT NOT NULL,
                CHECK(status IN ('succeeded', 'failed', 'cancelled')),
                CHECK(usage_accuracy IN (
                    'exact_backend_response', 'exact_backend_event', 'unavailable'
                ))
            );

            INSERT INTO usage_requests (
                request_id, client_app_id, protocol, requested_model, resolved_model,
                backend_id, started_at_ms, first_token_at_ms, completed_at_ms,
                input_tokens, cached_tokens, output_tokens, total_tokens, status,
                error_category, usage_accuracy
            )
            SELECT
                request_id, client_app_id, protocol, requested_model, resolved_model,
                backend_id, started_at_ms, first_token_at_ms, completed_at_ms,
                input_tokens, cached_tokens, output_tokens, total_tokens, status,
                error_category, usage_accuracy
            FROM usage_requests_v5;

            DROP TABLE usage_requests_v5;
            CREATE INDEX usage_requests_client_started
                ON usage_requests(client_app_id, started_at_ms DESC);
            CREATE INDEX usage_requests_backend_started
                ON usage_requests(backend_id, started_at_ms DESC);",
        ),
        M::up(
            "CREATE TABLE backends (
                id TEXT PRIMARY KEY NOT NULL
                    CHECK(length(id) BETWEEN 1 AND 128),
                display_name TEXT NOT NULL
                    CHECK(length(display_name) BETWEEN 1 AND 256),
                kind TEXT NOT NULL CHECK(kind IN (
                    'managed_llama_cpp', 'external_openai', 'external_anthropic',
                    'external_ollama', 'external_vllm', 'external_llama_cpp'
                )),
                api_root TEXT NOT NULL CHECK(length(api_root) BETWEEN 1 AND 2048),
                auth_style TEXT NOT NULL CHECK(auth_style IN (
                    'none', 'bearer', 'anthropic_api_key'
                )),
                credential_id TEXT,
                enabled INTEGER NOT NULL CHECK(enabled IN (0, 1)),
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );

            CREATE TABLE model_routes (
                alias TEXT PRIMARY KEY NOT NULL
                    CHECK(length(alias) BETWEEN 1 AND 256 AND alias <> 'hal100-active'),
                backend_id TEXT NOT NULL REFERENCES backends(id) ON DELETE RESTRICT,
                resolved_model TEXT NOT NULL
                    CHECK(length(resolved_model) BETWEEN 1 AND 256),
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );

            CREATE INDEX model_routes_backend ON model_routes(backend_id);",
        ),
        M::up(
            "CREATE TABLE integration_resources (
                integration_id TEXT NOT NULL
                    REFERENCES integrations(id) ON DELETE CASCADE,
                role TEXT NOT NULL CHECK(role IN (
                    'configuration', 'credential', 'auxiliary_configuration'
                )),
                path TEXT NOT NULL,
                managed_content_hash BLOB NOT NULL CHECK(length(managed_content_hash) = 32),
                backup_path TEXT,
                contains_secret INTEGER NOT NULL CHECK(contains_secret IN (0, 1)),
                PRIMARY KEY(integration_id, role),
                CHECK(contains_secret = 0 OR backup_path IS NULL)
            );

            CREATE INDEX integration_resources_path
                ON integration_resources(path);

            INSERT INTO integration_resources (
                integration_id, role, path, managed_content_hash, backup_path,
                contains_secret
            )
            SELECT
                id, 'configuration', config_path, managed_fragment_hash, backup_path, 0
            FROM integrations;

            INSERT INTO integration_resources (
                integration_id, role, path, managed_content_hash, backup_path,
                contains_secret
            )
            SELECT
                integrations.id,
                'credential',
                integrations.credential_path,
                (
                    SELECT api_key_hashes.key_hash
                    FROM api_key_hashes
                    WHERE api_key_hashes.client_app_id = integrations.id
                    ORDER BY api_key_hashes.id
                    LIMIT 1
                ),
                NULL,
                1
            FROM integrations
            WHERE EXISTS (
                SELECT 1 FROM api_key_hashes
                WHERE api_key_hashes.client_app_id = integrations.id
            );",
        ),
        M::up(
            "CREATE INDEX usage_requests_started
                ON usage_requests(started_at_ms DESC);",
        ),
        M::up(
            "CREATE TABLE runtime_profiles (
                id TEXT PRIMARY KEY NOT NULL
                    CHECK(length(id) BETWEEN 1 AND 128),
                name TEXT NOT NULL
                    CHECK(length(trim(name)) BETWEEN 1 AND 80),
                description TEXT NOT NULL
                    CHECK(length(description) <= 500),
                spec_version INTEGER NOT NULL CHECK(spec_version = 1),
                model_id TEXT NOT NULL
                    CHECK(length(model_id) BETWEEN 1 AND 256),
                model_display_name TEXT NOT NULL
                    CHECK(length(model_display_name) BETWEEN 1 AND 256),
                model_sha256 BLOB NOT NULL CHECK(length(model_sha256) = 32),
                engine TEXT NOT NULL CHECK(engine = 'llama.cpp'),
                engine_version TEXT NOT NULL
                    CHECK(length(engine_version) BETWEEN 1 AND 64),
                capacity_tier TEXT NOT NULL
                    CHECK(length(capacity_tier) BETWEEN 1 AND 64),
                context_window_tokens INTEGER NOT NULL
                    CHECK(context_window_tokens BETWEEN 1024 AND 1048576),
                capacity_revision TEXT NOT NULL
                    CHECK(length(capacity_revision) BETWEEN 1 AND 64),
                verified_at_ms INTEGER NOT NULL,
                last_activated_at_ms INTEGER,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                UNIQUE(model_id, engine)
            );

            CREATE INDEX runtime_profiles_last_activated
                ON runtime_profiles(last_activated_at_ms DESC, updated_at_ms DESC);",
        ),
        M::up(
            "DROP INDEX runtime_profiles_last_activated;
            ALTER TABLE runtime_profiles RENAME TO runtime_profiles_v10;

            CREATE TABLE runtime_profiles (
                id TEXT PRIMARY KEY NOT NULL
                    CHECK(length(id) BETWEEN 1 AND 128),
                name TEXT NOT NULL
                    CHECK(length(trim(name)) BETWEEN 1 AND 80),
                description TEXT NOT NULL
                    CHECK(length(description) <= 500),
                spec_version INTEGER NOT NULL CHECK(spec_version = 1),
                model_id TEXT NOT NULL
                    CHECK(length(model_id) BETWEEN 1 AND 256),
                model_display_name TEXT NOT NULL
                    CHECK(length(model_display_name) BETWEEN 1 AND 256),
                model_sha256 BLOB NOT NULL CHECK(length(model_sha256) = 32),
                engine TEXT NOT NULL CHECK(engine IN (
                    'llama.cpp', 'ollama', 'mlx-lm', 'vllm', 'sglang',
                    'tensorrt-llm', 'openvino', 'mlc-llm', 'lmdeploy'
                )),
                engine_version TEXT NOT NULL
                    CHECK(length(engine_version) BETWEEN 1 AND 64),
                capacity_tier TEXT NOT NULL
                    CHECK(length(capacity_tier) BETWEEN 1 AND 64),
                context_window_tokens INTEGER NOT NULL
                    CHECK(context_window_tokens BETWEEN 1024 AND 1048576),
                capacity_revision TEXT NOT NULL
                    CHECK(length(capacity_revision) BETWEEN 1 AND 64),
                verified_at_ms INTEGER NOT NULL,
                last_activated_at_ms INTEGER,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                UNIQUE(model_id, engine)
            );

            INSERT INTO runtime_profiles (
                id, name, description, spec_version, model_id, model_display_name,
                model_sha256, engine, engine_version, capacity_tier,
                context_window_tokens, capacity_revision, verified_at_ms,
                last_activated_at_ms, created_at_ms, updated_at_ms
            )
            SELECT
                id, name, description, spec_version, model_id, model_display_name,
                model_sha256, engine, engine_version, capacity_tier,
                context_window_tokens, capacity_revision, verified_at_ms,
                last_activated_at_ms, created_at_ms, updated_at_ms
            FROM runtime_profiles_v10;

            DROP TABLE runtime_profiles_v10;
            CREATE INDEX runtime_profiles_last_activated
                ON runtime_profiles(last_activated_at_ms DESC, updated_at_ms DESC);",
        ),
        M::up(
            "DROP INDEX runtime_profiles_last_activated;
            ALTER TABLE runtime_profiles RENAME TO runtime_profiles_v11;

            CREATE TABLE runtime_profiles (
                id TEXT PRIMARY KEY NOT NULL
                    CHECK(length(id) BETWEEN 1 AND 128),
                name TEXT NOT NULL
                    CHECK(length(trim(name)) BETWEEN 1 AND 80),
                description TEXT NOT NULL
                    CHECK(length(description) <= 500),
                spec_version INTEGER NOT NULL CHECK(spec_version IN (1, 2)),
                ownership TEXT NOT NULL CHECK(ownership IN ('managed', 'external')),
                backend_id TEXT CHECK(backend_id IS NULL OR length(backend_id) BETWEEN 1 AND 128),
                backend_api_root TEXT CHECK(
                    backend_api_root IS NULL OR length(backend_api_root) BETWEEN 1 AND 2048
                ),
                model_id TEXT NOT NULL
                    CHECK(length(model_id) BETWEEN 1 AND 256),
                model_display_name TEXT NOT NULL
                    CHECK(length(model_display_name) BETWEEN 1 AND 256),
                model_digest TEXT NOT NULL CHECK(
                    length(model_digest) = 64
                    AND model_digest NOT GLOB '*[^0-9a-f]*'
                ),
                model_digest_kind TEXT NOT NULL CHECK(
                    model_digest_kind IN ('sha256', 'ollama_digest')
                ),
                engine TEXT NOT NULL CHECK(engine IN (
                    'llama.cpp', 'ollama', 'mlx-lm', 'vllm', 'sglang',
                    'tensorrt-llm', 'openvino', 'mlc-llm', 'lmdeploy'
                )),
                engine_version TEXT NOT NULL
                    CHECK(length(engine_version) BETWEEN 1 AND 64),
                capacity_tier TEXT CHECK(
                    capacity_tier IS NULL OR length(capacity_tier) BETWEEN 1 AND 64
                ),
                context_window_tokens INTEGER CHECK(
                    context_window_tokens IS NULL
                    OR context_window_tokens BETWEEN 1024 AND 1048576
                ),
                capacity_revision TEXT CHECK(
                    capacity_revision IS NULL OR length(capacity_revision) BETWEEN 1 AND 64
                ),
                verified_at_ms INTEGER NOT NULL,
                last_activated_at_ms INTEGER,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                CHECK(
                    (ownership = 'managed'
                     AND backend_id IS NULL
                     AND backend_api_root IS NULL
                     AND model_digest_kind = 'sha256'
                     AND capacity_tier IS NOT NULL
                     AND context_window_tokens IS NOT NULL
                     AND capacity_revision IS NOT NULL)
                    OR
                    (ownership = 'external'
                     AND backend_id IS NOT NULL
                     AND backend_api_root IS NOT NULL
                     AND model_digest_kind = 'ollama_digest'
                     AND capacity_tier IS NULL
                     AND context_window_tokens IS NULL
                     AND capacity_revision IS NULL)
                )
            );

            INSERT INTO runtime_profiles (
                id, name, description, spec_version, ownership, backend_id,
                backend_api_root, model_id, model_display_name, model_digest,
                model_digest_kind, engine, engine_version, capacity_tier,
                context_window_tokens, capacity_revision, verified_at_ms,
                last_activated_at_ms, created_at_ms, updated_at_ms
            )
            SELECT
                id, name, description, spec_version, 'managed', NULL, NULL,
                model_id, model_display_name, lower(hex(model_sha256)), 'sha256',
                engine, engine_version, capacity_tier, context_window_tokens,
                capacity_revision, verified_at_ms, last_activated_at_ms,
                created_at_ms, updated_at_ms
            FROM runtime_profiles_v11;

            DROP TABLE runtime_profiles_v11;
            CREATE INDEX runtime_profiles_last_activated
                ON runtime_profiles(last_activated_at_ms DESC, updated_at_ms DESC);
            CREATE UNIQUE INDEX runtime_profiles_managed_identity
                ON runtime_profiles(engine, model_id)
                WHERE ownership = 'managed';
            CREATE UNIQUE INDEX runtime_profiles_external_identity
                ON runtime_profiles(backend_id, engine, model_id)
                WHERE ownership = 'external';",
        ),
        M::up(
            "ALTER TABLE backends ADD COLUMN engine_kind TEXT CHECK(
                engine_kind IS NULL OR engine_kind IN (
                    'llama.cpp', 'ollama', 'mlx-lm', 'vllm', 'sglang',
                    'tensorrt-llm', 'openvino', 'mlc-llm', 'lmdeploy'
                )
            );
            ALTER TABLE backends ADD COLUMN adapter_variant TEXT CHECK(
                adapter_variant IS NULL OR length(adapter_variant) BETWEEN 1 AND 64
            );
            ALTER TABLE backends ADD COLUMN deployment TEXT NOT NULL DEFAULT 'local'
                CHECK(deployment IN ('local', 'remote'));
            ALTER TABLE backends ADD COLUMN config_revision INTEGER NOT NULL DEFAULT 1
                CHECK(config_revision >= 1);

            UPDATE backends SET
                engine_kind = CASE kind
                    WHEN 'managed_llama_cpp' THEN 'llama.cpp'
                    WHEN 'external_llama_cpp' THEN 'llama.cpp'
                    WHEN 'external_ollama' THEN 'ollama'
                    WHEN 'external_vllm' THEN 'vllm'
                    ELSE NULL
                END,
                adapter_variant = CASE kind
                    WHEN 'managed_llama_cpp' THEN 'hal100-managed-metal'
                    WHEN 'external_llama_cpp' THEN 'official-openai-server'
                    WHEN 'external_ollama' THEN 'official-loopback-api'
                    WHEN 'external_vllm' THEN 'official-openai-server'
                    ELSE NULL
                END,
                deployment = CASE
                    WHEN api_root GLOB 'http://127.0.0.1:*' THEN 'local'
                    ELSE 'remote'
                END,
                config_revision = 1;

            DROP INDEX runtime_profiles_last_activated;
            DROP INDEX runtime_profiles_managed_identity;
            DROP INDEX runtime_profiles_external_identity;
            ALTER TABLE runtime_profiles RENAME TO runtime_profiles_v12;

            CREATE TABLE runtime_profiles (
                id TEXT PRIMARY KEY NOT NULL
                    CHECK(length(id) BETWEEN 1 AND 128),
                name TEXT NOT NULL
                    CHECK(length(trim(name)) BETWEEN 1 AND 80),
                description TEXT NOT NULL
                    CHECK(length(description) <= 500),
                spec_version INTEGER NOT NULL CHECK(spec_version IN (1, 2, 3)),
                ownership TEXT NOT NULL CHECK(ownership IN ('managed', 'external')),
                backend_id TEXT CHECK(backend_id IS NULL OR length(backend_id) BETWEEN 1 AND 128),
                backend_api_root TEXT CHECK(
                    backend_api_root IS NULL OR length(backend_api_root) BETWEEN 1 AND 2048
                ),
                model_id TEXT NOT NULL
                    CHECK(length(model_id) BETWEEN 1 AND 256),
                model_display_name TEXT NOT NULL
                    CHECK(length(model_display_name) BETWEEN 1 AND 256),
                model_digest TEXT NOT NULL CHECK(
                    length(model_digest) = 64
                    AND model_digest NOT GLOB '*[^0-9a-f]*'
                ),
                model_digest_kind TEXT NOT NULL CHECK(
                    model_digest_kind IN ('sha256', 'ollama_digest', 'evidence_fingerprint')
                ),
                engine TEXT NOT NULL CHECK(engine IN (
                    'llama.cpp', 'ollama', 'mlx-lm', 'vllm', 'sglang',
                    'tensorrt-llm', 'openvino', 'mlc-llm', 'lmdeploy'
                )),
                engine_version TEXT NOT NULL
                    CHECK(length(engine_version) BETWEEN 1 AND 64),
                capacity_tier TEXT CHECK(
                    capacity_tier IS NULL OR length(capacity_tier) BETWEEN 1 AND 64
                ),
                context_window_tokens INTEGER CHECK(
                    context_window_tokens IS NULL
                    OR context_window_tokens BETWEEN 1024 AND 1048576
                ),
                capacity_revision TEXT CHECK(
                    capacity_revision IS NULL OR length(capacity_revision) BETWEEN 1 AND 64
                ),
                adapter_variant TEXT CHECK(
                    adapter_variant IS NULL OR length(adapter_variant) BETWEEN 1 AND 64
                ),
                adapter_contract_revision TEXT CHECK(
                    adapter_contract_revision IS NULL
                    OR length(adapter_contract_revision) BETWEEN 1 AND 64
                ),
                backend_config_revision INTEGER CHECK(
                    backend_config_revision IS NULL OR backend_config_revision >= 1
                ),
                origin_fingerprint TEXT CHECK(
                    origin_fingerprint IS NULL OR (
                        length(origin_fingerprint) = 64
                        AND origin_fingerprint NOT GLOB '*[^0-9a-f]*'
                    )
                ),
                evidence_kind TEXT CHECK(
                    evidence_kind IS NULL OR evidence_kind IN (
                        'content_digest', 'repository_revision',
                        'deployment_fingerprint', 'catalog_identity'
                    )
                ),
                evidence_algorithm TEXT CHECK(
                    evidence_algorithm IS NULL OR length(evidence_algorithm) BETWEEN 1 AND 64
                ),
                evidence_value TEXT CHECK(
                    evidence_value IS NULL OR length(evidence_value) BETWEEN 1 AND 512
                ),
                protocol_capability_hash TEXT CHECK(
                    protocol_capability_hash IS NULL OR (
                        length(protocol_capability_hash) = 64
                        AND protocol_capability_hash NOT GLOB '*[^0-9a-f]*'
                    )
                ),
                verified_at_ms INTEGER NOT NULL,
                last_activated_at_ms INTEGER,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                CHECK(
                    spec_version IN (1, 2)
                    OR (
                        adapter_variant IS NOT NULL
                        AND adapter_contract_revision IS NOT NULL
                        AND evidence_kind IS NOT NULL
                        AND evidence_algorithm IS NOT NULL
                        AND evidence_value IS NOT NULL
                        AND protocol_capability_hash IS NOT NULL
                        AND (
                            (ownership = 'managed'
                             AND backend_config_revision IS NULL
                             AND origin_fingerprint IS NULL)
                            OR
                            (ownership = 'external'
                             AND backend_id IS NOT NULL
                             AND backend_api_root IS NOT NULL
                             AND backend_config_revision IS NOT NULL
                             AND origin_fingerprint IS NOT NULL)
                        )
                    )
                )
            );

            INSERT INTO runtime_profiles (
                id, name, description, spec_version, ownership, backend_id,
                backend_api_root, model_id, model_display_name, model_digest,
                model_digest_kind, engine, engine_version, capacity_tier,
                context_window_tokens, capacity_revision, verified_at_ms,
                last_activated_at_ms, created_at_ms, updated_at_ms
            )
            SELECT
                id, name, description, spec_version, ownership, backend_id,
                backend_api_root, model_id, model_display_name, model_digest,
                model_digest_kind, engine, engine_version, capacity_tier,
                context_window_tokens, capacity_revision, verified_at_ms,
                last_activated_at_ms, created_at_ms, updated_at_ms
            FROM runtime_profiles_v12;

            DROP TABLE runtime_profiles_v12;
            CREATE INDEX runtime_profiles_last_activated
                ON runtime_profiles(last_activated_at_ms DESC, updated_at_ms DESC);
            CREATE UNIQUE INDEX runtime_profiles_managed_identity
                ON runtime_profiles(engine, model_id)
                WHERE ownership = 'managed';
            CREATE UNIQUE INDEX runtime_profiles_external_identity
                ON runtime_profiles(backend_id, engine, model_id)
                WHERE ownership = 'external';

            CREATE TABLE runtime_activation_journal (
                id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
                profile_id TEXT NOT NULL CHECK(length(profile_id) BETWEEN 1 AND 128),
                phase TEXT NOT NULL CHECK(phase IN (
                    'journaled', 'quiesced', 'route_switched', 'compensating',
                    'recovery_required'
                )),
                previous_route_json TEXT CHECK(
                    previous_route_json IS NULL OR length(previous_route_json) <= 4096
                ),
                previous_managed_model_id TEXT CHECK(
                    previous_managed_model_id IS NULL
                    OR length(previous_managed_model_id) BETWEEN 1 AND 256
                ),
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );
            CREATE UNIQUE INDEX runtime_activation_single_inflight
                ON runtime_activation_journal((1));",
        ),
        M::up(
            "ALTER TABLE runtime_profiles ADD COLUMN support_platform TEXT CHECK(
                support_platform IS NULL OR support_platform IN ('macos', 'windows', 'linux')
            );
            ALTER TABLE runtime_profiles ADD COLUMN support_architecture TEXT CHECK(
                support_architecture IS NULL OR support_architecture IN ('aarch64', 'x86_64')
            );
            ALTER TABLE runtime_profiles ADD COLUMN support_accelerator TEXT CHECK(
                support_accelerator IS NULL OR support_accelerator IN (
                    'cpu', 'metal', 'cuda', 'rocm', 'vulkan', 'sycl', 'openvino'
                )
            );
            ALTER TABLE runtime_profiles ADD COLUMN support_deployment TEXT CHECK(
                support_deployment IS NULL OR support_deployment IN ('local', 'remote')
            );

            CREATE TRIGGER runtime_profiles_support_cell_insert
            BEFORE INSERT ON runtime_profiles
            WHEN (
                (NEW.support_platform IS NULL
                 OR NEW.support_architecture IS NULL
                 OR NEW.support_accelerator IS NULL
                 OR NEW.support_deployment IS NULL)
                AND NOT (
                    NEW.support_platform IS NULL
                    AND NEW.support_architecture IS NULL
                    AND NEW.support_accelerator IS NULL
                    AND NEW.support_deployment IS NULL
                )
            )
            BEGIN
                SELECT RAISE(ABORT, 'runtime profile support cell must be complete');
            END;

            CREATE TRIGGER runtime_profiles_support_cell_update
            BEFORE UPDATE OF support_platform, support_architecture,
                support_accelerator, support_deployment ON runtime_profiles
            WHEN (
                (NEW.support_platform IS NULL
                 OR NEW.support_architecture IS NULL
                 OR NEW.support_accelerator IS NULL
                 OR NEW.support_deployment IS NULL)
                AND NOT (
                    NEW.support_platform IS NULL
                    AND NEW.support_architecture IS NULL
                    AND NEW.support_accelerator IS NULL
                    AND NEW.support_deployment IS NULL
                )
            )
            BEGIN
                SELECT RAISE(ABORT, 'runtime profile support cell must be complete');
            END;",
        ),
        M::up(
            "DROP TRIGGER runtime_profiles_support_cell_insert;
            DROP TRIGGER runtime_profiles_support_cell_update;
            DROP INDEX runtime_profiles_last_activated;
            DROP INDEX runtime_profiles_managed_identity;
            DROP INDEX runtime_profiles_external_identity;
            ALTER TABLE runtime_profiles RENAME TO runtime_profiles_v14;

            CREATE TABLE runtime_profiles (
                id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
                name TEXT NOT NULL CHECK(length(trim(name)) BETWEEN 1 AND 80),
                description TEXT NOT NULL CHECK(length(description) <= 500),
                spec_version INTEGER NOT NULL CHECK(spec_version IN (1, 2, 3)),
                ownership TEXT NOT NULL CHECK(ownership IN ('managed', 'external')),
                backend_id TEXT CHECK(backend_id IS NULL OR length(backend_id) BETWEEN 1 AND 128),
                backend_api_root TEXT CHECK(
                    backend_api_root IS NULL OR length(backend_api_root) BETWEEN 1 AND 2048
                ),
                model_id TEXT NOT NULL CHECK(length(model_id) BETWEEN 1 AND 256),
                model_display_name TEXT NOT NULL CHECK(length(model_display_name) BETWEEN 1 AND 256),
                model_digest TEXT NOT NULL CHECK(
                    length(model_digest) = 64 AND model_digest NOT GLOB '*[^0-9a-f]*'
                ),
                model_digest_kind TEXT NOT NULL CHECK(
                    model_digest_kind IN ('sha256', 'ollama_digest', 'evidence_fingerprint')
                ),
                engine TEXT NOT NULL CHECK(engine IN (
                    'llama.cpp', 'ollama', 'mlx-lm', 'vllm', 'sglang',
                    'tensorrt-llm', 'openvino', 'mlc-llm', 'lmdeploy'
                )),
                engine_version TEXT NOT NULL CHECK(length(engine_version) BETWEEN 1 AND 64),
                capacity_tier TEXT CHECK(
                    capacity_tier IS NULL OR length(capacity_tier) BETWEEN 1 AND 64
                ),
                context_window_tokens INTEGER CHECK(
                    context_window_tokens IS NULL
                    OR context_window_tokens BETWEEN 1024 AND 1048576
                ),
                capacity_revision TEXT CHECK(
                    capacity_revision IS NULL OR length(capacity_revision) BETWEEN 1 AND 64
                ),
                adapter_variant TEXT CHECK(
                    adapter_variant IS NULL OR length(adapter_variant) BETWEEN 1 AND 64
                ),
                adapter_contract_revision TEXT CHECK(
                    adapter_contract_revision IS NULL
                    OR length(adapter_contract_revision) BETWEEN 1 AND 64
                ),
                backend_config_revision INTEGER CHECK(
                    backend_config_revision IS NULL OR backend_config_revision >= 1
                ),
                origin_fingerprint TEXT CHECK(
                    origin_fingerprint IS NULL OR (
                        length(origin_fingerprint) = 64
                        AND origin_fingerprint NOT GLOB '*[^0-9a-f]*'
                    )
                ),
                evidence_kind TEXT CHECK(
                    evidence_kind IS NULL OR evidence_kind IN (
                        'content_digest', 'repository_revision',
                        'deployment_fingerprint', 'catalog_identity'
                    )
                ),
                evidence_algorithm TEXT CHECK(
                    evidence_algorithm IS NULL OR length(evidence_algorithm) BETWEEN 1 AND 64
                ),
                evidence_value TEXT CHECK(
                    evidence_value IS NULL OR length(evidence_value) BETWEEN 1 AND 512
                ),
                protocol_capability_hash TEXT CHECK(
                    protocol_capability_hash IS NULL OR (
                        length(protocol_capability_hash) = 64
                        AND protocol_capability_hash NOT GLOB '*[^0-9a-f]*'
                    )
                ),
                verified_at_ms INTEGER NOT NULL,
                last_activated_at_ms INTEGER,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                support_platform TEXT CHECK(
                    support_platform IS NULL OR support_platform IN ('macos', 'windows', 'linux')
                ),
                support_architecture TEXT CHECK(
                    support_architecture IS NULL OR support_architecture IN ('aarch64', 'x86_64')
                ),
                support_accelerator TEXT CHECK(
                    support_accelerator IS NULL OR support_accelerator IN (
                        'cpu', 'metal', 'cuda', 'rocm', 'vulkan', 'sycl',
                        'intel_gpu', 'intel_npu'
                    )
                ),
                support_deployment TEXT CHECK(
                    support_deployment IS NULL OR support_deployment IN ('local', 'remote')
                ),
                CHECK(
                    spec_version IN (1, 2)
                    OR (
                        adapter_variant IS NOT NULL
                        AND adapter_contract_revision IS NOT NULL
                        AND evidence_kind IS NOT NULL
                        AND evidence_algorithm IS NOT NULL
                        AND evidence_value IS NOT NULL
                        AND protocol_capability_hash IS NOT NULL
                        AND (
                            (ownership = 'managed'
                             AND backend_config_revision IS NULL
                             AND origin_fingerprint IS NULL)
                            OR
                            (ownership = 'external'
                             AND backend_id IS NOT NULL
                             AND backend_api_root IS NOT NULL
                             AND backend_config_revision IS NOT NULL
                             AND origin_fingerprint IS NOT NULL)
                        )
                    )
                )
            );

            INSERT INTO runtime_profiles (
                id, name, description, spec_version, ownership, backend_id,
                backend_api_root, model_id, model_display_name, model_digest,
                model_digest_kind, engine, engine_version, capacity_tier,
                context_window_tokens, capacity_revision, adapter_variant,
                adapter_contract_revision, backend_config_revision, origin_fingerprint,
                evidence_kind, evidence_algorithm, evidence_value,
                protocol_capability_hash, verified_at_ms, last_activated_at_ms,
                created_at_ms, updated_at_ms, support_platform, support_architecture,
                support_accelerator, support_deployment
            )
            SELECT
                id, name, description, spec_version, ownership, backend_id,
                backend_api_root, model_id, model_display_name, model_digest,
                model_digest_kind, engine, engine_version, capacity_tier,
                context_window_tokens, capacity_revision, adapter_variant,
                adapter_contract_revision, backend_config_revision, origin_fingerprint,
                evidence_kind, evidence_algorithm, evidence_value,
                protocol_capability_hash, verified_at_ms, last_activated_at_ms,
                created_at_ms, updated_at_ms,
                CASE WHEN support_accelerator = 'openvino' THEN NULL ELSE support_platform END,
                CASE WHEN support_accelerator = 'openvino' THEN NULL ELSE support_architecture END,
                CASE WHEN support_accelerator = 'openvino' THEN NULL ELSE support_accelerator END,
                CASE WHEN support_accelerator = 'openvino' THEN NULL ELSE support_deployment END
            FROM runtime_profiles_v14;

            DROP TABLE runtime_profiles_v14;
            CREATE INDEX runtime_profiles_last_activated
                ON runtime_profiles(last_activated_at_ms DESC, updated_at_ms DESC);
            CREATE UNIQUE INDEX runtime_profiles_managed_identity
                ON runtime_profiles(engine, model_id) WHERE ownership = 'managed';
            CREATE UNIQUE INDEX runtime_profiles_external_identity
                ON runtime_profiles(backend_id, engine, model_id) WHERE ownership = 'external';

            CREATE TRIGGER runtime_profiles_support_cell_insert
            BEFORE INSERT ON runtime_profiles
            WHEN (
                (NEW.support_platform IS NULL
                 OR NEW.support_architecture IS NULL
                 OR NEW.support_accelerator IS NULL
                 OR NEW.support_deployment IS NULL)
                AND NOT (
                    NEW.support_platform IS NULL
                    AND NEW.support_architecture IS NULL
                    AND NEW.support_accelerator IS NULL
                    AND NEW.support_deployment IS NULL
                )
            )
            BEGIN
                SELECT RAISE(ABORT, 'runtime profile support cell must be complete');
            END;

            CREATE TRIGGER runtime_profiles_support_cell_update
            BEFORE UPDATE OF support_platform, support_architecture,
                support_accelerator, support_deployment ON runtime_profiles
            WHEN (
                (NEW.support_platform IS NULL
                 OR NEW.support_architecture IS NULL
                 OR NEW.support_accelerator IS NULL
                 OR NEW.support_deployment IS NULL)
                AND NOT (
                    NEW.support_platform IS NULL
                    AND NEW.support_architecture IS NULL
                    AND NEW.support_accelerator IS NULL
                    AND NEW.support_deployment IS NULL
                )
            )
            BEGIN
                SELECT RAISE(ABORT, 'runtime profile support cell must be complete');
            END;

            UPDATE backends
            SET engine_kind = NULL, adapter_variant = NULL, config_revision = config_revision + 1
            WHERE engine_kind = 'openvino' AND adapter_variant = 'ovms-openai-server';",
        ),
    ])
}

fn managed_resource_role_key(role: ManagedIntegrationResourceRole) -> &'static str {
    match role {
        ManagedIntegrationResourceRole::Configuration => "configuration",
        ManagedIntegrationResourceRole::Credential => "credential",
        ManagedIntegrationResourceRole::AuxiliaryConfiguration => "auxiliary_configuration",
    }
}

fn parse_managed_resource_role(value: &str) -> rusqlite::Result<ManagedIntegrationResourceRole> {
    match value {
        "configuration" => Ok(ManagedIntegrationResourceRole::Configuration),
        "credential" => Ok(ManagedIntegrationResourceRole::Credential),
        "auxiliary_configuration" => Ok(ManagedIntegrationResourceRole::AuxiliaryConfiguration),
        _ => Err(invalid_column("managed integration resource role")),
    }
}

fn model_source_key(source: ModelSource) -> &'static str {
    match source {
        ModelSource::HuggingFace => "hugging_face",
        ModelSource::ModelScope => "modelscope",
        ModelSource::LocalFile => "local_file",
    }
}

fn download_source_key(source: DownloadSource) -> &'static str {
    match source {
        DownloadSource::HuggingFace => "hugging_face",
        DownloadSource::ModelScope => "modelscope",
    }
}

fn parse_download_source(value: &str) -> rusqlite::Result<DownloadSource> {
    match value {
        "hugging_face" => Ok(DownloadSource::HuggingFace),
        "modelscope" => Ok(DownloadSource::ModelScope),
        _ => Err(invalid_column("download source")),
    }
}

fn download_state_key(state: ModelDownloadState) -> &'static str {
    match state {
        ModelDownloadState::Pending => "pending",
        ModelDownloadState::Downloading => "downloading",
        ModelDownloadState::Paused => "paused",
        ModelDownloadState::Verifying => "verifying",
        ModelDownloadState::Installing => "installing",
        ModelDownloadState::Ready => "ready",
        ModelDownloadState::Failed => "failed",
        ModelDownloadState::Cancelled => "cancelled",
    }
}

fn parse_download_state(value: &str) -> rusqlite::Result<ModelDownloadState> {
    match value {
        "pending" | "resolving" => Ok(ModelDownloadState::Pending),
        "downloading" => Ok(ModelDownloadState::Downloading),
        "paused" => Ok(ModelDownloadState::Paused),
        "verifying" => Ok(ModelDownloadState::Verifying),
        "installing" => Ok(ModelDownloadState::Installing),
        "ready" => Ok(ModelDownloadState::Ready),
        "failed" => Ok(ModelDownloadState::Failed),
        "cancelled" => Ok(ModelDownloadState::Cancelled),
        _ => Err(invalid_column("download state")),
    }
}

fn parse_model_source(value: &str) -> rusqlite::Result<ModelSource> {
    match value {
        "hugging_face" => Ok(ModelSource::HuggingFace),
        "modelscope" => Ok(ModelSource::ModelScope),
        "local_file" => Ok(ModelSource::LocalFile),
        _ => Err(invalid_column("model source")),
    }
}

fn model_ownership_key(ownership: ModelOwnership) -> &'static str {
    match ownership {
        ModelOwnership::Managed => "managed",
        ModelOwnership::External => "external",
    }
}

fn parse_model_ownership(value: &str) -> rusqlite::Result<ModelOwnership> {
    match value {
        "managed" => Ok(ModelOwnership::Managed),
        "external" => Ok(ModelOwnership::External),
        _ => Err(invalid_column("model ownership")),
    }
}

fn model_state_key(state: LocalModelState) -> &'static str {
    match state {
        LocalModelState::Ready => "ready",
        LocalModelState::Missing => "missing",
        LocalModelState::Changed => "changed",
        LocalModelState::VerificationFailed => "verification_failed",
    }
}

fn parse_model_state(value: &str) -> rusqlite::Result<LocalModelState> {
    match value {
        "ready" => Ok(LocalModelState::Ready),
        "missing" => Ok(LocalModelState::Missing),
        "changed" => Ok(LocalModelState::Changed),
        "verification_failed" => Ok(LocalModelState::VerificationFailed),
        _ => Err(invalid_column("model state")),
    }
}

fn invalid_column(message: &'static str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        format!("invalid {message}").into(),
    )
}

fn file_snapshot_state(
    path: &Path,
    expected_size: i64,
    expected_modified_at_ms: Option<i64>,
) -> String {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return "missing".to_owned();
        }
        Err(_) => return "changed".to_owned(),
    };
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return "changed".to_owned();
    }
    if i64::try_from(metadata.len()).ok() != Some(expected_size) {
        return "changed".to_owned();
    }
    if let Some(expected) = expected_modified_at_ms {
        let actual = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|duration| i64::try_from(duration.as_millis()).ok());
        if actual != Some(expected) {
            return "changed".to_owned();
        }
    }
    "ready".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_initial_migration() {
        let database = Database::open_in_memory().expect("in-memory database");

        assert_eq!(database.schema_version().expect("schema version"), 15);
    }

    #[test]
    fn schema_v15_preserves_profiles_and_upgrades_typed_adapter_and_evidence_identity() {
        let mut connection = rusqlite::Connection::open_in_memory().expect("in-memory database");
        migrations()
            .to_version(&mut connection, 10)
            .expect("schema v10");
        connection
            .execute(
                "INSERT INTO runtime_profiles (
                    id, name, description, spec_version, model_id, model_display_name,
                    model_sha256, engine, engine_version, capacity_tier,
                    context_window_tokens, capacity_revision, verified_at_ms,
                    last_activated_at_ms, created_at_ms, updated_at_ms
                ) VALUES (
                    'legacy-profile', '旧方案', '', 1, 'model-a', 'Model A',
                    ?1, 'llama.cpp', 'b10218', 'baseline16k', 16384,
                    'managed-route-v3', 1, 1, 1, 1
                )",
                [[7_u8; 32].as_slice()],
            )
            .expect("insert schema v10 profile");

        migrations()
            .to_latest(&mut connection)
            .expect("migrate to schema v15");
        upgrade_runtime_profiles_to_v3(&mut connection).expect("upgrade profile spec v3");
        let preserved: String = connection
            .query_row(
                "SELECT engine FROM runtime_profiles WHERE id = 'legacy-profile'",
                [],
                |row| row.get(0),
            )
            .expect("preserved profile");
        assert_eq!(preserved, "llama.cpp");
        let (ownership, digest_kind, digest): (String, String, String) = connection
            .query_row(
                "SELECT ownership, model_digest_kind, model_digest
                 FROM runtime_profiles WHERE id = 'legacy-profile'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("migrated managed identity");
        assert_eq!(ownership, "managed");
        assert_eq!(digest_kind, "sha256");
        assert_eq!(digest, "07".repeat(32));
        let (spec_version, variant, contract, evidence_kind, evidence_algorithm, evidence_value): (
            i64,
            String,
            String,
            String,
            String,
            String,
        ) = connection
            .query_row(
                "SELECT spec_version, adapter_variant, adapter_contract_revision,
                        evidence_kind, evidence_algorithm, evidence_value
                 FROM runtime_profiles WHERE id = 'legacy-profile'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .expect("spec v3 evidence identity");
        assert_eq!(spec_version, 3);
        assert_eq!(variant, "hal100-managed-metal");
        assert_eq!(contract, "engine-contract-v1");
        assert_eq!(evidence_kind, "content_digest");
        assert_eq!(evidence_algorithm, "sha256");
        assert_eq!(evidence_value, "07".repeat(32));

        for kind in hal100_protocol::InferenceEngineKind::ALL {
            connection
                .execute(
                    "UPDATE runtime_profiles SET engine = ?1 WHERE id = 'legacy-profile'",
                    [kind.storage_key()],
                )
                .expect("known engine accepted");
        }
        assert!(
            connection
                .execute(
                    "UPDATE runtime_profiles SET engine = 'arbitrary-shell-engine'
                     WHERE id = 'legacy-profile'",
                    [],
                )
                .is_err()
        );
    }

    #[test]
    fn schema_v15_invalidates_ambiguous_openvino_cells_and_accepts_typed_intel_devices() {
        let mut connection = rusqlite::Connection::open_in_memory().expect("in-memory database");
        migrations()
            .to_version(&mut connection, 14)
            .expect("schema v14");
        connection
            .execute(
                "INSERT INTO runtime_profiles (
                    id, name, description, spec_version, ownership, model_id,
                    model_display_name, model_digest, model_digest_kind, engine,
                    engine_version, adapter_variant, adapter_contract_revision,
                    evidence_kind, evidence_algorithm, evidence_value,
                    protocol_capability_hash, verified_at_ms, created_at_ms, updated_at_ms,
                    support_platform, support_architecture, support_accelerator,
                    support_deployment
                ) VALUES (
                    'legacy-ovms-profile', 'Legacy OVMS', '', 3, 'managed', 'model-a',
                    'Model A', ?1, 'evidence_fingerprint', 'openvino', '2026.1',
                    'ovms-openai-server', 'engine-contract-v1', 'catalog_identity',
                    'catalog-identity-v1', ?1, ?1, 1, 1, 1,
                    'windows', 'x86_64', 'openvino', 'local'
                )",
                ["a".repeat(64)],
            )
            .expect("insert legacy ambiguous OVMS profile");

        migrations().to_latest(&mut connection).expect("schema v15");
        let support_cell: (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = connection
            .query_row(
                "SELECT support_platform, support_architecture,
                            support_accelerator, support_deployment
                     FROM runtime_profiles WHERE id = 'legacy-ovms-profile'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("migrated support cell");
        assert_eq!(support_cell, (None, None, None, None));

        for accelerator in ["intel_gpu", "intel_npu"] {
            connection
                .execute(
                    "UPDATE runtime_profiles SET
                        support_platform = 'windows', support_architecture = 'x86_64',
                        support_accelerator = ?1, support_deployment = 'local'
                     WHERE id = 'legacy-ovms-profile'",
                    [accelerator],
                )
                .expect("typed Intel accelerator accepted");
        }
        assert!(
            connection
                .execute(
                    "UPDATE runtime_profiles SET support_accelerator = 'openvino'
                     WHERE id = 'legacy-ovms-profile'",
                    [],
                )
                .is_err()
        );
    }

    #[test]
    fn runtime_profiles_preserve_bounded_identity_and_activation_history() {
        let database = Database::open_in_memory().expect("in-memory database");
        let profile = StoredRuntimeProfileRecord {
            id: "runtime-profile-test".to_owned(),
            name: "代码助手".to_owned(),
            description: "已验证组合".to_owned(),
            spec_version: 3,
            ownership: "managed".to_owned(),
            backend_id: None,
            backend_api_root: None,
            model_id: "model-test".to_owned(),
            model_display_name: "Qwen Test".to_owned(),
            model_digest: "07".repeat(32),
            model_digest_kind: "sha256".to_owned(),
            engine: "llama.cpp".to_owned(),
            engine_version: "b10218".to_owned(),
            capacity_tier: Some("baseline16k".to_owned()),
            context_window_tokens: Some(16_384),
            capacity_revision: Some("agent-runtime-v2".to_owned()),
            adapter_variant: "hal100-managed-metal".to_owned(),
            adapter_contract_revision: hal100_protocol::ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
            backend_config_revision: None,
            origin_fingerprint: None,
            evidence_kind: "content_digest".to_owned(),
            evidence_algorithm: "sha256".to_owned(),
            evidence_value: "07".repeat(32),
            protocol_capability_hash:
                "1b3e385cbb7f30878cba8eaccf7d5f5e6e1f18b2861a44bc79b18d963cbdd258".to_owned(),
            support_cell: Some(RuntimeProfileSupportCell {
                platform: hal100_protocol::InferencePlatform::MacOs,
                architecture: hal100_protocol::InferenceArchitecture::Aarch64,
                accelerator: hal100_protocol::InferenceAccelerator::Metal,
                deployment: hal100_protocol::InferenceDeployment::Local,
            }),
            verified_at_ms: 100,
            last_activated_at_ms: Some(100),
            created_at_ms: 100,
            updated_at_ms: 100,
        };

        database
            .insert_runtime_profile(&profile)
            .expect("insert runtime profile");
        assert_eq!(
            database.runtime_profile(&profile.id).expect("load profile"),
            Some(profile.clone())
        );
        assert!(
            database
                .update_runtime_profile_metadata(&profile.id, "编码环境", "更新说明", 200)
                .expect("update profile")
        );
        assert!(
            database
                .mark_runtime_profile_activated(
                    &profile.id,
                    &StoredRuntimeProfileVerification {
                        model_digest: "08".repeat(32),
                        evidence_kind: "content_digest".to_owned(),
                        evidence_algorithm: "sha256".to_owned(),
                        evidence_value: "08".repeat(32),
                        engine_version: "b10219".to_owned(),
                        capacity_tier: Some("standard32k".to_owned()),
                        context_window_tokens: Some(32_768),
                        capacity_revision: Some("agent-runtime-v3".to_owned()),
                        support_cell: profile.support_cell,
                    },
                    300,
                )
                .expect("mark profile activated")
        );
        let updated = database
            .runtime_profile(&profile.id)
            .expect("reload profile")
            .expect("profile exists");
        assert_eq!(updated.name, "编码环境");
        assert_eq!(updated.model_digest, "08".repeat(32));
        assert_eq!(updated.evidence_value, "08".repeat(32));
        assert_eq!(updated.context_window_tokens, Some(32_768));
        assert_eq!(updated.last_activated_at_ms, Some(300));
        assert_eq!(updated.support_cell, profile.support_cell);
        assert!(
            database
                .reverify_runtime_profile(
                    &profile.id,
                    &StoredRuntimeProfileVerification {
                        model_digest: "09".repeat(32),
                        evidence_kind: "content_digest".to_owned(),
                        evidence_algorithm: "sha256".to_owned(),
                        evidence_value: "09".repeat(32),
                        engine_version: "b10220".to_owned(),
                        capacity_tier: Some("standard32k".to_owned()),
                        context_window_tokens: Some(32_768),
                        capacity_revision: Some("agent-runtime-v3".to_owned()),
                        support_cell: profile.support_cell,
                    },
                    350,
                )
                .expect("reverify profile")
        );
        let reverified = database
            .runtime_profile(&profile.id)
            .expect("reload reverified profile")
            .expect("profile exists");
        assert_eq!(reverified.model_digest, "09".repeat(32));
        assert_eq!(reverified.evidence_value, "09".repeat(32));
        assert_eq!(reverified.engine_version, "b10220");
        assert_eq!(reverified.verified_at_ms, 350);
        assert_eq!(reverified.updated_at_ms, 350);
        assert_eq!(reverified.last_activated_at_ms, Some(300));
        assert_eq!(reverified.support_cell, profile.support_cell);
        assert!(
            database
                .delete_runtime_profile(&profile.id, &updated.name, 400)
                .expect("delete profile")
        );
        assert!(
            database
                .runtime_profiles()
                .expect("list profiles")
                .is_empty()
        );
        assert_eq!(database.audit_event_count().expect("audit count"), 5);
    }

    #[test]
    fn runtime_profile_support_cell_must_be_complete_and_use_allowlisted_keys() {
        let database = Database::open_in_memory().expect("in-memory database");
        let profile = StoredRuntimeProfileRecord {
            id: "runtime-profile-support-cell".to_owned(),
            name: "支持格测试".to_owned(),
            description: String::new(),
            spec_version: 3,
            ownership: "managed".to_owned(),
            backend_id: None,
            backend_api_root: None,
            model_id: "model-support-cell".to_owned(),
            model_display_name: "Support Cell".to_owned(),
            model_digest: "0a".repeat(32),
            model_digest_kind: "sha256".to_owned(),
            engine: "llama.cpp".to_owned(),
            engine_version: "b10218".to_owned(),
            capacity_tier: Some("baseline16k".to_owned()),
            context_window_tokens: Some(16_384),
            capacity_revision: Some("agent-runtime-v2".to_owned()),
            adapter_variant: "hal100-managed-metal".to_owned(),
            adapter_contract_revision: hal100_protocol::ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
            backend_config_revision: None,
            origin_fingerprint: None,
            evidence_kind: "content_digest".to_owned(),
            evidence_algorithm: "sha256".to_owned(),
            evidence_value: "0a".repeat(32),
            protocol_capability_hash:
                "1b3e385cbb7f30878cba8eaccf7d5f5e6e1f18b2861a44bc79b18d963cbdd258".to_owned(),
            support_cell: Some(RuntimeProfileSupportCell {
                platform: hal100_protocol::InferencePlatform::MacOs,
                architecture: hal100_protocol::InferenceArchitecture::Aarch64,
                accelerator: hal100_protocol::InferenceAccelerator::Metal,
                deployment: hal100_protocol::InferenceDeployment::Local,
            }),
            verified_at_ms: 1,
            last_activated_at_ms: None,
            created_at_ms: 1,
            updated_at_ms: 1,
        };
        database
            .insert_runtime_profile(&profile)
            .expect("insert complete support cell");
        let connection = database
            .connection
            .lock()
            .expect("database connection lock");
        assert!(
            connection
                .execute(
                    "UPDATE runtime_profiles SET support_platform = NULL WHERE id = ?1",
                    [&profile.id],
                )
                .is_err()
        );
        assert!(
            connection
                .execute(
                    "UPDATE runtime_profiles SET support_accelerator = 'unknown' WHERE id = ?1",
                    [&profile.id],
                )
                .is_err()
        );
    }

    #[test]
    fn runtime_activation_journal_is_single_flight_cas_and_recovery_required_is_durable() {
        let database = Database::open_in_memory().expect("in-memory database");
        let journal = StoredRuntimeActivationJournal {
            id: "activation-test".to_owned(),
            profile_id: "runtime-profile-test".to_owned(),
            phase: RuntimeActivationPhase::Journaled,
            previous_route: Some(StoredActiveGatewayRoute {
                backend_id: "backend-before".to_owned(),
                resolved_model: Some("model-before".to_owned()),
            }),
            previous_managed_model_id: Some("managed-before".to_owned()),
            created_at_ms: 10,
            updated_at_ms: 10,
        };
        database
            .begin_runtime_activation(&journal)
            .expect("begin activation journal");
        let second = StoredRuntimeActivationJournal {
            id: "activation-second".to_owned(),
            ..journal.clone()
        };
        assert!(database.begin_runtime_activation(&second).is_err());
        assert!(
            !database
                .transition_runtime_activation(
                    &journal.id,
                    RuntimeActivationPhase::Quiesced,
                    RuntimeActivationPhase::RouteSwitched,
                    20,
                )
                .expect("stale CAS returns false")
        );
        assert!(
            database
                .transition_runtime_activation(
                    &journal.id,
                    RuntimeActivationPhase::Journaled,
                    RuntimeActivationPhase::Quiesced,
                    20,
                )
                .expect("journaled to quiesced")
        );
        assert!(
            database
                .transition_runtime_activation(
                    &journal.id,
                    RuntimeActivationPhase::Quiesced,
                    RuntimeActivationPhase::RecoveryRequired,
                    30,
                )
                .expect("mark recovery required")
        );
        let pending = database
            .runtime_activation_journals()
            .expect("pending activation journal");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].phase, RuntimeActivationPhase::RecoveryRequired);
        assert_eq!(pending[0].previous_route, journal.previous_route);
        assert!(
            database
                .finish_runtime_activation(&journal.id, RuntimeActivationPhase::RecoveryRequired,)
                .is_err()
        );
    }

    #[test]
    fn corrupted_database_fails_closed_without_overwriting_the_original_bytes() {
        let data_dir = std::env::temp_dir().join(format!(
            "hal100-corrupt-database-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&data_dir).expect("create corrupt database test directory");
        let database_path = data_dir.join("hal100.sqlite");
        let original = b"HAL100 intentionally corrupt SQLite fixture";
        std::fs::write(&database_path, original).expect("write corrupt database fixture");

        assert!(Database::open(&database_path).is_err());
        assert_eq!(
            std::fs::read(&database_path).expect("read preserved corrupt database"),
            original
        );

        std::fs::remove_dir_all(&data_dir).expect("remove corrupt database test directory");
    }

    #[test]
    #[ignore = "iteration 9 bounded SQLite busy/recovery probe; waits for the five-second timeout"]
    fn database_busy_failure_is_bounded_and_writes_resume_after_lock_release() {
        let data_dir = std::env::temp_dir().join(format!(
            "hal100-busy-database-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&data_dir).expect("create busy database test directory");
        let database_path = data_dir.join("hal100.sqlite");
        let database = Database::open(&database_path).expect("open busy test database");
        let blocker = rusqlite::Connection::open(&database_path).expect("open blocking connection");
        blocker
            .execute_batch("BEGIN IMMEDIATE;")
            .expect("hold SQLite write lock");

        let started = std::time::Instant::now();
        let result = database.insert_audit_event(
            "busy_probe",
            "database",
            "hal100.sqlite",
            r#"{"errorCode":"database_busy_probe"}"#,
            1,
        );
        let elapsed = started.elapsed();
        assert!(matches!(
            result,
            Err(DatabaseError::Sqlite(ref error))
                if matches!(
                    error.sqlite_error_code(),
                    Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
                )
        ));
        assert!(elapsed >= std::time::Duration::from_millis(4_500));
        assert!(elapsed < std::time::Duration::from_secs(7));

        blocker
            .execute_batch("ROLLBACK;")
            .expect("release SQLite write lock");
        database
            .insert_audit_event(
                "busy_probe_recovered",
                "database",
                "hal100.sqlite",
                r#"{"recovered":true}"#,
                2,
            )
            .expect("write after lock release");
        assert_eq!(
            database.audit_log(10).expect("recovery audit").total_count,
            1
        );

        drop(blocker);
        drop(database);
        std::fs::remove_dir_all(&data_dir).expect("remove busy database test directory");
    }

    #[test]
    fn persists_onboarding_and_retention_without_background_cleanup() {
        let database = Database::open_in_memory().expect("in-memory database");
        assert_eq!(
            database.onboarding_state().expect("onboarding"),
            (false, 1, false)
        );
        assert_eq!(
            database.retention_settings().expect("retention"),
            RetentionSettingsDraft {
                usage_retention_days: Some(90),
                audit_retention_days: Some(365),
            }
        );

        database
            .set_onboarding_step(4, 10)
            .expect("persist onboarding step");
        assert_eq!(
            database.onboarding_state().expect("onboarding"),
            (false, 4, false)
        );
        database
            .complete_onboarding(11)
            .expect("complete onboarding");
        assert_eq!(
            database.onboarding_state().expect("onboarding"),
            (true, 5, false)
        );

        let retention = database
            .set_retention_settings(
                RetentionSettingsDraft {
                    usage_retention_days: Some(30),
                    audit_retention_days: None,
                },
                12,
            )
            .expect("save retention");
        assert_eq!(retention.usage_retention_days, Some(30));
        assert_eq!(retention.audit_retention_days, None);
    }

    #[test]
    fn generic_client_lifecycle_stores_only_a_hash_and_audits_revocation() {
        let database = Database::open_in_memory().expect("in-memory database");
        let credential = StoredClientCredential {
            key_id: "generic-client-key".to_owned(),
            client_app_id: "generic-client".to_owned(),
            display_name: "通用客户端".to_owned(),
            display_prefix: "hal100_cli…".to_owned(),
            key_hash: [17; 32],
        };
        database
            .insert_generic_client_credential(&credential, 100)
            .expect("create generic client");
        assert_eq!(database.generic_clients().expect("clients").len(), 1);
        assert!(
            database
                .revoke_generic_client("generic-client", "通用客户端", 101)
                .expect("revoke client")
        );
        assert!(database.generic_clients().expect("clients").is_empty());
        let audit = database.audit_log(20).expect("audit");
        assert_eq!(audit.total_count, 2);
        assert_eq!(audit.events[0].event_type, "generic_client_revoked");
    }

    #[test]
    fn explicit_retention_cleanup_deletes_only_expired_rows_and_redacts_audit_details() {
        let database = Database::open_in_memory().expect("in-memory database");
        let now_ms = 500_i64 * 24 * 60 * 60 * 1_000;
        database
            .upsert_client_credential(
                &StoredClientCredential {
                    key_id: "retention-key".to_owned(),
                    client_app_id: "retention-client".to_owned(),
                    display_name: "Retention".to_owned(),
                    display_prefix: "hal100_ret…".to_owned(),
                    key_hash: [18; 32],
                },
                1,
            )
            .expect("client");
        for (request_id, age_days) in [("expired", 31_i64), ("recent", 29_i64)] {
            database
                .insert_usage_request(&UsageRequestRecord {
                    request_id: request_id.to_owned(),
                    client_app_id: "retention-client".to_owned(),
                    protocol: "openai_chat_completions".to_owned(),
                    requested_model: "model".to_owned(),
                    resolved_model: "model".to_owned(),
                    backend_id: "backend".to_owned(),
                    started_at_ms: now_ms - age_days * 24 * 60 * 60 * 1_000 - 1,
                    first_token_at_ms: None,
                    completed_at_ms: now_ms - age_days * 24 * 60 * 60 * 1_000,
                    input_tokens: Some(1),
                    cached_tokens: Some(0),
                    output_tokens: Some(1),
                    total_tokens: Some(2),
                    status: "succeeded".to_owned(),
                    error_category: None,
                    usage_accuracy: "exact_backend_response".to_owned(),
                })
                .expect("usage");
        }
        database
            .insert_audit_event(
                "sensitive_test",
                "test",
                "target",
                r#"{"displayName":"安全摘要","apiKey":"must-not-render"}"#,
                now_ms,
            )
            .expect("audit");
        database
            .set_retention_settings(
                RetentionSettingsDraft {
                    usage_retention_days: Some(30),
                    audit_retention_days: None,
                },
                now_ms,
            )
            .expect("retention");

        let preview = database.data_cleanup_preview(now_ms).expect("preview");
        assert_eq!(preview.usage_request_count, 1);
        assert_eq!(preview.audit_event_count, 0);
        assert!(
            !format!("{:?}", database.audit_log(20).expect("audit log"))
                .contains("must-not-render")
        );
        let result = database.apply_data_retention(now_ms).expect("cleanup");
        assert_eq!(result.usage_requests_deleted, 1);
        assert_eq!(result.audit_events_deleted, 0);
        assert_eq!(
            database
                .usage_dashboard(50, 0)
                .expect("dashboard")
                .totals
                .request_count,
            1
        );
    }

    #[test]
    fn audit_details_expose_only_safe_agent_action_scalars() {
        let details = safe_audit_details(
            r#"{
                "action":"start_or_switch_model",
                "modelId":"qwen35-2b-q4km",
                "reason":"native_confirmation_cancelled",
                "errorCode":"managed_model_operation_failed",
                "toolCalls":2,
                "toolPolicy":"read_only_allowlist",
                "prompt":"do not render",
                "modelPath":"/private/model.gguf",
                "apiKey":"secret",
                "nested":{"answer":"secret"}
            }"#,
        );
        let rendered = details
            .iter()
            .map(|detail| (detail.key.as_str(), detail.value.as_str()))
            .collect::<std::collections::HashMap<_, _>>();

        assert_eq!(rendered.get("action"), Some(&"start_or_switch_model"));
        assert_eq!(rendered.get("modelId"), Some(&"qwen35-2b-q4km"));
        assert_eq!(
            rendered.get("reason"),
            Some(&"native_confirmation_cancelled")
        );
        assert_eq!(
            rendered.get("errorCode"),
            Some(&"managed_model_operation_failed")
        );
        assert_eq!(rendered.get("toolCalls"), Some(&"2"));
        assert_eq!(rendered.get("toolPolicy"), Some(&"read_only_allowlist"));
        assert!(!rendered.contains_key("prompt"));
        assert!(!rendered.contains_key("modelPath"));
        assert!(!rendered.contains_key("apiKey"));
        assert!(!rendered.contains_key("nested"));
    }

    #[test]
    fn schema_v6_preserves_usage_and_accepts_exact_backend_events() {
        let mut connection = rusqlite::Connection::open_in_memory().expect("in-memory database");
        migrations()
            .to_version(&mut connection, 5)
            .expect("schema v5");
        connection
            .execute(
                "INSERT INTO client_apps (id, display_name, created_at_ms, updated_at_ms)
                 VALUES ('client-1', 'Test', 1, 1)",
                [],
            )
            .expect("client");
        connection
            .execute(
                "INSERT INTO usage_requests (
                    request_id, client_app_id, protocol, requested_model, resolved_model,
                    backend_id, started_at_ms, completed_at_ms, status, usage_accuracy
                 ) VALUES (
                    'old-response', 'client-1', 'openai_chat_completions', 'model', 'model',
                    'backend', 1, 2, 'succeeded', 'exact_backend_response'
                 )",
                [],
            )
            .expect("v5 usage");

        migrations()
            .to_latest(&mut connection)
            .expect("migrate to schema v6");
        let preserved: String = connection
            .query_row(
                "SELECT usage_accuracy FROM usage_requests WHERE request_id = 'old-response'",
                [],
                |row| row.get(0),
            )
            .expect("preserved usage");
        assert_eq!(preserved, "exact_backend_response");
        connection
            .execute(
                "INSERT INTO usage_requests (
                    request_id, client_app_id, protocol, requested_model, resolved_model,
                    backend_id, started_at_ms, completed_at_ms, status, usage_accuracy
                 ) VALUES (
                    'stream-event', 'client-1', 'anthropic_messages', 'model', 'model',
                    'backend', 3, 4, 'succeeded', 'exact_backend_event'
                 )",
                [],
            )
            .expect("event usage accepted");
    }

    #[test]
    fn schema_v7_persists_non_secret_backends_and_restricts_routed_deletion() {
        let database = Database::open_in_memory().expect("in-memory database");
        let backend = StoredBackendRecord {
            id: "secondary-backend".to_owned(),
            display_name: "局域网 vLLM".to_owned(),
            kind: "external_vllm".to_owned(),
            engine_kind: Some("vllm".to_owned()),
            adapter_variant: Some("official-openai-server".to_owned()),
            api_root: "http://192.168.1.20:8000/v1".to_owned(),
            auth_style: "bearer".to_owned(),
            credential_id: Some("hal100.backend.secondary-backend".to_owned()),
            enabled: true,
            created_at_ms: 1_700_000_000_000,
            updated_at_ms: 1_700_000_000_000,
        };
        database.upsert_backend(&backend).expect("store backend");
        let route = StoredModelRouteRecord {
            alias: "fast-local".to_owned(),
            backend_id: backend.id.clone(),
            resolved_model: "Qwen/Qwen3.5-9B".to_owned(),
            created_at_ms: 1_700_000_000_001,
            updated_at_ms: 1_700_000_000_001,
        };
        database
            .upsert_model_route(&route)
            .expect("store model route");
        database
            .set_active_backend_id(Some("secondary-backend"), 1_700_000_000_002)
            .expect("store active backend");

        assert_eq!(database.backends().expect("backends"), vec![backend]);
        assert_eq!(database.model_routes().expect("model routes"), vec![route]);
        assert_eq!(
            database.active_backend_id().expect("active backend"),
            Some("secondary-backend".to_owned())
        );
        assert_eq!(
            database.active_gateway_route().expect("active route"),
            Some(StoredActiveGatewayRoute {
                backend_id: "secondary-backend".to_owned(),
                resolved_model: None,
            })
        );
        let resolved_route = StoredActiveGatewayRoute {
            backend_id: "secondary-backend".to_owned(),
            resolved_model: Some("Qwen/Qwen3.5-9B".to_owned()),
        };
        database
            .set_active_gateway_route(Some(&resolved_route), 1_700_000_000_003)
            .expect("store resolved active route");
        assert_eq!(
            database.active_gateway_route().expect("resolved route"),
            Some(resolved_route)
        );
        assert_eq!(
            database.active_backend_id().expect("legacy active view"),
            Some("secondary-backend".to_owned())
        );
        assert!(matches!(
            database.delete_backend("secondary-backend"),
            Err(DatabaseError::Sqlite(rusqlite::Error::SqliteFailure(_, _)))
        ));
        assert!(
            database
                .delete_model_route("fast-local")
                .expect("delete route")
        );
        assert!(
            database
                .delete_backend("secondary-backend")
                .expect("delete backend")
        );
        database
            .set_active_gateway_route(None, 1_700_000_000_004)
            .expect("clear active backend");
        assert_eq!(database.active_backend_id().expect("cleared active"), None);

        let connection = database.connection.lock().expect("database lock");
        let mut statement = connection
            .prepare("PRAGMA table_info(backends)")
            .expect("backend columns");
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .expect("backend column rows")
            .collect::<Result<Vec<_>, _>>()
            .expect("backend column names");
        assert!(!columns.iter().any(|column| column == "api_key"));
        assert!(!columns.iter().any(|column| column.contains("secret")));
    }

    #[test]
    fn schema_v13_backend_config_revision_changes_only_with_target_configuration() {
        let database = Database::open_in_memory().expect("in-memory database");
        let mut backend = StoredBackendRecord {
            id: "revision-backend".to_owned(),
            display_name: "本机 Ollama".to_owned(),
            kind: "external_ollama".to_owned(),
            engine_kind: None,
            adapter_variant: None,
            api_root: "http://127.0.0.1:11434/v1/".to_owned(),
            auth_style: "none".to_owned(),
            credential_id: None,
            enabled: true,
            created_at_ms: 1,
            updated_at_ms: 1,
        };
        database.upsert_backend(&backend).expect("insert backend");
        assert_eq!(
            database
                .backend_config_revision(&backend.id)
                .expect("initial revision"),
            1
        );

        backend.display_name = "重命名 Ollama".to_owned();
        backend.updated_at_ms = 2;
        database.upsert_backend(&backend).expect("rename backend");
        assert_eq!(
            database
                .backend_config_revision(&backend.id)
                .expect("rename revision"),
            1
        );

        backend.api_root = "http://127.0.0.1:21434/v1/".to_owned();
        backend.updated_at_ms = 3;
        database
            .upsert_backend(&backend)
            .expect("change target backend");
        assert_eq!(
            database
                .backend_config_revision(&backend.id)
                .expect("changed revision"),
            2
        );
    }

    #[test]
    fn stores_only_hashed_client_credentials() {
        let database = Database::open_in_memory().expect("in-memory database");
        let credential = StoredClientCredential {
            key_id: "key-1".to_owned(),
            client_app_id: "client-1".to_owned(),
            display_name: "Test client".to_owned(),
            display_prefix: "hal100_test…".to_owned(),
            key_hash: [7; 32],
        };

        database
            .upsert_client_credential(&credential, 1_700_000_000_000)
            .expect("store credential hash");

        assert_eq!(
            database.load_client_credentials().expect("load hashes"),
            vec![credential]
        );
        let connection = database.connection.lock().expect("database lock");
        let schema = connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'api_key_hashes'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("credential schema");
        assert!(!schema.contains("plaintext"));
    }

    #[test]
    fn stores_integration_ownership_and_credential_in_one_transaction() {
        let database = Database::open_in_memory().expect("in-memory database");
        let credential = StoredClientCredential {
            key_id: "opencode-key".to_owned(),
            client_app_id: "opencode".to_owned(),
            display_name: "OpenCode".to_owned(),
            display_prefix: "hal100_open…".to_owned(),
            key_hash: [8; 32],
        };
        let integration = ManagedIntegrationRecord {
            id: "opencode".to_owned(),
            kind: "opencode-global-provider".to_owned(),
            config_path: "/tmp/opencode.json".to_owned(),
            credential_path: "/tmp/opencode.key".to_owned(),
            managed_fragment_hash: [9; 32],
            backup_path: Some("/tmp/opencode.backup".to_owned()),
            created_at_ms: 10,
            updated_at_ms: 11,
        };

        database
            .upsert_integration_and_credential(&integration, &credential)
            .expect("atomic integration record");

        assert_eq!(
            database
                .managed_integration("opencode")
                .expect("load integration"),
            Some(integration)
        );
        assert_eq!(
            database.load_client_credentials().expect("load key hash"),
            vec![credential]
        );
        assert_eq!(
            database
                .managed_integration_resources("opencode")
                .expect("load managed resources"),
            vec![
                ManagedIntegrationResourceRecord {
                    integration_id: "opencode".to_owned(),
                    role: ManagedIntegrationResourceRole::Configuration,
                    path: "/tmp/opencode.json".to_owned(),
                    managed_content_hash: [9; 32],
                    backup_path: Some("/tmp/opencode.backup".to_owned()),
                    contains_secret: false,
                },
                ManagedIntegrationResourceRecord {
                    integration_id: "opencode".to_owned(),
                    role: ManagedIntegrationResourceRole::Credential,
                    path: "/tmp/opencode.key".to_owned(),
                    managed_content_hash: [8; 32],
                    backup_path: None,
                    contains_secret: true,
                },
            ]
        );
    }

    #[test]
    fn secret_managed_resources_can_never_record_plaintext_backups() {
        let database = Database::open_in_memory().expect("in-memory database");
        let credential = StoredClientCredential {
            key_id: "hermes-key".to_owned(),
            client_app_id: "hermes-agent".to_owned(),
            display_name: "Hermes Agent".to_owned(),
            display_prefix: "hal100_herm…".to_owned(),
            key_hash: [3; 32],
        };
        let integration = ManagedIntegrationRecord {
            id: "hermes-agent".to_owned(),
            kind: "hermes-named-provider".to_owned(),
            config_path: "/tmp/config.yaml".to_owned(),
            credential_path: "/tmp/.env".to_owned(),
            managed_fragment_hash: [4; 32],
            backup_path: None,
            created_at_ms: 10,
            updated_at_ms: 11,
        };
        let resources = [ManagedIntegrationResourceRecord {
            integration_id: integration.id.clone(),
            role: ManagedIntegrationResourceRole::Credential,
            path: integration.credential_path.clone(),
            managed_content_hash: credential.key_hash,
            backup_path: Some("/tmp/unsafe-env-backup".to_owned()),
            contains_secret: true,
        }];

        assert!(matches!(
            database.upsert_integration_resources_and_credential(
                &integration,
                &resources,
                &credential
            ),
            Err(DatabaseError::InvalidData(_))
        ));
        assert_eq!(
            database
                .managed_integration("hermes-agent")
                .expect("load integration"),
            None
        );
    }

    #[test]
    fn leaves_download_source_unset_until_the_user_selects_one() {
        let database = Database::open_in_memory().expect("in-memory database");
        assert_eq!(database.default_download_source().expect("source"), None);

        database
            .set_default_download_source(DownloadSource::ModelScope, 123)
            .expect("save source");
        assert_eq!(
            database.default_download_source().expect("source"),
            Some(DownloadSource::ModelScope)
        );
    }

    #[test]
    fn persists_the_model_catalog_and_location_transactionally() {
        let database = Database::open_in_memory().expect("in-memory database");
        let model = LocalModelSummary {
            id: "qwen-test".to_owned(),
            display_name: "Qwen Test".to_owned(),
            format: "gguf".to_owned(),
            quantization: Some("Q4_K_M".to_owned()),
            source: ModelSource::ModelScope,
            repository: Some("Qwen/test-GGUF".to_owned()),
            revision: Some("main".to_owned()),
            file_name: "qwen-test-q4.gguf".to_owned(),
            ownership: ModelOwnership::Managed,
            license: Some("Apache-2.0".to_owned()),
            state: LocalModelState::Ready,
            path: "/tmp/qwen-test-q4.gguf".to_owned(),
            size_bytes: 4_000_000_000,
        };
        database
            .upsert_local_model(&model, 456)
            .expect("store model");

        let library = database
            .model_library(Path::new("/tmp/models"))
            .expect("model library");
        assert_eq!(library.model_storage_path, "/tmp/models");
        assert_eq!(library.models, vec![model]);
    }

    #[test]
    fn aggregates_exact_usage_and_returns_recent_requests_by_client() {
        let database = Database::open_in_memory().expect("in-memory database");
        database
            .upsert_client_credential(
                &StoredClientCredential {
                    key_id: "test-key".to_owned(),
                    client_app_id: "model-test".to_owned(),
                    display_name: "HAL100 模型测试".to_owned(),
                    display_prefix: "hal100_test…".to_owned(),
                    key_hash: [3; 32],
                },
                10,
            )
            .expect("store test client");
        database
            .insert_usage_request(&UsageRequestRecord {
                request_id: "request-success".to_owned(),
                client_app_id: "model-test".to_owned(),
                protocol: "openai".to_owned(),
                requested_model: "hal100-active".to_owned(),
                resolved_model: "qwen-test".to_owned(),
                backend_id: "llama.cpp".to_owned(),
                started_at_ms: 100,
                first_token_at_ms: Some(110),
                completed_at_ms: 120,
                input_tokens: Some(14),
                cached_tokens: Some(3),
                output_tokens: Some(9),
                total_tokens: Some(23),
                status: "succeeded".to_owned(),
                error_category: None,
                usage_accuracy: "exact_backend_response".to_owned(),
            })
            .expect("store successful usage");
        database
            .insert_usage_request(&UsageRequestRecord {
                request_id: "request-failed".to_owned(),
                client_app_id: "model-test".to_owned(),
                protocol: "openai".to_owned(),
                requested_model: "hal100-active".to_owned(),
                resolved_model: "qwen-test".to_owned(),
                backend_id: "llama.cpp".to_owned(),
                started_at_ms: 200,
                first_token_at_ms: None,
                completed_at_ms: 205,
                input_tokens: None,
                cached_tokens: None,
                output_tokens: None,
                total_tokens: None,
                status: "failed".to_owned(),
                error_category: Some("backend_error".to_owned()),
                usage_accuracy: "unavailable".to_owned(),
            })
            .expect("store failed usage");

        let dashboard = database.usage_dashboard(50, 0).expect("usage dashboard");
        assert_eq!(dashboard.totals.request_count, 2);
        assert_eq!(dashboard.totals.input_tokens, 14);
        assert_eq!(dashboard.totals.cached_tokens, 3);
        assert_eq!(dashboard.totals.output_tokens, 9);
        assert_eq!(dashboard.totals.total_tokens, 23);
        assert_eq!(dashboard.recent_requests.len(), 2);
        assert_eq!(dashboard.daily_usage.len(), 1);
        assert_eq!(dashboard.daily_usage[0].request_count, 2);
        assert_eq!(dashboard.daily_usage[0].input_tokens, 14);
        assert_eq!(dashboard.daily_usage[0].cached_tokens, 3);
        assert_eq!(dashboard.daily_usage[0].output_tokens, 9);
        assert_eq!(dashboard.daily_usage[0].total_tokens, 23);
        let scoped = database
            .usage_scope(&UsageScopeQuery {
                start_at_ms: 0,
                end_at_ms_exclusive: 150,
                series_start_at_ms: 0,
                series_end_at_ms_exclusive: 300,
                client_app_id: Some("model-test".to_owned()),
                resolved_model: Some("qwen-test".to_owned()),
                backend_id: Some("llama.cpp".to_owned()),
                status: None,
                limit: 50,
            })
            .expect("scoped usage");
        assert_eq!(scoped.totals.request_count, 1);
        assert_eq!(scoped.totals.total_tokens, 23);
        assert_eq!(scoped.measured_request_count, 1);
        assert_eq!(scoped.succeeded_request_count, 1);
        assert_eq!(scoped.client_usage[0].total_tokens, 23);
        assert_eq!(scoped.recent_requests.len(), 1);
        assert_eq!(scoped.daily_usage[0].request_count, 2);
        assert_eq!(scoped.hourly_usage[0].request_count, 1);

        let failed_scope = database
            .usage_scope(&UsageScopeQuery {
                start_at_ms: 0,
                end_at_ms_exclusive: 300,
                series_start_at_ms: 0,
                series_end_at_ms_exclusive: 300,
                client_app_id: None,
                resolved_model: None,
                backend_id: None,
                status: Some("failed".to_owned()),
                limit: 50,
            })
            .expect("failed usage scope");
        assert_eq!(failed_scope.totals.request_count, 1);
        assert_eq!(failed_scope.measured_request_count, 0);
        assert_eq!(failed_scope.succeeded_request_count, 0);
        assert_eq!(failed_scope.daily_usage[0].request_count, 1);
        let options = database.usage_filter_options().expect("usage filters");
        assert_eq!(options.earliest_usage_at_ms, Some(100));
        assert_eq!(options.latest_usage_at_ms, Some(200));
        assert_eq!(options.clients[0].value, "model-test");
        assert_eq!(options.models[0].value, "qwen-test");
        assert_eq!(options.backends[0].value, "llama.cpp");
        let recent_activity = database
            .usage_dashboard(50, 150)
            .expect("filtered usage dashboard");
        assert_eq!(recent_activity.recent_requests.len(), 2);
        assert_eq!(recent_activity.daily_usage.len(), 1);
        assert_eq!(recent_activity.daily_usage[0].request_count, 1);
        assert_eq!(recent_activity.daily_usage[0].total_tokens, 0);
        let hourly = database.usage_hourly("1970-01-01").expect("hourly usage");
        assert_eq!(hourly.len(), 1);
        assert_eq!(hourly[0].request_count, 2);
        assert_eq!(hourly[0].input_tokens, 14);
        assert_eq!(hourly[0].cached_tokens, 3);
        assert_eq!(hourly[0].output_tokens, 9);
        assert_eq!(hourly[0].total_tokens, 23);
        assert!(database.usage_hourly("2026-02-30").is_err());
        assert_eq!(dashboard.recent_requests[0].request_id, "request-failed");
        assert_eq!(
            dashboard.recent_requests[1].client_display_name,
            "HAL100 模型测试"
        );
        assert_eq!(
            dashboard.recent_requests[1].usage_accuracy,
            "exact_backend_response"
        );
    }

    #[test]
    #[ignore = "iteration 9 million-row SQLite scale probe; run explicitly"]
    fn million_usage_rows_remain_queryable_and_cleanable_within_the_scale_budget() {
        const ROW_COUNT: u64 = 1_000_000;
        const DAY_MS: i64 = 24 * 60 * 60 * 1_000;
        let data_dir = std::env::temp_dir().join(format!(
            "hal100-million-usage-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&data_dir).expect("create scale test directory");
        let database_path = data_dir.join("hal100.sqlite");
        let database = Database::open(&database_path).expect("open scale test database");

        let insert_started = std::time::Instant::now();
        {
            let connection = database.connection.lock().expect("scale database lock");
            connection
                .execute_batch(
                    "WITH RECURSIVE sequence(value) AS (
                        SELECT 0
                        UNION ALL
                        SELECT value + 1 FROM sequence WHERE value < 999999
                     )
                     INSERT INTO usage_requests (
                        request_id, client_app_id, protocol, requested_model, resolved_model,
                        backend_id, started_at_ms, first_token_at_ms, completed_at_ms,
                        input_tokens, cached_tokens, output_tokens, total_tokens,
                        status, error_category, usage_accuracy
                     )
                     SELECT
                        printf('scale-%07d', value),
                        CASE value % 4
                            WHEN 0 THEN 'opencode'
                            WHEN 1 THEN 'hal100-model-test'
                            WHEN 2 THEN 'hal100-agent'
                            ELSE 'hal100-agent-cloud'
                        END,
                        'openai_chat_completions', 'requested-model', 'resolved-model',
                        printf('backend-%d', value % 3), value, value + 1, value + 2,
                        10, 2, 5, 15, 'succeeded', NULL, 'exact_backend_response'
                     FROM sequence;",
                )
                .expect("insert one million usage rows");
        }
        let insert_elapsed = insert_started.elapsed();
        assert_eq!(
            database.usage_request_count().expect("scale usage count"),
            ROW_COUNT
        );

        let dashboard_started = std::time::Instant::now();
        let dashboard = database.usage_dashboard(50, 0).expect("scale dashboard");
        let dashboard_elapsed = dashboard_started.elapsed();
        assert_eq!(dashboard.totals.request_count, ROW_COUNT);
        assert_eq!(dashboard.totals.input_tokens, ROW_COUNT * 10);
        assert_eq!(dashboard.totals.cached_tokens, ROW_COUNT * 2);
        assert_eq!(dashboard.totals.output_tokens, ROW_COUNT * 5);
        assert_eq!(dashboard.totals.total_tokens, ROW_COUNT * 15);
        assert_eq!(dashboard.recent_requests.len(), 50);
        assert_eq!(dashboard.recent_requests[0].request_id, "scale-0999999");

        let now_ms = 100 * DAY_MS;
        database
            .set_retention_settings(
                RetentionSettingsDraft {
                    usage_retention_days: Some(30),
                    audit_retention_days: None,
                },
                now_ms,
            )
            .expect("scale retention settings");
        let preview_started = std::time::Instant::now();
        let preview = database
            .data_cleanup_preview(now_ms)
            .expect("scale cleanup preview");
        let preview_elapsed = preview_started.elapsed();
        assert_eq!(preview.usage_request_count, ROW_COUNT);

        let cleanup_started = std::time::Instant::now();
        let cleanup = database
            .apply_data_retention(now_ms)
            .expect("scale cleanup");
        let cleanup_elapsed = cleanup_started.elapsed();
        assert_eq!(cleanup.usage_requests_deleted, ROW_COUNT);
        assert_eq!(database.usage_request_count().expect("empty usage"), 0);

        println!(
            "million_usage insert_ms={} dashboard_ms={} preview_ms={} cleanup_ms={} database_bytes={}",
            insert_elapsed.as_millis(),
            dashboard_elapsed.as_millis(),
            preview_elapsed.as_millis(),
            cleanup_elapsed.as_millis(),
            std::fs::metadata(&database_path)
                .expect("scale database metadata")
                .len()
        );
        assert!(insert_elapsed < std::time::Duration::from_secs(60));
        assert!(dashboard_elapsed < std::time::Duration::from_secs(5));
        assert!(preview_elapsed < std::time::Duration::from_secs(5));
        assert!(cleanup_elapsed < std::time::Duration::from_secs(30));

        drop(database);
        std::fs::remove_dir_all(&data_dir).expect("remove scale test directory");
    }

    #[test]
    #[ignore = "iteration 9 ten-thousand-model library scale probe; run explicitly"]
    fn ten_thousand_model_snapshots_refresh_and_list_within_the_scale_budget() {
        const MODEL_COUNT: usize = 10_000;
        let data_dir = std::env::temp_dir().join(format!(
            "hal100-model-library-scale-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&data_dir).expect("create model scale test directory");
        let database_path = data_dir.join("hal100.sqlite");
        let database = Database::open(&database_path).expect("open model scale database");

        let insert_started = std::time::Instant::now();
        {
            let connection = database
                .connection
                .lock()
                .expect("model scale database lock");
            connection
                .execute_batch(
                    "WITH RECURSIVE sequence(value) AS (
                        SELECT 0
                        UNION ALL
                        SELECT value + 1 FROM sequence WHERE value < 9999
                     )
                     INSERT INTO models (
                        id, display_name, format, quantization, source, repository, revision,
                        file_name, ownership, license, capabilities_json, state,
                        created_at_ms, updated_at_ms
                     )
                     SELECT
                        printf('model-%05d', value), printf('Model %05d', value), 'gguf',
                        'Q4_K_M', 'hugging_face', 'HAL100/scale-fixture', 'main',
                        printf('model-%05d.gguf', value), 'managed', 'MIT', '{}', 'ready',
                        value, value
                     FROM sequence;

                     WITH RECURSIVE sequence(value) AS (
                        SELECT 0
                        UNION ALL
                        SELECT value + 1 FROM sequence WHERE value < 9999
                     )
                     INSERT INTO model_locations (
                        model_id, path, size_bytes, modified_at_ms, sha256, updated_at_ms
                     )
                     SELECT
                        printf('model-%05d', value),
                        printf('/nonexistent/hal100-model-%05d.gguf', value),
                        1048576, NULL, zeroblob(32), value
                     FROM sequence;",
                )
                .expect("insert ten thousand model snapshots");
        }
        let insert_elapsed = insert_started.elapsed();

        let refresh_started = std::time::Instant::now();
        database
            .refresh_local_model_states()
            .expect("refresh missing model snapshots");
        let refresh_elapsed = refresh_started.elapsed();

        let list_started = std::time::Instant::now();
        let library = database
            .model_library(&data_dir.join("models"))
            .expect("list large model library");
        let list_elapsed = list_started.elapsed();

        assert_eq!(library.models.len(), MODEL_COUNT);
        assert!(
            library
                .models
                .iter()
                .all(|model| model.state == LocalModelState::Missing)
        );
        println!(
            "model_library_scale insert_ms={} refresh_ms={} list_ms={} database_bytes={}",
            insert_elapsed.as_millis(),
            refresh_elapsed.as_millis(),
            list_elapsed.as_millis(),
            std::fs::metadata(&database_path)
                .expect("model scale database metadata")
                .len()
        );
        assert!(insert_elapsed < std::time::Duration::from_secs(10));
        assert!(refresh_elapsed < std::time::Duration::from_secs(10));
        assert!(list_elapsed < std::time::Duration::from_secs(5));

        drop(database);
        std::fs::remove_dir_all(&data_dir).expect("remove model scale test directory");
    }
}
