use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use hal100_core::{
    AGENT_TASK_GRAPH_MAX_DEPENDENCIES, AGENT_TASK_GRAPH_MAX_NODES, AgentTaskCompletionEffect,
    AgentTaskGraph, AgentTaskGraphCheckpoint as CoreGraphCheckpoint, AgentTaskGraphDefinition,
    AgentTaskGraphError, AgentTaskGraphNodeCheckpoint as CoreNodeCheckpoint, AgentTaskGraphNodeId,
    AgentTaskGraphNodeState as CoreNodeState, AgentTaskGraphState as CoreGraphState, AgentTaskSpec,
};
use hal100_infra::{atomic_write_managed_file, read_managed_file};
use hal100_protocol::{
    AGENT_TASK_GRAPH_CHECKPOINT_SCHEMA_VERSION, AgentTaskEvidenceSource, AgentTaskGraphCheckpoint,
    AgentTaskGraphCheckpointState, AgentTaskGraphNodeCheckpoint, AgentTaskGraphNodeCheckpointState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AgentTaskGraphRuntimeError {
    StateUnavailable,
    GraphUnavailable,
    GraphAlreadyActive,
    CheckpointStorage,
    Graph(AgentTaskGraphError),
}

#[derive(Debug, Clone)]
struct AgentTaskGraphRecord {
    graph: AgentTaskGraph,
    updated_at_ms: i64,
}

const MAX_PERSISTED_GRAPH_CHECKPOINT_BYTES: u64 = 16 * 1024;

#[derive(Clone)]
struct AgentTaskGraphCheckpointStore {
    path: Arc<PathBuf>,
    recoverable: Arc<Mutex<Option<AgentTaskGraphCheckpoint>>>,
}

impl AgentTaskGraphCheckpointStore {
    fn open(path: PathBuf) -> Self {
        let checkpoint = if path.exists() {
            read_managed_file(&path, MAX_PERSISTED_GRAPH_CHECKPOINT_BYTES)
                .map_err(|_| AgentTaskGraphRuntimeError::CheckpointStorage)
                .and_then(|bytes| {
                    serde_json::from_slice(&bytes)
                        .map_err(|_| AgentTaskGraphRuntimeError::CheckpointStorage)
                })
                .and_then(|checkpoint| {
                    validate_persisted_checkpoint(&checkpoint)?;
                    Ok(checkpoint)
                })
                .map(Some)
                .unwrap_or_else(|_| {
                    tracing::warn!(
                        error_code = "agent_task_graph_checkpoint_rejected",
                        "agent_task_graph_checkpoint_rejected"
                    );
                    None
                })
        } else {
            None
        };
        Self {
            path: Arc::new(path),
            recoverable: Arc::new(Mutex::new(checkpoint)),
        }
    }

    fn persist(
        &self,
        checkpoint: &AgentTaskGraphCheckpoint,
    ) -> Result<(), AgentTaskGraphRuntimeError> {
        if matches!(
            checkpoint.state,
            AgentTaskGraphCheckpointState::Succeeded | AgentTaskGraphCheckpointState::Compensated
        ) {
            match fs::remove_file(self.path.as_ref()) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(AgentTaskGraphRuntimeError::CheckpointStorage),
            }
            *self
                .recoverable
                .lock()
                .map_err(|_| AgentTaskGraphRuntimeError::StateUnavailable)? = None;
            return Ok(());
        }

        validate_persisted_checkpoint(checkpoint)?;
        let bytes = serde_json::to_vec(checkpoint)
            .map_err(|_| AgentTaskGraphRuntimeError::CheckpointStorage)?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_PERSISTED_GRAPH_CHECKPOINT_BYTES {
            return Err(AgentTaskGraphRuntimeError::CheckpointStorage);
        }
        atomic_write_managed_file(self.path.as_ref(), &bytes, 0o600)
            .map_err(|_| AgentTaskGraphRuntimeError::CheckpointStorage)?;
        *self
            .recoverable
            .lock()
            .map_err(|_| AgentTaskGraphRuntimeError::StateUnavailable)? = Some(checkpoint.clone());
        Ok(())
    }

    fn recoverable(&self) -> Result<Option<AgentTaskGraphCheckpoint>, AgentTaskGraphRuntimeError> {
        self.recoverable
            .lock()
            .map_err(|_| AgentTaskGraphRuntimeError::StateUnavailable)
            .map(|checkpoint| checkpoint.clone())
    }
}

#[derive(Clone, Default)]
pub(super) struct AgentTaskGraphRuntime {
    current: Arc<Mutex<Option<AgentTaskGraphRecord>>>,
    checkpoint_store: Option<AgentTaskGraphCheckpointStore>,
}

impl AgentTaskGraphRuntime {
    pub(super) fn persistent(
        checkpoint_path: impl AsRef<Path>,
    ) -> Result<Self, AgentTaskGraphRuntimeError> {
        Ok(Self {
            current: Arc::default(),
            checkpoint_store: Some(AgentTaskGraphCheckpointStore::open(
                checkpoint_path.as_ref().to_path_buf(),
            )),
        })
    }

