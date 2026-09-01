use std::sync::Arc;

use crate::{Database, DatabaseError, RuntimeActivationPhase, StoredRuntimeActivationJournal};

/// Durable rollback boundary for runtime-profile activation.
///
/// At most one row may exist. A row is removed only after activation commits or compensation has
/// restored the previous route/runtime. `RecoveryRequired` is deliberately not finishable through
/// this API, so a failed recovery remains visible after restart.
#[derive(Clone)]
pub struct RuntimeActivationJournalRepository {
    database: Arc<Database>,
}

impl RuntimeActivationJournalRepository {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    pub fn begin(&self, journal: &StoredRuntimeActivationJournal) -> Result<(), DatabaseError> {
        self.database.begin_runtime_activation(journal)
    }

    pub fn pending(&self) -> Result<Vec<StoredRuntimeActivationJournal>, DatabaseError> {
        self.database.runtime_activation_journals()
    }

    pub fn transition(
        &self,
        id: &str,
        expected: RuntimeActivationPhase,
        next: RuntimeActivationPhase,
        now_ms: i64,
    ) -> Result<bool, DatabaseError> {
        self.database
            .transition_runtime_activation(id, expected, next, now_ms)
    }

    pub fn finish(
        &self,
        id: &str,
        expected: RuntimeActivationPhase,
    ) -> Result<bool, DatabaseError> {
        self.database.finish_runtime_activation(id, expected)
    }
}
