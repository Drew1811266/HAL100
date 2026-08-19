use std::{path::Path, sync::Mutex};

use hal100_protocol::{
    AuditDetail, AuditEventSummary, AuditLog, DataCleanupPreview, DataCleanupResult,
    DownloadSource, GenericClientSummary, LocalModelState, LocalModelSummary, ModelDownloadState,
    ModelLibrary, ModelOwnership, ModelRemovalKind, ModelSource, RetentionSettingsDraft,
    UsageDashboard, UsageRequestSummary, UsageTotals,
};
use rusqlite::{Connection, Transaction, params};
use rusqlite_migration::{M, Migrations};
use serde_json::json;
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
    pub api_root: String,
    pub auth_style: String,
    pub credential_id: Option<String>,
    pub enabled: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredModelRouteRecord {
    pub alias: String,
    pub backend_id: String,
    pub resolved_model: String,
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
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection.execute(
            "INSERT INTO backends (
                id, display_name, kind, api_root, auth_style, credential_id,
                enabled, created_at_ms, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
                display_name = excluded.display_name,
                kind = excluded.kind,
                api_root = excluded.api_root,
                auth_style = excluded.auth_style,
                credential_id = excluded.credential_id,
                enabled = excluded.enabled,
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
            ],
        )?;
        Ok(())
    }

    pub fn backends(&self) -> Result<Vec<StoredBackendRecord>, DatabaseError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT id, display_name, kind, api_root, auth_style, credential_id,
                    enabled, created_at_ms, updated_at_ms
             FROM backends ORDER BY display_name COLLATE NOCASE, id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(StoredBackendRecord {
                id: row.get(0)?,
                display_name: row.get(1)?,
                kind: row.get(2)?,
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
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        let result = connection.query_row(
            "SELECT value_json FROM settings WHERE key = 'gateway.active_backend_id'",
            [],
            |row| row.get::<_, String>(0),
        );
        match result {
            Ok(value) => serde_json::from_str::<Option<String>>(&value).map_err(|_| {
                DatabaseError::InvalidData(
                    "gateway.active_backend_id is not a string or null".to_owned(),
                )
            }),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn set_active_backend_id(
        &self,
        backend_id: Option<&str>,
        now_ms: i64,
    ) -> Result<(), DatabaseError> {
        let value = serde_json::to_string(&backend_id).map_err(|_| {
            DatabaseError::InvalidData("gateway active backend could not be encoded".to_owned())
        })?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| DatabaseError::LockPoisoned)?;
        connection.execute(
            "INSERT INTO settings (key, value_json, updated_at)
             VALUES ('gateway.active_backend_id', ?1, ?2)
             ON CONFLICT(key) DO UPDATE SET
                value_json = excluded.value_json,
                updated_at = excluded.updated_at",
            params![value, now_ms.to_string()],
        )?;
        Ok(())
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
        upsert_setting(
            &transaction,
            "desktop.launch_at_login_asked",
            "true",
            now_ms,
        )?;
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

    pub fn upsert_integration_and_credential(
        &self,
        integration: &ManagedIntegrationRecord,
        credential: &StoredClientCredential,
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
        transaction.commit()?;
        Ok(())
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

    pub fn usage_dashboard(&self, limit: u32) -> Result<UsageDashboard, DatabaseError> {
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
        Ok(UsageDashboard {
            totals,
            recent_requests: requests.collect::<Result<Vec<_>, _>>()?,
        })
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
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
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
    ])
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

        assert_eq!(database.schema_version().expect("schema version"), 7);
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
            (true, 5, true)
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
                .usage_dashboard(50)
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
            .set_active_backend_id(None, 1_700_000_000_003)
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

        let dashboard = database.usage_dashboard(50).expect("usage dashboard");
        assert_eq!(dashboard.totals.request_count, 2);
        assert_eq!(dashboard.totals.input_tokens, 14);
        assert_eq!(dashboard.totals.cached_tokens, 3);
        assert_eq!(dashboard.totals.output_tokens, 9);
        assert_eq!(dashboard.totals.total_tokens, 23);
        assert_eq!(dashboard.recent_requests.len(), 2);
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
        let dashboard = database.usage_dashboard(50).expect("scale dashboard");
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