    pub(super) fn recoverable_snapshot(
        &self,
    ) -> Result<Option<AgentTaskGraphCheckpoint>, AgentTaskGraphRuntimeError> {
        self.checkpoint_store
            .as_ref()
            .map(AgentTaskGraphCheckpointStore::recoverable)
            .transpose()
            .map(Option::flatten)
    }

    /// Installs a Rust-built graph without starting a model run or creating action authority.
    pub(super) fn begin(
        &self,
        definition: AgentTaskGraphDefinition,
        updated_at_ms: i64,
    ) -> Result<AgentTaskGraphCheckpoint, AgentTaskGraphRuntimeError> {
        let mut current = self
            .current
            .lock()
            .map_err(|_| AgentTaskGraphRuntimeError::StateUnavailable)?;
        if current.as_ref().is_some_and(|record| {
            matches!(
                record.graph.state(),
                CoreGraphState::Active | CoreGraphState::Compensating
            )
        }) {
            return Err(AgentTaskGraphRuntimeError::GraphAlreadyActive);
        }
        let record = AgentTaskGraphRecord {
            graph: AgentTaskGraph::new(definition),
            updated_at_ms,
        };
        let checkpoint = checkpoint(&record);
        self.persist_record(&record)?;
        *current = Some(record);
        Ok(checkpoint)
    }

    /// Restores only after the caller supplies a new exact Rust definition. The persisted object
    /// validates semantic shape and sequence but contributes no target, plan, run or confirmation
    /// authority; Core resets every node to dependency-derived reality revalidation.
    pub(super) fn restore_for_revalidation(
        &self,
        definition: AgentTaskGraphDefinition,
        updated_at_ms: i64,
    ) -> Result<AgentTaskGraphCheckpoint, AgentTaskGraphRuntimeError> {
        let mut current = self
            .current
            .lock()
            .map_err(|_| AgentTaskGraphRuntimeError::StateUnavailable)?;
        if current.as_ref().is_some_and(|record| {
            matches!(
                record.graph.state(),
                CoreGraphState::Active | CoreGraphState::Compensating
            )
        }) {
            return Err(AgentTaskGraphRuntimeError::GraphAlreadyActive);
        }
        let persisted = self
            .recoverable_snapshot()?
            .ok_or(AgentTaskGraphRuntimeError::GraphUnavailable)?;
        let core_checkpoint = core_checkpoint_from_protocol(&definition, &persisted)?;
        let graph = AgentTaskGraph::restore_for_revalidation(definition, &core_checkpoint)
            .map_err(AgentTaskGraphRuntimeError::Graph)?;
        let record = AgentTaskGraphRecord {
            graph,
            updated_at_ms,
        };
        let restored = checkpoint(&record);
        self.persist_record(&record)?;
        *current = Some(record);
        Ok(restored)
    }

    /// Returns the existing bounded-replan node or starts the next ready node. It never advances
    /// a node that is waiting for native confirmation.
    pub(super) fn task_for_next_run(
        &self,
        updated_at_ms: i64,
    ) -> Result<AgentTaskSpec, AgentTaskGraphRuntimeError> {
        let mut current = self
            .current
            .lock()
            .map_err(|_| AgentTaskGraphRuntimeError::StateUnavailable)?;
        let record = current
            .as_mut()
            .ok_or(AgentTaskGraphRuntimeError::GraphUnavailable)?;
        if let Some((_, state, task)) = record.graph.active_node() {
            return if state == CoreNodeState::Running {
                Ok(task.clone())
            } else {
                Err(AgentTaskGraphRuntimeError::Graph(
                    AgentTaskGraphError::InvalidTransition,
                ))
            };
        }
        let (node_id, task) = record
            .graph
            .next_ready()
            .map(|(node_id, task)| (node_id, task.clone()))
            .ok_or(AgentTaskGraphRuntimeError::Graph(
                AgentTaskGraphError::InvalidTransition,
            ))?;
        record
            .graph
            .start(node_id)
            .map_err(AgentTaskGraphRuntimeError::Graph)?;
        record.updated_at_ms = updated_at_ms;
        self.persist_record(record)?;
        Ok(task)
    }

    /// Explicitly starts the next safe inverse task in Rust-defined reverse order. Merely failing
    /// or cancelling a graph never calls this method, so compensation cannot run automatically.
    pub(super) fn task_for_next_compensation(
        &self,
        updated_at_ms: i64,
    ) -> Result<AgentTaskSpec, AgentTaskGraphRuntimeError> {
        let mut current = self
            .current
            .lock()
            .map_err(|_| AgentTaskGraphRuntimeError::StateUnavailable)?;
        let record = current
            .as_mut()
            .ok_or(AgentTaskGraphRuntimeError::GraphUnavailable)?;
        if let Some((_, task)) = record.graph.active_compensation() {
            return Ok(task.clone());
        }
        let (_, task) = record
            .graph
            .begin_next_compensation()
            .map_err(AgentTaskGraphRuntimeError::Graph)?;
        record.updated_at_ms = updated_at_ms;
        self.persist_record(record)?;
        Ok(task)
    }

