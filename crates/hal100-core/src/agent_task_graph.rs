use hal100_protocol::AgentTaskEvidenceSource;

use crate::{
    AgentTaskKind, AgentTaskProviderMode, AgentTaskSpec, AgentTaskSpecError,
    AgentTaskSuccessPredicate, AgentTaskTarget, AgentTaskTargetKind, ExternalAgentIntegrationId,
};

pub const AGENT_TASK_GRAPH_SCHEMA_VERSION: u8 = 1;
pub const AGENT_TASK_GRAPH_MAX_NODES: usize = 8;
pub const AGENT_TASK_GRAPH_MAX_DEPENDENCIES: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AgentTaskGraphNodeId(u8);

impl AgentTaskGraphNodeId {
    pub const fn from_index(index: u8) -> Self {
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTaskGraphNodeDefinition {
    task: AgentTaskSpec,
    dependencies: Vec<AgentTaskGraphNodeId>,
    compensation: Option<AgentTaskSpec>,
}

impl AgentTaskGraphNodeDefinition {
    pub fn new(task: AgentTaskSpec, dependencies: Vec<AgentTaskGraphNodeId>) -> Self {
        Self {
            task,
            dependencies,
            compensation: None,
        }
    }

    pub fn with_compensation(mut self, compensation: AgentTaskSpec) -> Self {
        self.compensation = Some(compensation);
        self
    }

    pub const fn task(&self) -> &AgentTaskSpec {
        &self.task
    }

    pub fn dependencies(&self) -> &[AgentTaskGraphNodeId] {
        &self.dependencies
    }

    pub const fn compensation(&self) -> Option<&AgentTaskSpec> {
        self.compensation.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTaskGraphDefinition {
    nodes: Vec<AgentTaskGraphNodeDefinition>,
}

impl AgentTaskGraphDefinition {
    pub fn new(nodes: Vec<AgentTaskGraphNodeDefinition>) -> Result<Self, AgentTaskGraphError> {
        if nodes.is_empty() {
            return Err(AgentTaskGraphError::EmptyGraph);
        }
        if nodes.len() > AGENT_TASK_GRAPH_MAX_NODES {
            return Err(AgentTaskGraphError::TooManyNodes);
        }

        let provider_mode = nodes[0].task.provider_mode();
        for (index, node) in nodes.iter().enumerate() {
            if node.task.provider_mode() != provider_mode {
                return Err(AgentTaskGraphError::MixedProviderModes);
            }
            if node.dependencies.len() > AGENT_TASK_GRAPH_MAX_DEPENDENCIES {
                return Err(AgentTaskGraphError::TooManyDependencies);
            }
            for (dependency_index, dependency) in node.dependencies.iter().enumerate() {
                if dependency.index() >= index {
                    return Err(AgentTaskGraphError::ForwardOrSelfDependency);
                }
                if node.dependencies[..dependency_index].contains(dependency) {
                    return Err(AgentTaskGraphError::DuplicateDependency);
                }
            }
            if nodes[..index]
                .iter()
                .any(|previous| previous.task == node.task)
            {
                return Err(AgentTaskGraphError::DuplicateTask);
            }
            if let Some(compensation) = node.compensation.as_ref()
                && !valid_compensation(&node.task, compensation)
            {
                return Err(AgentTaskGraphError::InvalidCompensation);
            }
        }

        Ok(Self { nodes })
    }

    pub fn nodes(&self) -> &[AgentTaskGraphNodeDefinition] {
        &self.nodes
    }

    /// Builds the stable three-node path used to make a local model available and then connect a
    /// supported external Agent. Every node and edge is Rust-owned; model output cannot alter it.
    pub fn prepare_external_agent(
        model_id: String,
        integration_id: ExternalAgentIntegrationId,
        provider_mode: AgentTaskProviderMode,
    ) -> Result<Self, AgentTaskGraphBuildError> {
        let install_engine = graph_task(
            AgentTaskKind::InstallEngine,
            AgentTaskTarget::llama_cpp(),
            provider_mode,
        )?;
        let remove_engine = graph_task(
            AgentTaskKind::RemoveEngine,
            AgentTaskTarget::llama_cpp(),
            provider_mode,
        )?;
        let start_model = graph_task(
            AgentTaskKind::StartModel,
            AgentTaskTarget::model(Some(model_id)).map_err(AgentTaskGraphBuildError::Task)?,
            provider_mode,
        )?;
        let configure = graph_task(
            AgentTaskKind::ConfigureExternalAgent,
            AgentTaskTarget::external_agent(integration_id),
            provider_mode,
        )?;
        let disconnect = graph_task(
            AgentTaskKind::DisconnectExternalAgent,
            AgentTaskTarget::external_agent(integration_id),
            provider_mode,
        )?;
        Self::new(vec![
            AgentTaskGraphNodeDefinition::new(install_engine, vec![])
                .with_compensation(remove_engine),
            AgentTaskGraphNodeDefinition::new(
                start_model,
                vec![AgentTaskGraphNodeId::from_index(0)],
            ),
            AgentTaskGraphNodeDefinition::new(configure, vec![AgentTaskGraphNodeId::from_index(1)])
                .with_compensation(disconnect),
        ])
        .map_err(AgentTaskGraphBuildError::InvalidGraph)
    }

    /// Adds HAL100's private Pi installation as a second prerequisite of configuration. This path
    /// is intentionally unavailable for other integrations because their installation is not
    /// owned by the managed Pi deployment contract.
    pub fn prepare_managed_pi(
        model_id: String,
        provider_mode: AgentTaskProviderMode,
    ) -> Result<Self, AgentTaskGraphBuildError> {
        let mut definition = Self::prepare_external_agent(
            model_id,
            ExternalAgentIntegrationId::PiCodingAgent,
            provider_mode,
        )?;
        let install_pi = graph_task(
            AgentTaskKind::InstallManagedExternalAgent,
            AgentTaskTarget::external_agent(ExternalAgentIntegrationId::PiCodingAgent),
            provider_mode,
        )?;
        let remove_pi = graph_task(
            AgentTaskKind::RemoveManagedExternalAgent,
            AgentTaskTarget::external_agent(ExternalAgentIntegrationId::PiCodingAgent),
            provider_mode,
        )?;
        let configure = definition
            .nodes
            .pop()
            .expect("base path always has a configuration node");
        definition.nodes.push(
            AgentTaskGraphNodeDefinition::new(
                install_pi,
                vec![AgentTaskGraphNodeId::from_index(0)],
            )
            .with_compensation(remove_pi),
        );
        definition.nodes.push(
            AgentTaskGraphNodeDefinition::new(
                configure.task,
                vec![
                    AgentTaskGraphNodeId::from_index(1),
                    AgentTaskGraphNodeId::from_index(2),
                ],
            )
            .with_compensation(
                configure
                    .compensation
                    .expect("base configuration always has compensation"),
            ),
        );
        Self::new(definition.nodes).map_err(AgentTaskGraphBuildError::InvalidGraph)
    }
}

fn graph_task(
    task_kind: AgentTaskKind,
    target: AgentTaskTarget,
    provider_mode: AgentTaskProviderMode,
) -> Result<AgentTaskSpec, AgentTaskGraphBuildError> {
    AgentTaskSpec::new(task_kind, target, provider_mode).map_err(AgentTaskGraphBuildError::Task)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTaskGraphNodeState {
    Blocked,
    Ready,
    Running,
    AwaitingConfirmation,
    Succeeded,
    Failed,
    Compensating,
    Compensated,
    Cancelled,
}

impl AgentTaskGraphNodeState {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Blocked => "blocked",
            Self::Ready => "ready",
            Self::Running => "running",
            Self::AwaitingConfirmation => "awaiting_confirmation",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Compensating => "compensating",
            Self::Compensated => "compensated",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTaskGraphState {
    Active,
    Succeeded,
    Failed,
    Compensating,
    Compensated,
    Cancelled,
}

impl AgentTaskGraphState {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Compensating => "compensating",
            Self::Compensated => "compensated",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTaskCompletionEffect {
    Observed,
    AlreadySatisfied,
    ChangedOwnedState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentTaskGraphNode {
    definition: AgentTaskGraphNodeDefinition,
    state: AgentTaskGraphNodeState,
    evidence_source: Option<AgentTaskEvidenceSource>,
    completion_effect: Option<AgentTaskCompletionEffect>,
    compensation_attempted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTaskGraph {
    nodes: Vec<AgentTaskGraphNode>,
    state: AgentTaskGraphState,
    checkpoint_sequence: u32,
}

impl AgentTaskGraph {
    pub fn new(definition: AgentTaskGraphDefinition) -> Self {
        let nodes = definition
            .nodes
            .into_iter()
            .map(|definition| {
                let state = if definition.dependencies.is_empty() {
                    AgentTaskGraphNodeState::Ready
                } else {
                    AgentTaskGraphNodeState::Blocked
                };
                AgentTaskGraphNode {
                    definition,
                    state,
                    evidence_source: None,
                    completion_effect: None,
                    compensation_attempted: false,
                }
            })
            .collect();
        Self {
            nodes,
            state: AgentTaskGraphState::Active,
            checkpoint_sequence: 1,
        }
    }

    /// Rebuilds an executable graph from a caller-supplied exact definition and a redacted
    /// checkpoint. The checkpoint is used only to validate semantic shape and advance its
    /// sequence. No success, plan, confirmation, run, or compensation authority is restored;
    /// every node starts from the dependency-derived ready/blocked state and must be observed
    /// again against current reality.
    pub fn restore_for_revalidation(
        definition: AgentTaskGraphDefinition,
        checkpoint: &AgentTaskGraphCheckpoint,
    ) -> Result<Self, AgentTaskGraphError> {
        if checkpoint.schema_version != AGENT_TASK_GRAPH_SCHEMA_VERSION {
            return Err(AgentTaskGraphError::UnsupportedCheckpointSchema);
        }
        if definition.nodes.len() != checkpoint.nodes.len() {
            return Err(AgentTaskGraphError::CheckpointMismatch);
        }
        for (index, (definition_node, checkpoint_node)) in
            definition.nodes.iter().zip(&checkpoint.nodes).enumerate()
        {
            if checkpoint_node.node_id.index() != index
                || checkpoint_node.task_kind != definition_node.task.task_kind()
                || checkpoint_node.target_kind != definition_node.task.target().kind()
                || checkpoint_node.success_predicate != definition_node.task.success_predicate()
                || checkpoint_node.dependencies != definition_node.dependencies
                || checkpoint_node.evidence_source.is_some_and(|source| {
                    !definition_node.task.accepts_evidence_source(source)
                        && !definition_node
                            .compensation
                            .as_ref()
                            .is_some_and(|compensation| {
                                compensation.accepts_evidence_source(source)
                            })
                })
                || (checkpoint_node.changed_owned_state
                    && !definition_node
                        .task
                        .constraints()
                        .requires_native_confirmation)
            {
                return Err(AgentTaskGraphError::CheckpointMismatch);
            }
        }
        let mut graph = Self::new(definition);
        graph.checkpoint_sequence = checkpoint.checkpoint_sequence.saturating_add(1);
        Ok(graph)
    }

    pub const fn state(&self) -> AgentTaskGraphState {
        self.state
    }

    pub const fn checkpoint_sequence(&self) -> u32 {
        self.checkpoint_sequence
    }

    pub fn node_state(
        &self,
        node_id: AgentTaskGraphNodeId,
    ) -> Result<AgentTaskGraphNodeState, AgentTaskGraphError> {
        self.node(node_id).map(|node| node.state)
    }

    pub fn next_ready(&self) -> Option<(AgentTaskGraphNodeId, &AgentTaskSpec)> {
        if self.state != AgentTaskGraphState::Active {
            return None;
        }
        self.nodes.iter().enumerate().find_map(|(index, node)| {
            (node.state == AgentTaskGraphNodeState::Ready).then_some((
                AgentTaskGraphNodeId::from_index(index as u8),
                &node.definition.task,
            ))
        })
    }

    pub fn active_node(
        &self,
    ) -> Option<(
        AgentTaskGraphNodeId,
        AgentTaskGraphNodeState,
        &AgentTaskSpec,
    )> {
        self.nodes.iter().enumerate().find_map(|(index, node)| {
            matches!(
                node.state,
                AgentTaskGraphNodeState::Running
                    | AgentTaskGraphNodeState::AwaitingConfirmation
                    | AgentTaskGraphNodeState::Compensating
            )
            .then_some((
                AgentTaskGraphNodeId::from_index(index as u8),
                node.state,
                &node.definition.task,
            ))
        })
    }

    pub fn active_compensation(&self) -> Option<(AgentTaskGraphNodeId, &AgentTaskSpec)> {
        self.nodes.iter().enumerate().find_map(|(index, node)| {
            (node.state == AgentTaskGraphNodeState::Compensating).then(|| {
                (
                    AgentTaskGraphNodeId::from_index(index as u8),
                    node.definition
                        .compensation
                        .as_ref()
                        .expect("compensating nodes always have a Rust-defined inverse task"),
                )
            })
        })
    }

    pub fn start(&mut self, node_id: AgentTaskGraphNodeId) -> Result<(), AgentTaskGraphError> {
        self.ensure_graph_state(AgentTaskGraphState::Active)?;
        if self.active_node().is_some() {
            return Err(AgentTaskGraphError::InvalidTransition);
        }
        self.transition_node(
            node_id,
            AgentTaskGraphNodeState::Ready,
            AgentTaskGraphNodeState::Running,
        )
    }

    pub fn await_confirmation(
        &mut self,
        node_id: AgentTaskGraphNodeId,
    ) -> Result<(), AgentTaskGraphError> {
        let node = self.node(node_id)?;
        if !node
            .definition
            .task
            .constraints()
            .requires_native_confirmation
        {
            return Err(AgentTaskGraphError::InvalidTransition);
        }
        self.transition_node(
            node_id,
            AgentTaskGraphNodeState::Running,
            AgentTaskGraphNodeState::AwaitingConfirmation,
        )
    }

    pub fn confirm(&mut self, node_id: AgentTaskGraphNodeId) -> Result<(), AgentTaskGraphError> {
        self.transition_node(
            node_id,
            AgentTaskGraphNodeState::AwaitingConfirmation,
            AgentTaskGraphNodeState::Running,
        )
    }

    pub fn succeed(
        &mut self,
        node_id: AgentTaskGraphNodeId,
        evidence_source: AgentTaskEvidenceSource,
        completion_effect: AgentTaskCompletionEffect,
    ) -> Result<(), AgentTaskGraphError> {
        self.ensure_graph_state(AgentTaskGraphState::Active)?;
        let node = self.node(node_id)?;
        if node.state != AgentTaskGraphNodeState::Running {
            return Err(AgentTaskGraphError::InvalidTransition);
        }
        if !node
            .definition
            .task
            .accepts_evidence_source(evidence_source)
        {
            return Err(AgentTaskGraphError::EvidenceRejected);
        }
        if completion_effect == AgentTaskCompletionEffect::ChangedOwnedState
            && !node
                .definition
                .task
                .constraints()
                .requires_native_confirmation
        {
            return Err(AgentTaskGraphError::InvalidCompletionEffect);
        }

        let node = self.node_mut(node_id)?;
        node.state = AgentTaskGraphNodeState::Succeeded;
        node.evidence_source = Some(evidence_source);
        node.completion_effect = Some(completion_effect);
        self.advance_sequence();
        self.refresh_ready_nodes();
        if self
            .nodes
            .iter()
            .all(|node| node.state == AgentTaskGraphNodeState::Succeeded)
        {
            self.state = AgentTaskGraphState::Succeeded;
            self.advance_sequence();
        }
        Ok(())
    }

    pub fn fail(&mut self, node_id: AgentTaskGraphNodeId) -> Result<(), AgentTaskGraphError> {
        self.ensure_graph_state(AgentTaskGraphState::Active)?;
        let node = self.node_mut(node_id)?;
        if !matches!(
            node.state,
            AgentTaskGraphNodeState::Running | AgentTaskGraphNodeState::AwaitingConfirmation
        ) {
            return Err(AgentTaskGraphError::InvalidTransition);
        }
        node.state = AgentTaskGraphNodeState::Failed;
        for node in &mut self.nodes {
            if matches!(
                node.state,
                AgentTaskGraphNodeState::Ready | AgentTaskGraphNodeState::Blocked
            ) {
                node.state = AgentTaskGraphNodeState::Blocked;
            }
        }
        self.state = AgentTaskGraphState::Failed;
        self.advance_sequence();
        Ok(())
    }

    pub fn cancel(&mut self) -> Result<(), AgentTaskGraphError> {
        self.ensure_graph_state(AgentTaskGraphState::Active)?;
        for node in &mut self.nodes {
            if matches!(
                node.state,
                AgentTaskGraphNodeState::Ready
                    | AgentTaskGraphNodeState::Blocked
                    | AgentTaskGraphNodeState::Running
                    | AgentTaskGraphNodeState::AwaitingConfirmation
            ) {
                node.state = AgentTaskGraphNodeState::Cancelled;
            }
        }
        self.state = AgentTaskGraphState::Cancelled;
        self.advance_sequence();
        Ok(())
    }

    /// Drops any in-process execution/confirmation authority while retaining already verified
    /// outcomes. The caller must start and, for mutations, plan and confirm the ready node again.
    pub fn lose_in_process_authority(&mut self) -> Result<(), AgentTaskGraphError> {
        self.ensure_graph_state(AgentTaskGraphState::Active)?;
        for node in &mut self.nodes {
            if matches!(
                node.state,
                AgentTaskGraphNodeState::Running
                    | AgentTaskGraphNodeState::AwaitingConfirmation
                    | AgentTaskGraphNodeState::Ready
            ) {
                node.state = AgentTaskGraphNodeState::Blocked;
            }
        }
        self.refresh_ready_nodes();
        self.advance_sequence();
        Ok(())
    }

    pub fn begin_next_compensation(
        &mut self,
    ) -> Result<(AgentTaskGraphNodeId, AgentTaskSpec), AgentTaskGraphError> {
        if !matches!(
            self.state,
            AgentTaskGraphState::Failed
                | AgentTaskGraphState::Cancelled
                | AgentTaskGraphState::Compensating
        ) {
            return Err(AgentTaskGraphError::CompensationUnavailable);
        }
        if self
            .nodes
            .iter()
            .any(|node| node.state == AgentTaskGraphNodeState::Compensating)
        {
            return Err(AgentTaskGraphError::InvalidTransition);
        }
        let Some(index) = self.nodes.iter().rposition(compensation_candidate) else {
            return Err(AgentTaskGraphError::CompensationUnavailable);
        };
        let node = &mut self.nodes[index];
        node.state = AgentTaskGraphNodeState::Compensating;
        node.compensation_attempted = true;
        let compensation = node
            .definition
            .compensation
            .clone()
            .expect("candidate always has a compensation");
        self.state = AgentTaskGraphState::Compensating;
        self.advance_sequence();
        Ok((AgentTaskGraphNodeId::from_index(index as u8), compensation))
    }

    pub fn complete_compensation(
        &mut self,
        node_id: AgentTaskGraphNodeId,
        evidence_source: AgentTaskEvidenceSource,
    ) -> Result<(), AgentTaskGraphError> {
        self.ensure_graph_state(AgentTaskGraphState::Compensating)?;
        let node = self.node(node_id)?;
        let compensation = node
            .definition
            .compensation
            .as_ref()
            .ok_or(AgentTaskGraphError::CompensationUnavailable)?;
        if node.state != AgentTaskGraphNodeState::Compensating {
            return Err(AgentTaskGraphError::InvalidTransition);
        }
        if !compensation.accepts_evidence_source(evidence_source) {
            return Err(AgentTaskGraphError::EvidenceRejected);
        }
        let node = self.node_mut(node_id)?;
        node.state = AgentTaskGraphNodeState::Compensated;
        node.evidence_source = Some(evidence_source);
        self.advance_sequence();
        if !self.nodes.iter().any(compensation_candidate) {
            self.state = AgentTaskGraphState::Compensated;
            self.advance_sequence();
        }
        Ok(())
    }

    pub fn fail_compensation(
        &mut self,
        node_id: AgentTaskGraphNodeId,
    ) -> Result<(), AgentTaskGraphError> {
        self.ensure_graph_state(AgentTaskGraphState::Compensating)?;
        self.transition_node(
            node_id,
            AgentTaskGraphNodeState::Compensating,
            AgentTaskGraphNodeState::Failed,
        )?;
        self.state = AgentTaskGraphState::Failed;
        self.advance_sequence();
        Ok(())
    }

    pub fn checkpoint(&self) -> AgentTaskGraphCheckpoint {
        AgentTaskGraphCheckpoint {
            schema_version: AGENT_TASK_GRAPH_SCHEMA_VERSION,
            checkpoint_sequence: self.checkpoint_sequence,
            state: self.state,
            nodes: self
                .nodes
                .iter()
                .enumerate()
                .map(|(index, node)| AgentTaskGraphNodeCheckpoint {
                    node_id: AgentTaskGraphNodeId::from_index(index as u8),
                    state: node.state,
                    task_kind: node.definition.task.task_kind(),
                    target_kind: node.definition.task.target().kind(),
                    success_predicate: node.definition.task.success_predicate(),
                    dependencies: node.definition.dependencies.clone(),
                    evidence_source: node.evidence_source,
                    changed_owned_state: node.completion_effect
                        == Some(AgentTaskCompletionEffect::ChangedOwnedState),
                    requires_reauthorization: matches!(
                        node.state,
                        AgentTaskGraphNodeState::Running
                            | AgentTaskGraphNodeState::AwaitingConfirmation
                            | AgentTaskGraphNodeState::Compensating
                    ),
                })
                .collect(),
        }
    }

    fn node(
        &self,
        node_id: AgentTaskGraphNodeId,
    ) -> Result<&AgentTaskGraphNode, AgentTaskGraphError> {
        self.nodes
            .get(node_id.index())
            .ok_or(AgentTaskGraphError::NodeUnavailable)
    }

    fn node_mut(
        &mut self,
        node_id: AgentTaskGraphNodeId,
    ) -> Result<&mut AgentTaskGraphNode, AgentTaskGraphError> {
        self.nodes
            .get_mut(node_id.index())
            .ok_or(AgentTaskGraphError::NodeUnavailable)
    }

    fn ensure_graph_state(&self, state: AgentTaskGraphState) -> Result<(), AgentTaskGraphError> {
        if self.state == state {
            Ok(())
        } else {
            Err(AgentTaskGraphError::InvalidTransition)
        }
    }

    fn transition_node(
        &mut self,
        node_id: AgentTaskGraphNodeId,
        current: AgentTaskGraphNodeState,
        next: AgentTaskGraphNodeState,
    ) -> Result<(), AgentTaskGraphError> {
        let node = self.node_mut(node_id)?;
        if node.state != current {
            return Err(AgentTaskGraphError::InvalidTransition);
        }
        node.state = next;
        self.advance_sequence();
        Ok(())
    }

    fn refresh_ready_nodes(&mut self) {
        let succeeded = self
            .nodes
            .iter()
            .map(|node| node.state == AgentTaskGraphNodeState::Succeeded)
            .collect::<Vec<_>>();
        for node in &mut self.nodes {
            if node.state == AgentTaskGraphNodeState::Blocked
                && node
                    .definition
                    .dependencies
                    .iter()
                    .all(|dependency| succeeded[dependency.index()])
            {
                node.state = AgentTaskGraphNodeState::Ready;
            }
        }
    }

    fn advance_sequence(&mut self) {
        self.checkpoint_sequence = self.checkpoint_sequence.saturating_add(1);
    }
}

fn compensation_candidate(node: &AgentTaskGraphNode) -> bool {
    node.state == AgentTaskGraphNodeState::Succeeded
        && node.completion_effect == Some(AgentTaskCompletionEffect::ChangedOwnedState)
        && node.definition.compensation.is_some()
        && !node.compensation_attempted
}

fn valid_compensation(forward: &AgentTaskSpec, compensation: &AgentTaskSpec) -> bool {
    if forward.provider_mode() != compensation.provider_mode()
        || forward.target() != compensation.target()
        || !compensation.constraints().requires_native_confirmation
    {
        return false;
    }
    matches!(
        (forward.task_kind(), compensation.task_kind()),
        (AgentTaskKind::InstallEngine, AgentTaskKind::RemoveEngine)
            | (
                AgentTaskKind::ConfigureExternalAgent,
                AgentTaskKind::DisconnectExternalAgent
            )
            | (
                AgentTaskKind::InstallManagedExternalAgent,
                AgentTaskKind::RemoveManagedExternalAgent
            )
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTaskGraphCheckpoint {
    pub schema_version: u8,
    pub checkpoint_sequence: u32,
    pub state: AgentTaskGraphState,
    pub nodes: Vec<AgentTaskGraphNodeCheckpoint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTaskGraphNodeCheckpoint {
    pub node_id: AgentTaskGraphNodeId,
    pub state: AgentTaskGraphNodeState,
    pub task_kind: AgentTaskKind,
    pub target_kind: AgentTaskTargetKind,
    pub success_predicate: AgentTaskSuccessPredicate,
    pub dependencies: Vec<AgentTaskGraphNodeId>,
    pub evidence_source: Option<AgentTaskEvidenceSource>,
    pub changed_owned_state: bool,
    pub requires_reauthorization: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTaskGraphError {
    EmptyGraph,
    TooManyNodes,
    TooManyDependencies,
    ForwardOrSelfDependency,
    DuplicateDependency,
    DuplicateTask,
    MixedProviderModes,
    InvalidCompensation,
    NodeUnavailable,
    InvalidTransition,
    EvidenceRejected,
    InvalidCompletionEffect,
    CompensationUnavailable,
    UnsupportedCheckpointSchema,
    CheckpointMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTaskGraphBuildError {
    Task(AgentTaskSpecError),
    InvalidGraph(AgentTaskGraphError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentTaskProviderMode, AgentTaskTarget, ExternalAgentIntegrationId};

    fn task(task_kind: AgentTaskKind) -> AgentTaskSpec {
        let target = match task_kind {
            AgentTaskKind::InspectSystem => AgentTaskTarget::system(),
            AgentTaskKind::InspectRuntime | AgentTaskKind::StartModel => {
                if task_kind == AgentTaskKind::StartModel {
                    AgentTaskTarget::model(Some("fixture-model".into())).expect("model target")
                } else {
                    AgentTaskTarget::runtime()
                }
            }
            AgentTaskKind::InstallEngine | AgentTaskKind::RemoveEngine => {
                AgentTaskTarget::llama_cpp()
            }
            AgentTaskKind::InspectExternalAgent
            | AgentTaskKind::ConfigureExternalAgent
            | AgentTaskKind::DisconnectExternalAgent
            | AgentTaskKind::InstallManagedExternalAgent
            | AgentTaskKind::RemoveManagedExternalAgent => {
                AgentTaskTarget::external_agent(ExternalAgentIntegrationId::PiCodingAgent)
            }
            _ => AgentTaskTarget::environment(),
        };
        AgentTaskSpec::new(task_kind, target, AgentTaskProviderMode::Local).expect("task spec")
    }

    #[test]
    fn definition_rejects_cycles_duplicates_and_unsafe_compensation() {
        assert_eq!(
            AgentTaskGraphDefinition::new(Vec::new()),
            Err(AgentTaskGraphError::EmptyGraph)
        );
        assert_eq!(
            AgentTaskGraphDefinition::new(vec![AgentTaskGraphNodeDefinition::new(
                task(AgentTaskKind::InspectSystem),
                vec![AgentTaskGraphNodeId::from_index(0)],
            )]),
            Err(AgentTaskGraphError::ForwardOrSelfDependency)
        );
        assert_eq!(
            AgentTaskGraphDefinition::new(vec![
                AgentTaskGraphNodeDefinition::new(task(AgentTaskKind::InspectSystem), vec![]),
                AgentTaskGraphNodeDefinition::new(task(AgentTaskKind::InspectSystem), vec![]),
            ]),
            Err(AgentTaskGraphError::DuplicateTask)
        );
        assert_eq!(
            AgentTaskGraphDefinition::new(vec![
                AgentTaskGraphNodeDefinition::new(task(AgentTaskKind::InstallEngine), vec![])
                    .with_compensation(task(AgentTaskKind::DisconnectExternalAgent)),
            ]),
            Err(AgentTaskGraphError::InvalidCompensation)
        );
    }

    #[test]
    fn dependencies_unlock_only_after_rust_accepted_evidence() {
        let mut graph = AgentTaskGraph::new(
            AgentTaskGraphDefinition::new(vec![
                AgentTaskGraphNodeDefinition::new(task(AgentTaskKind::InstallEngine), vec![]),
                AgentTaskGraphNodeDefinition::new(
                    task(AgentTaskKind::StartModel),
                    vec![AgentTaskGraphNodeId::from_index(0)],
                ),
                AgentTaskGraphNodeDefinition::new(
                    task(AgentTaskKind::ConfigureExternalAgent),
                    vec![AgentTaskGraphNodeId::from_index(1)],
                ),
            ])
            .expect("graph definition"),
        );
        assert_eq!(
            graph.next_ready().map(|(id, _)| id),
            Some(AgentTaskGraphNodeId::from_index(0))
        );
        assert_eq!(
            graph.node_state(AgentTaskGraphNodeId::from_index(1)),
            Ok(AgentTaskGraphNodeState::Blocked)
        );

        graph
            .start(AgentTaskGraphNodeId::from_index(0))
            .expect("start engine node");
        assert_eq!(
            graph.succeed(
                AgentTaskGraphNodeId::from_index(0),
                AgentTaskEvidenceSource::SystemProbe,
                AgentTaskCompletionEffect::ChangedOwnedState,
            ),
            Err(AgentTaskGraphError::EvidenceRejected)
        );
        graph
            .succeed(
                AgentTaskGraphNodeId::from_index(0),
                AgentTaskEvidenceSource::EngineRecheck,
                AgentTaskCompletionEffect::ChangedOwnedState,
            )
            .expect("verified engine state");
        assert_eq!(
            graph.next_ready().map(|(id, _)| id),
            Some(AgentTaskGraphNodeId::from_index(1))
        );
    }

    #[test]
    fn only_one_graph_node_can_hold_execution_authority() {
        let mut graph = AgentTaskGraph::new(
            AgentTaskGraphDefinition::new(vec![
                AgentTaskGraphNodeDefinition::new(task(AgentTaskKind::InspectSystem), vec![]),
                AgentTaskGraphNodeDefinition::new(task(AgentTaskKind::InspectRuntime), vec![]),
            ])
            .expect("parallel roots"),
        );
        let first = AgentTaskGraphNodeId::from_index(0);
        let second = AgentTaskGraphNodeId::from_index(1);
        graph.start(first).expect("start first root");
        assert_eq!(
            graph.start(second),
            Err(AgentTaskGraphError::InvalidTransition)
        );
        assert_eq!(
            graph.active_node().map(|(id, state, _)| (id, state)),
            Some((first, AgentTaskGraphNodeState::Running))
        );
    }

    #[test]
    fn confirmation_authority_is_explicit_and_lost_on_restart_boundary() {
        let mut graph = AgentTaskGraph::new(
            AgentTaskGraphDefinition::new(vec![AgentTaskGraphNodeDefinition::new(
                task(AgentTaskKind::InstallEngine),
                vec![],
            )])
            .expect("graph definition"),
        );
        let node_id = AgentTaskGraphNodeId::from_index(0);
        graph.start(node_id).expect("start");
        graph.await_confirmation(node_id).expect("await");
        assert!(graph.checkpoint().nodes[0].requires_reauthorization);
        graph.lose_in_process_authority().expect("drop authority");
        assert_eq!(
            graph.node_state(node_id),
            Ok(AgentTaskGraphNodeState::Ready)
        );
        assert!(!graph.checkpoint().nodes[0].requires_reauthorization);
        assert_eq!(
            graph.confirm(node_id),
            Err(AgentTaskGraphError::InvalidTransition)
        );
    }

    #[test]
    fn failure_blocks_forward_work_and_compensation_is_bounded_and_reverse_ordered() {
        let engine = AgentTaskGraphNodeDefinition::new(task(AgentTaskKind::InstallEngine), vec![])
            .with_compensation(task(AgentTaskKind::RemoveEngine));
        let integration = AgentTaskGraphNodeDefinition::new(
            task(AgentTaskKind::ConfigureExternalAgent),
            vec![AgentTaskGraphNodeId::from_index(0)],
        )
        .with_compensation(task(AgentTaskKind::DisconnectExternalAgent));
        let final_node = AgentTaskGraphNodeDefinition::new(
            task(AgentTaskKind::InspectSystem),
            vec![AgentTaskGraphNodeId::from_index(1)],
        );
        let mut graph = AgentTaskGraph::new(
            AgentTaskGraphDefinition::new(vec![engine, integration, final_node])
                .expect("graph definition"),
        );

        for (index, evidence) in [
            (0, AgentTaskEvidenceSource::EngineRecheck),
            (1, AgentTaskEvidenceSource::IntegrationRecheck),
        ] {
            let node_id = AgentTaskGraphNodeId::from_index(index);
            graph.start(node_id).expect("start node");
            graph
                .succeed(
                    node_id,
                    evidence,
                    AgentTaskCompletionEffect::ChangedOwnedState,
                )
                .expect("complete node");
        }
        let failed = AgentTaskGraphNodeId::from_index(2);
        graph.start(failed).expect("start final node");
        graph.fail(failed).expect("fail final node");
        assert_eq!(graph.state(), AgentTaskGraphState::Failed);
        assert!(graph.next_ready().is_none());

        let (first, first_task) = graph
            .begin_next_compensation()
            .expect("integration compensation");
        assert_eq!(first, AgentTaskGraphNodeId::from_index(1));
        assert_eq!(
            first_task.task_kind(),
            AgentTaskKind::DisconnectExternalAgent
        );
        assert_eq!(
            graph.begin_next_compensation(),
            Err(AgentTaskGraphError::InvalidTransition)
        );
        graph
            .complete_compensation(first, AgentTaskEvidenceSource::IntegrationRecheck)
            .expect("integration compensated");

        let (second, second_task) = graph
            .begin_next_compensation()
            .expect("engine compensation");
        assert_eq!(second, AgentTaskGraphNodeId::from_index(0));
        assert_eq!(second_task.task_kind(), AgentTaskKind::RemoveEngine);
        graph
            .complete_compensation(second, AgentTaskEvidenceSource::EngineRecheck)
            .expect("engine compensated");
        assert_eq!(graph.state(), AgentTaskGraphState::Compensated);
        assert_eq!(
            graph.begin_next_compensation(),
            Err(AgentTaskGraphError::CompensationUnavailable)
        );
    }

    #[test]
    fn already_satisfied_nodes_are_never_compensation_candidates() {
        let mut graph = AgentTaskGraph::new(
            AgentTaskGraphDefinition::new(vec![
                AgentTaskGraphNodeDefinition::new(task(AgentTaskKind::InstallEngine), vec![])
                    .with_compensation(task(AgentTaskKind::RemoveEngine)),
                AgentTaskGraphNodeDefinition::new(
                    task(AgentTaskKind::InspectSystem),
                    vec![AgentTaskGraphNodeId::from_index(0)],
                ),
            ])
            .expect("graph definition"),
        );
        let engine = AgentTaskGraphNodeId::from_index(0);
        graph.start(engine).expect("start engine");
        graph
            .succeed(
                engine,
                AgentTaskEvidenceSource::EngineRecheck,
                AgentTaskCompletionEffect::AlreadySatisfied,
            )
            .expect("already installed");
        let inspection = AgentTaskGraphNodeId::from_index(1);
        graph.start(inspection).expect("start inspection");
        graph.fail(inspection).expect("fail inspection");
        assert_eq!(
            graph.begin_next_compensation(),
            Err(AgentTaskGraphError::CompensationUnavailable)
        );
    }

    #[test]
    fn checkpoint_contains_only_bounded_semantic_identity() {
        let mut graph = AgentTaskGraph::new(
            AgentTaskGraphDefinition::new(vec![AgentTaskGraphNodeDefinition::new(
                task(AgentTaskKind::StartModel),
                vec![],
            )])
            .expect("graph definition"),
        );
        let node = AgentTaskGraphNodeId::from_index(0);
        graph.start(node).expect("start node");
        let checkpoint = graph.checkpoint();
        assert_eq!(checkpoint.schema_version, AGENT_TASK_GRAPH_SCHEMA_VERSION);
        assert_eq!(checkpoint.nodes.len(), 1);
        assert_eq!(checkpoint.nodes[0].task_kind, AgentTaskKind::StartModel);
        assert_eq!(checkpoint.nodes[0].target_kind, AgentTaskTargetKind::Model);
        let debug = format!("{checkpoint:?}");
        assert!(!debug.contains("fixture-model"));
        for forbidden in ["prompt", "answer", "plan_id", "run_id", "credential"] {
            assert!(!debug.contains(forbidden));
        }
    }

    #[test]
    fn rust_factory_builds_exact_external_agent_readiness_graphs() {
        let generic = AgentTaskGraphDefinition::prepare_external_agent(
            "model-1".into(),
            ExternalAgentIntegrationId::OpenCode,
            AgentTaskProviderMode::Local,
        )
        .expect("generic readiness graph");
        assert_eq!(generic.nodes().len(), 3);
        assert_eq!(
            generic
                .nodes()
                .iter()
                .map(|node| node.task().task_kind())
                .collect::<Vec<_>>(),
            vec![
                AgentTaskKind::InstallEngine,
                AgentTaskKind::StartModel,
                AgentTaskKind::ConfigureExternalAgent,
            ]
        );
        assert_eq!(
            generic.nodes()[2].dependencies(),
            &[AgentTaskGraphNodeId::from_index(1)]
        );

        let managed_pi = AgentTaskGraphDefinition::prepare_managed_pi(
            "model-1".into(),
            AgentTaskProviderMode::Local,
        )
        .expect("managed Pi readiness graph");
        assert_eq!(managed_pi.nodes().len(), 4);
        assert_eq!(
            managed_pi
                .nodes()
                .iter()
                .map(|node| node.task().task_kind())
                .collect::<Vec<_>>(),
            vec![
                AgentTaskKind::InstallEngine,
                AgentTaskKind::StartModel,
                AgentTaskKind::InstallManagedExternalAgent,
                AgentTaskKind::ConfigureExternalAgent,
            ]
        );
        assert_eq!(
            managed_pi.nodes()[3].dependencies(),
            &[
                AgentTaskGraphNodeId::from_index(1),
                AgentTaskGraphNodeId::from_index(2),
            ]
        );
        assert_eq!(
            managed_pi.nodes()[2]
                .compensation()
                .map(AgentTaskSpec::task_kind),
            Some(AgentTaskKind::RemoveManagedExternalAgent)
        );
    }

    #[test]
    fn composite_graph_contract_tracks_bounded_security_semantics() {
        let manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../../../contracts/agent-evals/v10-composite-task-graphs.json"
        ))
        .expect("composite task graph contract");
        assert_eq!(manifest["schemaVersion"], 1);
        assert_eq!(
            manifest["thresholds"]["maxNodes"],
            AGENT_TASK_GRAPH_MAX_NODES
        );
        assert_eq!(
            manifest["thresholds"]["maxDependenciesPerNode"],
            AGENT_TASK_GRAPH_MAX_DEPENDENCIES
        );
        assert_eq!(manifest["thresholds"]["automaticCompensationCount"], 0);
        assert_eq!(manifest["thresholds"]["restoredPlanAuthorityCount"], 0);

        let required_states = manifest["requiredNodeStates"]
            .as_array()
            .expect("required states");
        for state in [
            AgentTaskGraphNodeState::Blocked,
            AgentTaskGraphNodeState::Ready,
            AgentTaskGraphNodeState::Running,
            AgentTaskGraphNodeState::AwaitingConfirmation,
            AgentTaskGraphNodeState::Succeeded,
            AgentTaskGraphNodeState::Failed,
            AgentTaskGraphNodeState::Compensating,
            AgentTaskGraphNodeState::Compensated,
            AgentTaskGraphNodeState::Cancelled,
        ] {
            assert!(required_states.iter().any(|value| value == state.key()));
        }
        assert!(
            manifest["scenarios"]
                .as_array()
                .is_some_and(|scenarios| scenarios.len() >= 10)
        );
    }

    #[test]
    fn restart_restores_only_shape_and_revalidates_every_node_from_reality() {
        let definition = AgentTaskGraphDefinition::prepare_external_agent(
            "model-1".into(),
            ExternalAgentIntegrationId::OpenCode,
            AgentTaskProviderMode::Local,
        )
        .expect("readiness graph");
        let mut graph = AgentTaskGraph::new(definition.clone());
        let engine = AgentTaskGraphNodeId::from_index(0);
        graph.start(engine).expect("start engine");
        graph
            .succeed(
                engine,
                AgentTaskEvidenceSource::EngineRecheck,
                AgentTaskCompletionEffect::ChangedOwnedState,
            )
            .expect("engine verified");
        let model = AgentTaskGraphNodeId::from_index(1);
        graph.start(model).expect("start model");
        graph
            .await_confirmation(model)
            .expect("await model confirmation");
        let checkpoint = graph.checkpoint();
        assert_eq!(
            checkpoint.nodes[0].state,
            AgentTaskGraphNodeState::Succeeded
        );
        assert_eq!(
            checkpoint.nodes[1].state,
            AgentTaskGraphNodeState::AwaitingConfirmation
        );

        let restored = AgentTaskGraph::restore_for_revalidation(definition, &checkpoint)
            .expect("restore redacted shape");
        assert_eq!(restored.state(), AgentTaskGraphState::Active);
        assert_eq!(
            restored.node_state(engine),
            Ok(AgentTaskGraphNodeState::Ready)
        );
        assert_eq!(
            restored.node_state(model),
            Ok(AgentTaskGraphNodeState::Blocked)
        );
        assert!(
            restored
                .checkpoint()
                .nodes
                .iter()
                .all(|node| !node.requires_reauthorization && node.evidence_source.is_none())
        );
        assert_eq!(
            restored.checkpoint_sequence(),
            checkpoint.checkpoint_sequence.saturating_add(1)
        );
    }

    #[test]
    fn restart_rejects_tampered_or_unknown_checkpoint_shapes() {
        let definition = AgentTaskGraphDefinition::prepare_managed_pi(
            "model-1".into(),
            AgentTaskProviderMode::Local,
        )
        .expect("managed Pi graph");
        let checkpoint = AgentTaskGraph::new(definition.clone()).checkpoint();

        let mut tampered = checkpoint.clone();
        tampered.nodes[3].dependencies.clear();
        assert_eq!(
            AgentTaskGraph::restore_for_revalidation(definition.clone(), &tampered),
            Err(AgentTaskGraphError::CheckpointMismatch)
        );

        let mut unsupported = checkpoint;
        unsupported.schema_version = AGENT_TASK_GRAPH_SCHEMA_VERSION.saturating_add(1);
        assert_eq!(
            AgentTaskGraph::restore_for_revalidation(definition, &unsupported),
            Err(AgentTaskGraphError::UnsupportedCheckpointSchema)
        );
    }
}
