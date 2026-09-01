use std::sync::Arc;

use crate::{
    Database, DatabaseError, StoredRuntimeProfileRecord, StoredRuntimeProfileVerification,
};

/// Persistence boundary for runtime-profile application services.
///
/// Keeping this adapter separate from `RuntimeProfileManager` prevents catalog,
/// verification, and activation policy from growing new SQL responsibilities.
#[derive(Clone)]
pub struct RuntimeProfileRepository {
    database: Arc<Database>,
}

impl RuntimeProfileRepository {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    pub fn list(&self) -> Result<Vec<StoredRuntimeProfileRecord>, DatabaseError> {
        self.database.runtime_profiles()
    }

    pub fn get(&self, id: &str) -> Result<Option<StoredRuntimeProfileRecord>, DatabaseError> {
        self.database.runtime_profile(id)
    }

    pub fn insert(&self, profile: &StoredRuntimeProfileRecord) -> Result<(), DatabaseError> {
        self.database.insert_runtime_profile(profile)
    }

    pub fn update_metadata(
        &self,
        id: &str,
        name: &str,
        description: &str,
        now_ms: i64,
    ) -> Result<bool, DatabaseError> {
        self.database
            .update_runtime_profile_metadata(id, name, description, now_ms)
    }

    pub fn mark_activated(
        &self,
        id: &str,
        verification: &StoredRuntimeProfileVerification,
        now_ms: i64,
    ) -> Result<bool, DatabaseError> {
        self.database
            .mark_runtime_profile_activated(id, verification, now_ms)
    }

    pub fn reverify(
        &self,
        id: &str,
        verification: &StoredRuntimeProfileVerification,
        now_ms: i64,
    ) -> Result<bool, DatabaseError> {
        self.database
            .reverify_runtime_profile(id, verification, now_ms)
    }

    pub fn delete(&self, id: &str, name: &str, now_ms: i64) -> Result<bool, DatabaseError> {
        self.database.delete_runtime_profile(id, name, now_ms)
    }
}