    pub(super) fn await_active_confirmation(
        &self,
        updated_at_ms: i64,
    ) -> Result<(), AgentTaskGraphRuntimeError> {
        self.with_active_graph(updated_at_ms, |graph, node_id, state| {
            match state {
                CoreNodeState::Running => graph.await_confirmation(node_id),
                // The single-task runtime owns the exact plan and confirmation phase. The graph
                // deliberately stores no plan ID and remains `compensating` until Rust evidence.
                CoreNodeState::Compensating => Ok(()),
                _ => Err(AgentTaskGraphError::InvalidTransition),
            }
        })
    }

    pub(super) fn confirm_active_if_any(
        &self,
        updated_at_ms: i64,
    ) -> Result<bool, AgentTaskGraphRuntimeError> {
        let mut current = self
            .current
            .lock()
            .map_err(|_| AgentTaskGraphRuntimeError::StateUnavailable)?;
        let Some(record) = current.as_mut() else {
            return Ok(false);
        };
        let Some((node_id, state, _)) = record.graph.active_node() else {
            return Ok(false);
        };
        match state {
            CoreNodeState::AwaitingConfirmation => record
                .graph
                .confirm(node_id)
                .map_err(AgentTaskGraphRuntimeError::Graph)?,
            CoreNodeState::Compensating => {}
            _ => return Ok(false),
        }
        record.updated_at_ms = updated_at_ms;
        self.persist_record(record)?;
        Ok(true)
    }

    pub(super) fn complete_active(
        &self,
        evidence_source: AgentTaskEvidenceSource,
        completion_effect: AgentTaskCompletionEffect,
        updated_at_ms: i64,
    ) -> Result<(), AgentTaskGraphRuntimeError> {
        self.with_active_graph(updated_at_ms, |graph, node_id, state| match state {
            CoreNodeState::Running => graph.succeed(node_id, evidence_source, completion_effect),
            CoreNodeState::Compensating => graph.complete_compensation(node_id, evidence_source),
            _ => Err(AgentTaskGraphError::InvalidTransition),
        })
    }

    pub(super) fn fail_active_if_any(
        &self,
        updated_at_ms: i64,
    ) -> Result<bool, AgentTaskGraphRuntimeError> {
        let mut current = self
            .current
            .lock()
            .map_err(|_| AgentTaskGraphRuntimeError::StateUnavailable)?;
        let Some(record) = current.as_mut() else {
            return Ok(false);
        };
        let Some((node_id, state, _)) = record.graph.active_node() else {
            return Ok(false);
        };
        match state {
            CoreNodeState::Running | CoreNodeState::AwaitingConfirmation => record
                .graph
                .fail(node_id)
                .map_err(AgentTaskGraphRuntimeError::Graph)?,
            CoreNodeState::Compensating => record
                .graph
                .fail_compensation(node_id)
                .map_err(AgentTaskGraphRuntimeError::Graph)?,
            _ => return Ok(false),
        }
        record.updated_at_ms = updated_at_ms;
        self.persist_record(record)?;
        Ok(true)
    }

    pub(super) fn cancel_if_active(
        &self,
        updated_at_ms: i64,
    ) -> Result<bool, AgentTaskGraphRuntimeError> {
        let mut current = self
            .current
            .lock()
            .map_err(|_| AgentTaskGraphRuntimeError::StateUnavailable)?;
        let Some(record) = current.as_mut() else {
            return Ok(false);
        };
        match record.graph.state() {
            CoreGraphState::Active => record
                .graph
                .cancel()
                .map_err(AgentTaskGraphRuntimeError::Graph)?,
            CoreGraphState::Compensating => {
                let (node_id, state, _) =
                    record
                        .graph
                        .active_node()
                        .ok_or(AgentTaskGraphRuntimeError::Graph(
                            AgentTaskGraphError::InvalidTransition,
                        ))?;
                if state != CoreNodeState::Compensating {
                    return Err(AgentTaskGraphRuntimeError::Graph(
                        AgentTaskGraphError::InvalidTransition,
                    ));
                }
                record
                    .graph
                    .fail_compensation(node_id)
                    .map_err(AgentTaskGraphRuntimeError::Graph)?;
            }
            _ => return Ok(false),
        }
        record.updated_at_ms = updated_at_ms;
        self.persist_record(record)?;
        Ok(true)
    }

    pub(super) fn snapshot(
        &self,
    ) -> Result<Option<AgentTaskGraphCheckpoint>, AgentTaskGraphRuntimeError> {
        let current = self
            .current
            .lock()
            .map_err(|_| AgentTaskGraphRuntimeError::StateUnavailable)?;
        Ok(current.as_ref().map(checkpoint))
    }

