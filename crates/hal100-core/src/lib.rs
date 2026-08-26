mod agent_capability;
mod agent_intent;
mod agent_task;
mod agent_task_graph;
mod external_agent_integration;
mod tool_broker;

use std::{
    error::Error,
    fmt,
    sync::atomic::{AtomicU8, Ordering},
};

use hal100_protocol::{AppOverview, DatabaseState, PlatformSummary, ServiceState};

pub use agent_capability::{
    AGENT_CAPABILITY_COUNT, AgentCapabilityDataScope, AgentCapabilityDescriptor,
    AgentCapabilityEffect, AgentCapabilityId, AgentCapabilityRegistry, AgentCapabilityRisk,
    AgentCapabilitySet,
};
pub use agent_intent::{
    AGENT_TASK_INTENT_SCHEMA_VERSION, AgentTaskAdjudication, AgentTaskAdjudicationOutcome,
    AgentTaskAdjudicator, AgentTaskClarificationKind, AgentTaskClarificationResolution,
    AgentTaskClarificationSpec, AgentTaskClarificationSpecError, AgentTaskIntentRouter,
    AgentTaskProposalError, AgentTaskProposalValidator, AgentTaskRejectionReason, AgentTaskRoute,
};
pub use agent_task::{
    AGENT_TASK_KIND_COUNT, AgentTaskConstraints, AgentTaskDesiredState, AgentTaskKind,
    AgentTaskPhase, AgentTaskProviderMode, AgentTaskSpec, AgentTaskSpecError, AgentTaskState,
    AgentTaskSuccessPredicate, AgentTaskTarget, AgentTaskTargetKind, AgentTaskTransitionError,
    AgentTaskWorkflowRegistry, AgentWorkflowDefinition, AgentWorkflowStep,
};
pub use agent_task_graph::{
    AGENT_TASK_GRAPH_MAX_DEPENDENCIES, AGENT_TASK_GRAPH_MAX_NODES, AGENT_TASK_GRAPH_SCHEMA_VERSION,
    AgentTaskCompletionEffect, AgentTaskGraph, AgentTaskGraphBuildError, AgentTaskGraphCheckpoint,
    AgentTaskGraphDefinition, AgentTaskGraphError, AgentTaskGraphNodeCheckpoint,
    AgentTaskGraphNodeDefinition, AgentTaskGraphNodeId, AgentTaskGraphNodeState,
    AgentTaskGraphState,
};
pub use external_agent_integration::{
    BUILT_IN_AGENT_RUNTIME, BuiltInAgentIsolation, BuiltInAgentRuntimeDescriptor,
    ConfigurationOwnership, CredentialIsolation, ExternalAgentIntegrationAvailability,
    ExternalAgentIntegrationDescriptor, ExternalAgentIntegrationId,
    ExternalAgentIntegrationRegistry, GatewayProtocol, HERMES_AGENT_INTEGRATION,
    OPENCLAW_INTEGRATION, OPENCODE_INTEGRATION, PI_CODING_AGENT_INTEGRATION,
};
pub use tool_broker::{AgentToolPolicy, AuthorizedAgentTool, SimulatedToolBroker};

pub trait SystemProbe: Send + Sync + 'static {
    fn platform_summary(&self) -> PlatformSummary;
}

pub trait SecretStore: Send + Sync + 'static {
    fn read(&self, credential_id: &str) -> Result<Option<Vec<u8>>, SecretStoreError>;
    fn write(&self, credential_id: &str, secret: &[u8]) -> Result<(), SecretStoreError>;
    fn delete(&self, credential_id: &str) -> Result<(), SecretStoreError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretStoreOperation {
    Read,
    Write,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretStoreError {
    operation: SecretStoreOperation,
}

impl SecretStoreError {
    pub const fn new(operation: SecretStoreOperation) -> Self {
        Self { operation }
    }

    pub const fn operation(&self) -> SecretStoreOperation {
        self.operation
    }
}

impl fmt::Display for SecretStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let operation = match self.operation {
            SecretStoreOperation::Read => "read",
            SecretStoreOperation::Write => "write",
            SecretStoreOperation::Delete => "delete",
        };
        write!(formatter, "system credential store {operation} failed")
    }
}

impl Error for SecretStoreError {}

pub struct AppCore<P> {
    system_probe: P,
    gateway_state: AtomicU8,
}

impl<P> AppCore<P>
where
    P: SystemProbe,
{
    pub fn new(system_probe: P) -> Self {
        Self {
            system_probe,
            gateway_state: AtomicU8::new(service_state_code(ServiceState::NotStarted)),
        }
    }

    pub fn set_gateway_state(&self, state: ServiceState) {
        self.gateway_state
            .store(service_state_code(state), Ordering::Release);
    }

    pub fn overview(&self, version: &str) -> AppOverview {
        AppOverview {
            app_name: "HAL100".to_owned(),
            version: version.to_owned(),
            phase: "1.0.4 · 开发初期".to_owned(),
            gateway_state: service_state_from_code(self.gateway_state.load(Ordering::Acquire)),
            database_state: DatabaseState::Ready,
            platform: self.system_probe.platform_summary(),
        }
    }
}

const fn service_state_code(state: ServiceState) -> u8 {
    match state {
        ServiceState::NotStarted => 0,
        ServiceState::Running => 1,
        ServiceState::Error => 2,
    }
}

const fn service_state_from_code(code: u8) -> ServiceState {
    match code {
        1 => ServiceState::Running,
        2 => ServiceState::Error,
        _ => ServiceState::NotStarted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestProbe;

    impl SystemProbe for TestProbe {
        fn platform_summary(&self) -> PlatformSummary {
            PlatformSummary {
                os: "testOS".to_owned(),
                architecture: "testArch".to_owned(),
                supported: true,
            }
        }
    }

    #[test]
    fn builds_overview_without_desktop_framework_dependency() {
        let overview = AppCore::new(TestProbe).overview("0.0.1-test");

        assert_eq!(overview.app_name, "HAL100");
        assert_eq!(overview.platform.os, "testOS");
        assert_eq!(overview.gateway_state, ServiceState::NotStarted);

        let core = AppCore::new(TestProbe);
        core.set_gateway_state(ServiceState::Running);
        assert_eq!(
            core.overview("0.0.1-test").gateway_state,
            ServiceState::Running
        );
    }
}