    fn with_active_graph(
        &self,
        updated_at_ms: i64,
        update: impl FnOnce(
            &mut AgentTaskGraph,
            hal100_core::AgentTaskGraphNodeId,
            CoreNodeState,
        ) -> Result<(), AgentTaskGraphError>,
    ) -> Result<(), AgentTaskGraphRuntimeError> {
        let mut current = self
            .current
            .lock()
            .map_err(|_| AgentTaskGraphRuntimeError::StateUnavailable)?;
        let record = current
            .as_mut()
            .ok_or(AgentTaskGraphRuntimeError::GraphUnavailable)?;
        let (node_id, state, _) =
            record
                .graph
                .active_node()
                .ok_or(AgentTaskGraphRuntimeError::Graph(
                    AgentTaskGraphError::InvalidTransition,
                ))?;
        update(&mut record.graph, node_id, state).map_err(AgentTaskGraphRuntimeError::Graph)?;
        record.updated_at_ms = updated_at_ms;
        self.persist_record(record)?;
        Ok(())
    }

    fn persist_record(
        &self,
        record: &AgentTaskGraphRecord,
    ) -> Result<(), AgentTaskGraphRuntimeError> {
        self.checkpoint_store
            .as_ref()
            .map(|store| store.persist(&checkpoint(record)))
            .transpose()
            .map(|_| ())
    }

    #[cfg(test)]
    fn update(
        &self,
        updated_at_ms: i64,
        update: impl FnOnce(&mut AgentTaskGraph),
    ) -> Result<(), AgentTaskGraphRuntimeError> {
        let mut current = self
            .current
            .lock()
            .map_err(|_| AgentTaskGraphRuntimeError::StateUnavailable)?;
        let record = current
            .as_mut()
            .ok_or(AgentTaskGraphRuntimeError::StateUnavailable)?;
        update(&mut record.graph);
        record.updated_at_ms = updated_at_ms;
        Ok(())
    }
}

fn validate_persisted_checkpoint(
    checkpoint: &AgentTaskGraphCheckpoint,
) -> Result<(), AgentTaskGraphRuntimeError> {
    if checkpoint.schema_version != AGENT_TASK_GRAPH_CHECKPOINT_SCHEMA_VERSION
        || checkpoint.nodes.is_empty()
        || checkpoint.nodes.len() > AGENT_TASK_GRAPH_MAX_NODES
    {
        return Err(AgentTaskGraphRuntimeError::CheckpointStorage);
    }
    for (index, node) in checkpoint.nodes.iter().enumerate() {
        if node.node_index as usize != index
            || node.dependency_indexes.len() > AGENT_TASK_GRAPH_MAX_DEPENDENCIES
            || node.task_kind.is_empty()
            || node.task_kind.len() > 64
            || node.target_kind.is_empty()
            || node.target_kind.len() > 64
            || node.success_predicate.is_empty()
            || node.success_predicate.len() > 64
            || node
                .dependency_indexes
                .iter()
                .any(|dependency| *dependency as usize >= index)
            || node
                .dependency_indexes
                .iter()
                .enumerate()
                .any(|(dependency_index, dependency)| {
                    node.dependency_indexes[..dependency_index].contains(dependency)
                })
            || node.requires_reauthorization
                != matches!(
                    node.state,
                    AgentTaskGraphNodeCheckpointState::Running
                        | AgentTaskGraphNodeCheckpointState::AwaitingConfirmation
                        | AgentTaskGraphNodeCheckpointState::Compensating
                )
        {
            return Err(AgentTaskGraphRuntimeError::CheckpointStorage);
        }
    }
    let ready = checkpoint
        .nodes
        .iter()
        .filter(|node| node.state == AgentTaskGraphNodeCheckpointState::Ready)
        .count();
    let succeeded = checkpoint
        .nodes
        .iter()
        .filter(|node| {
            matches!(
                node.state,
                AgentTaskGraphNodeCheckpointState::Succeeded
                    | AgentTaskGraphNodeCheckpointState::Compensated
            )
        })
        .count();
    if usize::from(checkpoint.ready_node_count) != ready
        || usize::from(checkpoint.succeeded_node_count) != succeeded
    {
        return Err(AgentTaskGraphRuntimeError::CheckpointStorage);
    }
    Ok(())
}

fn core_checkpoint_from_protocol(
    definition: &AgentTaskGraphDefinition,
    checkpoint: &AgentTaskGraphCheckpoint,
) -> Result<CoreGraphCheckpoint, AgentTaskGraphRuntimeError> {
    validate_persisted_checkpoint(checkpoint)?;
    if definition.nodes().len() != checkpoint.nodes.len() {
        return Err(AgentTaskGraphRuntimeError::Graph(
            AgentTaskGraphError::CheckpointMismatch,
        ));
    }
    let mut nodes = Vec::with_capacity(checkpoint.nodes.len());
    for (index, (definition_node, persisted_node)) in
        definition.nodes().iter().zip(&checkpoint.nodes).enumerate()
    {
        let expected_dependencies = definition_node
            .dependencies()
            .iter()
            .map(|dependency| u8::try_from(dependency.index()).unwrap_or(u8::MAX))
            .collect::<Vec<_>>();
        if persisted_node.node_index as usize != index
            || persisted_node.task_kind != definition_node.task().task_kind().key()
            || persisted_node.target_kind != definition_node.task().target().kind().key()
            || persisted_node.success_predicate != definition_node.task().success_predicate().key()
            || persisted_node.dependency_indexes != expected_dependencies
        {
            return Err(AgentTaskGraphRuntimeError::Graph(
                AgentTaskGraphError::CheckpointMismatch,
            ));
        }
        nodes.push(CoreNodeCheckpoint {
            node_id: AgentTaskGraphNodeId::from_index(persisted_node.node_index),
            state: core_node_state(persisted_node.state),
            task_kind: definition_node.task().task_kind(),
            target_kind: definition_node.task().target().kind(),
            success_predicate: definition_node.task().success_predicate(),
            dependencies: definition_node.dependencies().to_vec(),
            evidence_source: persisted_node.evidence_source,
            changed_owned_state: persisted_node.changed_owned_state,
            requires_reauthorization: persisted_node.requires_reauthorization,
        });
    }
    Ok(CoreGraphCheckpoint {
        schema_version: checkpoint.schema_version,
        checkpoint_sequence: checkpoint.checkpoint_sequence,
        state: core_graph_state(checkpoint.state),
        nodes,
    })
}

const fn core_graph_state(state: AgentTaskGraphCheckpointState) -> CoreGraphState {
    match state {
        AgentTaskGraphCheckpointState::Active => CoreGraphState::Active,
        AgentTaskGraphCheckpointState::Succeeded => CoreGraphState::Succeeded,
        AgentTaskGraphCheckpointState::Failed => CoreGraphState::Failed,
        AgentTaskGraphCheckpointState::Compensating => CoreGraphState::Compensating,
        AgentTaskGraphCheckpointState::Compensated => CoreGraphState::Compensated,
        AgentTaskGraphCheckpointState::Cancelled => CoreGraphState::Cancelled,
    }
}

const fn core_node_state(state: AgentTaskGraphNodeCheckpointState) -> CoreNodeState {
    match state {
        AgentTaskGraphNodeCheckpointState::Blocked => CoreNodeState::Blocked,
        AgentTaskGraphNodeCheckpointState::Ready => CoreNodeState::Ready,
        AgentTaskGraphNodeCheckpointState::Running => CoreNodeState::Running,
        AgentTaskGraphNodeCheckpointState::AwaitingConfirmation => {
            CoreNodeState::AwaitingConfirmation
        }
        AgentTaskGraphNodeCheckpointState::Succeeded => CoreNodeState::Succeeded,
        AgentTaskGraphNodeCheckpointState::Failed => CoreNodeState::Failed,
        AgentTaskGraphNodeCheckpointState::Compensating => CoreNodeState::Compensating,
        AgentTaskGraphNodeCheckpointState::Compensated => CoreNodeState::Compensated,
        AgentTaskGraphNodeCheckpointState::Cancelled => CoreNodeState::Cancelled,
    }
}

fn checkpoint(record: &AgentTaskGraphRecord) -> AgentTaskGraphCheckpoint {
    let core = record.graph.checkpoint();
    AgentTaskGraphCheckpoint {
        schema_version: AGENT_TASK_GRAPH_CHECKPOINT_SCHEMA_VERSION,
        checkpoint_sequence: core.checkpoint_sequence,
        state: graph_state(core.state),
        ready_node_count: bounded_count(&core, |state| state == CoreNodeState::Ready),
        succeeded_node_count: bounded_count(&core, |state| {
            matches!(state, CoreNodeState::Succeeded | CoreNodeState::Compensated)
        }),
        nodes: core
            .nodes
            .into_iter()
            .map(|node| AgentTaskGraphNodeCheckpoint {
                node_index: u8::try_from(node.node_id.index()).unwrap_or(u8::MAX),
                state: node_state(node.state),
                task_kind: node.task_kind.key().to_owned(),
                target_kind: node.target_kind.key().to_owned(),
                success_predicate: node.success_predicate.key().to_owned(),
                dependency_indexes: node
                    .dependencies
                    .into_iter()
                    .map(|dependency| u8::try_from(dependency.index()).unwrap_or(u8::MAX))
                    .collect(),
                evidence_source: node.evidence_source,
                changed_owned_state: node.changed_owned_state,
                requires_reauthorization: node.requires_reauthorization,
            })
            .collect(),
        updated_at_ms: record.updated_at_ms,
    }
}

fn bounded_count(
    checkpoint: &CoreGraphCheckpoint,
    predicate: impl Fn(CoreNodeState) -> bool,
) -> u8 {
    u8::try_from(
        checkpoint
            .nodes
            .iter()
            .filter(|node| predicate(node.state))
            .count(),
    )
    .unwrap_or(u8::MAX)
}

const fn graph_state(state: CoreGraphState) -> AgentTaskGraphCheckpointState {
    match state {
        CoreGraphState::Active => AgentTaskGraphCheckpointState::Active,
        CoreGraphState::Succeeded => AgentTaskGraphCheckpointState::Succeeded,
        CoreGraphState::Failed => AgentTaskGraphCheckpointState::Failed,
        CoreGraphState::Compensating => AgentTaskGraphCheckpointState::Compensating,
        CoreGraphState::Compensated => AgentTaskGraphCheckpointState::Compensated,
        CoreGraphState::Cancelled => AgentTaskGraphCheckpointState::Cancelled,
    }
}

const fn node_state(state: CoreNodeState) -> AgentTaskGraphNodeCheckpointState {
    match state {
        CoreNodeState::Blocked => AgentTaskGraphNodeCheckpointState::Blocked,
        CoreNodeState::Ready => AgentTaskGraphNodeCheckpointState::Ready,
        CoreNodeState::Running => AgentTaskGraphNodeCheckpointState::Running,
        CoreNodeState::AwaitingConfirmation => {
            AgentTaskGraphNodeCheckpointState::AwaitingConfirmation
        }
        CoreNodeState::Succeeded => AgentTaskGraphNodeCheckpointState::Succeeded,
        CoreNodeState::Failed => AgentTaskGraphNodeCheckpointState::Failed,
        CoreNodeState::Compensating => AgentTaskGraphNodeCheckpointState::Compensating,
        CoreNodeState::Compensated => AgentTaskGraphNodeCheckpointState::Compensated,
        CoreNodeState::Cancelled => AgentTaskGraphNodeCheckpointState::Cancelled,
    }
}

#[cfg(test)]
mod tests {
    use hal100_core::{AgentTaskGraphDefinition, AgentTaskGraphNodeId, AgentTaskProviderMode};

    use super::*;

    #[test]
    fn managed_pi_graph_checkpoint_is_bounded_and_redacted() {
        let runtime = AgentTaskGraphRuntime::default();
        let checkpoint = runtime
            .begin(
                AgentTaskGraphDefinition::prepare_managed_pi(
                    "secret-model-resource".into(),
                    AgentTaskProviderMode::Local,
                )
                .expect("managed Pi graph"),
                1_755_000_000_000,
            )
            .expect("begin graph");
        assert_eq!(checkpoint.schema_version, 1);
        assert_eq!(checkpoint.nodes.len(), 4);
        assert_eq!(checkpoint.ready_node_count, 1);
        assert_eq!(checkpoint.succeeded_node_count, 0);
        assert_eq!(checkpoint.nodes[3].dependency_indexes, vec![1, 2]);

        runtime
            .update(1_755_000_000_001, |graph| {
                let node = AgentTaskGraphNodeId::from_index(0);
                graph.start(node).expect("start root");
                graph.await_confirmation(node).expect("await confirmation");
            })
            .expect("update graph");
        let checkpoint = runtime.snapshot().expect("snapshot").expect("graph");
        assert!(checkpoint.nodes[0].requires_reauthorization);
        let rendered = serde_json::to_string(&checkpoint).expect("checkpoint JSON");
        assert!(!rendered.contains("secret-model-resource"));
        for forbidden in ["planId", "runId", "prompt", "answer", "credential", "path"] {
            assert!(!rendered.contains(forbidden), "leaked {forbidden}");
        }
    }

    #[test]
    fn a_second_active_graph_is_rejected_without_replacing_the_first() {
        let runtime = AgentTaskGraphRuntime::default();
        let definition = || {
            AgentTaskGraphDefinition::prepare_managed_pi(
                "model".into(),
                AgentTaskProviderMode::Local,
            )
            .expect("graph")
        };
        runtime.begin(definition(), 1).expect("first graph");
        assert_eq!(
            runtime.begin(definition(), 2),
            Err(AgentTaskGraphRuntimeError::GraphAlreadyActive)
        );
        assert_eq!(
            runtime
                .snapshot()
                .expect("snapshot")
                .expect("graph")
                .updated_at_ms,
            1
        );
    }

    #[test]
    fn runtime_advances_one_node_and_never_skips_native_confirmation() {
        let runtime = AgentTaskGraphRuntime::default();
        runtime
            .begin(
                AgentTaskGraphDefinition::prepare_managed_pi(
                    "model".into(),
                    AgentTaskProviderMode::Local,
                )
                .expect("graph"),
                1,
            )
            .expect("begin");
        let task = runtime.task_for_next_run(2).expect("engine task");
        assert_eq!(task.task_kind(), hal100_core::AgentTaskKind::InstallEngine);
        runtime
            .await_active_confirmation(3)
            .expect("await confirmation");
        assert_eq!(
            runtime.task_for_next_run(4),
            Err(AgentTaskGraphRuntimeError::Graph(
                AgentTaskGraphError::InvalidTransition
            ))
        );
        assert!(runtime.confirm_active_if_any(5).expect("confirm"));
        runtime
            .complete_active(
                AgentTaskEvidenceSource::EngineRecheck,
                AgentTaskCompletionEffect::ChangedOwnedState,
                6,
            )
            .expect("complete engine");
        let task = runtime.task_for_next_run(7).expect("model task");
        assert_eq!(task.task_kind(), hal100_core::AgentTaskKind::StartModel);
    }

    #[test]
    fn compensation_is_explicit_reverse_ordered_and_reauthorized() {
        let runtime = AgentTaskGraphRuntime::default();
        runtime
            .begin(
                AgentTaskGraphDefinition::prepare_managed_pi(
                    "model".into(),
                    AgentTaskProviderMode::Local,
                )
                .expect("graph"),
                1,
            )
            .expect("begin");
        assert_eq!(
            runtime
                .task_for_next_run(2)
                .expect("start engine node")
                .task_kind(),
            hal100_core::AgentTaskKind::InstallEngine
        );
        runtime
            .await_active_confirmation(3)
            .expect("await forward confirmation");
        assert!(runtime.confirm_active_if_any(4).expect("confirm forward"));
        runtime
            .complete_active(
                AgentTaskEvidenceSource::EngineRecheck,
                AgentTaskCompletionEffect::ChangedOwnedState,
                5,
            )
            .expect("complete changed engine node");
        runtime.task_for_next_run(6).expect("start model node");
        assert!(runtime.fail_active_if_any(7).expect("fail model node"));

        let compensation = runtime
            .task_for_next_compensation(8)
            .expect("explicit compensation");
        assert_eq!(
            compensation.task_kind(),
            hal100_core::AgentTaskKind::RemoveEngine
        );
        let checkpoint = runtime.snapshot().expect("snapshot").expect("graph");
        assert_eq!(
            checkpoint.state,
            AgentTaskGraphCheckpointState::Compensating
        );
        assert!(checkpoint.nodes[0].requires_reauthorization);

        runtime
            .await_active_confirmation(9)
            .expect("single-task plan now awaits native confirmation");
        assert!(
            runtime
                .confirm_active_if_any(10)
                .expect("confirm compensation")
        );
        runtime
            .complete_active(
                AgentTaskEvidenceSource::EngineRecheck,
                AgentTaskCompletionEffect::Observed,
                11,
            )
            .expect("complete compensation from reality evidence");
        let checkpoint = runtime.snapshot().expect("snapshot").expect("graph");
        assert_eq!(checkpoint.state, AgentTaskGraphCheckpointState::Compensated);
        assert_eq!(
            checkpoint.nodes[0].state,
            AgentTaskGraphNodeCheckpointState::Compensated
        );
        assert!(matches!(
            runtime.task_for_next_compensation(12),
            Err(AgentTaskGraphRuntimeError::Graph(
                AgentTaskGraphError::CompensationUnavailable
            ))
        ));
    }

    #[test]
    fn persisted_checkpoint_restores_only_shape_after_exact_user_rebinding() {
        let directory = std::env::temp_dir().join(format!(
            "hal100-agent-graph-recovery-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&directory).expect("create recovery directory");
        let path = directory.join("checkpoint.json");
        let runtime = AgentTaskGraphRuntime::persistent(&path).expect("persistent runtime");
        runtime
            .begin(
                AgentTaskGraphDefinition::prepare_external_agent(
                    "old-secret-model-id".into(),
                    hal100_core::ExternalAgentIntegrationId::OpenCode,
                    AgentTaskProviderMode::Local,
                )
                .expect("old graph"),
                100,
            )
            .expect("begin persisted graph");
        runtime.task_for_next_run(101).expect("start root");
        runtime
            .await_active_confirmation(102)
            .expect("persist lost confirmation boundary");
        let before_restart = runtime
            .snapshot()
            .expect("snapshot")
            .expect("current graph");
        drop(runtime);

        let rendered = fs::read_to_string(&path).expect("read persisted checkpoint");
        for forbidden in [
            "old-secret-model-id",
            "opencode",
            "planId",
            "runId",
            "prompt",
            "answer",
            "credential",
            "path",
        ] {
            assert!(!rendered.contains(forbidden), "persisted {forbidden}");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path)
                    .expect("checkpoint metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }

        let restarted = AgentTaskGraphRuntime::persistent(&path).expect("restart runtime");
        assert!(restarted.snapshot().expect("snapshot").is_none());
        let recoverable = restarted
            .recoverable_snapshot()
            .expect("recoverable snapshot")
            .expect("persisted graph shape");
        assert_eq!(recoverable, before_restart);
        assert!(matches!(
            restarted.restore_for_revalidation(
                AgentTaskGraphDefinition::prepare_managed_pi(
                    "new-model".into(),
                    AgentTaskProviderMode::Local,
                )
                .expect("wrong shape"),
                200,
            ),
            Err(AgentTaskGraphRuntimeError::Graph(
                AgentTaskGraphError::CheckpointMismatch
            ))
        ));

        let restored = restarted
            .restore_for_revalidation(
                AgentTaskGraphDefinition::prepare_external_agent(
                    "new-user-selected-model".into(),
                    hal100_core::ExternalAgentIntegrationId::OpenClaw,
                    AgentTaskProviderMode::Local,
                )
                .expect("new exact graph"),
                201,
            )
            .expect("restore for full reality revalidation");
        assert!(restored.checkpoint_sequence > before_restart.checkpoint_sequence);
        assert_eq!(restored.state, AgentTaskGraphCheckpointState::Active);
        assert_eq!(
            restored.nodes[0].state,
            AgentTaskGraphNodeCheckpointState::Ready
        );
        assert!(
            restored.nodes[1..]
                .iter()
                .all(|node| node.state == AgentTaskGraphNodeCheckpointState::Blocked)
        );
        assert!(restored.nodes.iter().all(|node| {
            node.evidence_source.is_none()
                && !node.changed_owned_state
                && !node.requires_reauthorization
        }));
        let rendered = fs::read_to_string(&path).expect("read rebound checkpoint");
        assert!(!rendered.contains("new-user-selected-model"));
        assert!(!rendered.contains("openclaw"));
        for (source, effect) in [
            (
                AgentTaskEvidenceSource::EngineRecheck,
                AgentTaskCompletionEffect::AlreadySatisfied,
            ),
            (
                AgentTaskEvidenceSource::RuntimeRecheck,
                AgentTaskCompletionEffect::AlreadySatisfied,
            ),
            (
                AgentTaskEvidenceSource::IntegrationRecheck,
                AgentTaskCompletionEffect::AlreadySatisfied,
            ),
        ] {
            restarted
                .task_for_next_run(300)
                .expect("start revalidation node");
            restarted
                .complete_active(source, effect, 301)
                .expect("complete revalidation node");
        }
        assert_eq!(
            restarted
                .snapshot()
                .expect("terminal snapshot")
                .expect("terminal graph")
                .state,
            AgentTaskGraphCheckpointState::Succeeded
        );
        assert!(!path.exists(), "terminal success must clear recovery file");
        assert!(
            restarted
                .recoverable_snapshot()
                .expect("terminal recoverable snapshot")
                .is_none()
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn persisted_checkpoint_rejects_unknown_or_authorizing_fields() {
        let directory = std::env::temp_dir().join(format!(
            "hal100-agent-graph-tamper-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&directory).expect("create tamper directory");
        let path = directory.join("checkpoint.json");
        let runtime = AgentTaskGraphRuntime::persistent(&path).expect("persistent runtime");
        runtime
            .begin(
                AgentTaskGraphDefinition::prepare_managed_pi(
                    "model".into(),
                    AgentTaskProviderMode::Local,
                )
                .expect("graph"),
                1,
            )
            .expect("persist graph");
        drop(runtime);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).expect("read checkpoint"))
                .expect("checkpoint JSON");
        value
            .as_object_mut()
            .expect("checkpoint object")
            .insert("prompt".to_owned(), serde_json::json!("do not persist"));
        fs::write(&path, serde_json::to_vec(&value).expect("tampered JSON"))
            .expect("write tampered checkpoint");
        let rejected = AgentTaskGraphRuntime::persistent(&path)
            .expect("invalid recovery data does not disable the Agent runtime");
        assert!(rejected.snapshot().expect("current snapshot").is_none());
        assert!(
            rejected
                .recoverable_snapshot()
                .expect("rejected recovery snapshot")
                .is_none()
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn composite_recovery_contract_matches_runtime_limits_and_zero_authority() {
        let manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../contracts/agent-evals/v11-composite-recovery.json"
        ))
        .expect("recovery contract");
        assert_eq!(manifest["schemaVersion"], 1);
        assert_eq!(manifest["limits"]["maxCheckpointBytes"], 16_384);
        assert_eq!(manifest["limits"]["maxNodes"], AGENT_TASK_GRAPH_MAX_NODES);
        assert_eq!(
            manifest["limits"]["maxDependenciesPerNode"],
            AGENT_TASK_GRAPH_MAX_DEPENDENCIES
        );
        assert_eq!(
            manifest["scenarios"]
                .as_array()
                .expect("recovery scenarios")
                .len(),
            9
        );
        for value in manifest["thresholds"]
            .as_object()
            .expect("recovery thresholds")
            .values()
        {
            assert_eq!(value, 0);
        }
    }
}
