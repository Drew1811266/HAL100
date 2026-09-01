use crate::{
    AgentCapabilityDataScope, AgentCapabilityId, AgentCapabilitySet, ExternalAgentIntegrationId,
    ExternalAgentIntegrationRegistry,
};
#[cfg(test)]
use crate::{AgentCapabilityEffect, AgentCapabilityRegistry};
use hal100_protocol::{AgentActionKind, AgentTaskEvidenceSource};

pub const AGENT_TASK_KIND_COUNT: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentTaskKind {
    InspectSystem,
    InspectRuntime,
    DiagnoseEnvironment,
    RepairEnvironment,
    AnalyzeOperationalHistory,
    ObserveDeploymentHealth,
    StartModel,
    ActivateRuntimeProfile,
    StopModel,
    RemoveModel,
    SearchModelCatalog,
    InspectModelRepository,
    DownloadModel,
    InstallEngine,
    RemoveEngine,
    InspectExternalAgent,
    ConfigureExternalAgent,
    DisconnectExternalAgent,
    InstallManagedExternalAgent,
    RemoveManagedExternalAgent,
}

impl AgentTaskKind {
    pub const fn key(self) -> &'static str {
        match self {
            Self::InspectSystem => "inspect_system",
            Self::InspectRuntime => "inspect_runtime",
            Self::DiagnoseEnvironment => "diagnose_environment",
            Self::RepairEnvironment => "repair_environment",
            Self::AnalyzeOperationalHistory => "analyze_operational_history",
            Self::ObserveDeploymentHealth => "observe_deployment_health",
            Self::StartModel => "start_model",
            Self::ActivateRuntimeProfile => "activate_runtime_profile",
            Self::StopModel => "stop_model",
            Self::RemoveModel => "remove_model",
            Self::SearchModelCatalog => "search_model_catalog",
            Self::InspectModelRepository => "inspect_model_repository",
            Self::DownloadModel => "download_model",
            Self::InstallEngine => "install_engine",
            Self::RemoveEngine => "remove_engine",
            Self::InspectExternalAgent => "inspect_external_agent",
            Self::ConfigureExternalAgent => "configure_external_agent",
            Self::DisconnectExternalAgent => "disconnect_external_agent",
            Self::InstallManagedExternalAgent => "install_managed_external_agent",
            Self::RemoveManagedExternalAgent => "remove_managed_external_agent",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        AgentTaskWorkflowRegistry::all()
            .iter()
            .find(|workflow| workflow.task_kind.key() == key)
            .map(|workflow| workflow.task_kind)
    }

    /// Returns the single leaf capability owned by this task. The capability registry closes over
    /// prerequisites; model output never supplies or expands this mapping.
    pub const fn primary_capability(self) -> AgentCapabilityId {
        match self {
            Self::InspectSystem => AgentCapabilityId::InspectSystemSummary,
            Self::InspectRuntime => AgentCapabilityId::InspectRuntimeCatalog,
            Self::DiagnoseEnvironment => AgentCapabilityId::InspectEnvironmentDiagnostics,
            Self::RepairEnvironment => AgentCapabilityId::PlanDiagnosticRepair,
            Self::AnalyzeOperationalHistory => AgentCapabilityId::InspectOperationalHistory,
            Self::ObserveDeploymentHealth => AgentCapabilityId::ObserveOperationalHealth,
            Self::StartModel => AgentCapabilityId::PlanModelStart,
            Self::ActivateRuntimeProfile => AgentCapabilityId::PlanRuntimeProfileActivation,
            Self::StopModel => AgentCapabilityId::PlanModelStop,
            Self::RemoveModel => AgentCapabilityId::PlanModelRemoval,
            Self::SearchModelCatalog => AgentCapabilityId::SearchModelCatalog,
            Self::InspectModelRepository => AgentCapabilityId::InspectModelRepository,
            Self::DownloadModel => AgentCapabilityId::PlanModelDownload,
            Self::InstallEngine => AgentCapabilityId::PlanEngineInstall,
            Self::RemoveEngine => AgentCapabilityId::PlanEngineRemove,
            Self::InspectExternalAgent => AgentCapabilityId::InspectExternalAgent,
            Self::ConfigureExternalAgent => AgentCapabilityId::PlanExternalAgentConfiguration,
            Self::DisconnectExternalAgent => AgentCapabilityId::PlanExternalAgentDisconnection,
            Self::InstallManagedExternalAgent => AgentCapabilityId::PlanExternalAgentInstallation,
            Self::RemoveManagedExternalAgent => AgentCapabilityId::PlanManagedExternalAgentRemoval,
        }
    }

    /// Returns the native action kinds that may implement this Rust-owned task. Read-only tasks
    /// return an empty slice. Repair is intentionally limited to the three deterministic repair
    /// executors currently exposed by the diagnostic subsystem.
    pub const fn allowed_action_kinds(self) -> &'static [AgentActionKind] {
        match self {
            Self::RepairEnvironment => &[
                AgentActionKind::InstallLlamaCpp,
                AgentActionKind::ConfigureExternalAgent,
                AgentActionKind::RemoveModel,
            ],
            Self::StartModel => &[AgentActionKind::StartOrSwitchModel],
            Self::ActivateRuntimeProfile => &[AgentActionKind::ActivateRuntimeProfile],
            Self::StopModel => &[AgentActionKind::StopModel],
            Self::RemoveModel => &[AgentActionKind::RemoveModel],
            Self::DownloadModel => &[AgentActionKind::DownloadModel],
            Self::InstallEngine => &[AgentActionKind::InstallLlamaCpp],
            Self::RemoveEngine => &[AgentActionKind::RemoveLlamaCpp],
            Self::ConfigureExternalAgent => &[AgentActionKind::ConfigureExternalAgent],
            Self::DisconnectExternalAgent => &[AgentActionKind::DisconnectExternalAgent],
            Self::InstallManagedExternalAgent => &[AgentActionKind::InstallExternalAgent],
            Self::RemoveManagedExternalAgent => &[AgentActionKind::RemoveExternalAgent],
            Self::InspectSystem
            | Self::InspectRuntime
            | Self::DiagnoseEnvironment
            | Self::AnalyzeOperationalHistory
            | Self::ObserveDeploymentHealth
            | Self::SearchModelCatalog
            | Self::InspectModelRepository
            | Self::InspectExternalAgent => &[],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentTaskTargetKind {
    System,
    Runtime,
    RuntimeProfile,
    Environment,
    Model,
    ModelCatalog,
    LlamaCpp,
    ExternalAgent,
}

impl AgentTaskTargetKind {
    pub const fn key(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Runtime => "runtime",
            Self::RuntimeProfile => "runtime_profile",
            Self::Environment => "environment",
            Self::Model => "model",
            Self::ModelCatalog => "model_catalog",
            Self::LlamaCpp => "llama_cpp",
            Self::ExternalAgent => "external_agent",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTaskTarget {
    kind: AgentTaskTargetKind,
    resource_id: Option<String>,
}

impl AgentTaskTarget {
    pub const fn system() -> Self {
        Self::without_resource(AgentTaskTargetKind::System)
    }

    pub const fn runtime() -> Self {
        Self::without_resource(AgentTaskTargetKind::Runtime)
    }

    pub const fn runtime_profile() -> Self {
        Self::without_resource(AgentTaskTargetKind::RuntimeProfile)
    }

    pub const fn environment() -> Self {
        Self::without_resource(AgentTaskTargetKind::Environment)
    }

    pub const fn model_catalog() -> Self {
        Self::without_resource(AgentTaskTargetKind::ModelCatalog)
    }

    pub const fn llama_cpp() -> Self {
        Self::without_resource(AgentTaskTargetKind::LlamaCpp)
    }

    pub fn model(resource_id: Option<String>) -> Result<Self, AgentTaskSpecError> {
        Ok(Self {
            kind: AgentTaskTargetKind::Model,
            resource_id: validate_optional_resource_id(resource_id)?,
        })
    }

    pub fn external_agent(integration_id: ExternalAgentIntegrationId) -> Self {
        Self {
            kind: AgentTaskTargetKind::ExternalAgent,
            resource_id: Some(
                ExternalAgentIntegrationRegistry::descriptor(integration_id)
                    .integration_id
                    .to_owned(),
            ),
        }
    }

    pub const fn kind(&self) -> AgentTaskTargetKind {
        self.kind
    }

    pub fn resource_id(&self) -> Option<&str> {
        self.resource_id.as_deref()
    }

    const fn without_resource(kind: AgentTaskTargetKind) -> Self {
        Self {
            kind,
            resource_id: None,
        }
    }
}

fn validate_optional_resource_id(
    resource_id: Option<String>,
) -> Result<Option<String>, AgentTaskSpecError> {
    let Some(resource_id) = resource_id else {
        return Ok(None);
    };
    if resource_id.is_empty()
        || resource_id.len() > 128
        || resource_id.trim() != resource_id
        || resource_id.chars().any(char::is_control)
    {
        return Err(AgentTaskSpecError::InvalidResourceId);
    }
    Ok(Some(resource_id))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTaskDesiredState {
    Inspected,
    Diagnosed,
    Healthy,
    Running,
    Stopped,
    Absent,
    DownloadPlanned,
    Installed,
    Configured,
    Disconnected,
    Removed,
}

impl AgentTaskDesiredState {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Inspected => "inspected",
            Self::Diagnosed => "diagnosed",
            Self::Healthy => "healthy",
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Absent => "absent",
            Self::DownloadPlanned => "download_planned",
            Self::Installed => "installed",
            Self::Configured => "configured",
            Self::Disconnected => "disconnected",
            Self::Removed => "removed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTaskProviderMode {
    Local,
    CloudSingle,
    CloudSession,
}

impl AgentTaskProviderMode {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::CloudSingle => "cloud_single",
            Self::CloudSession => "cloud_session",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTaskSuccessPredicate {
    EvidenceCollected,
    EnvironmentDiagnosed,
    RepairFindingResolved,
    RuntimeModelActive,
    RuntimeProfileActive,
    RuntimeModelStopped,
    ModelAbsent,
    CatalogResultsAvailable,
    RepositoryInspected,
    DownloadPlanCreated,
    EngineInstalled,
    EngineAbsent,
    IntegrationConfigured,
    IntegrationDisconnected,
    ManagedInstallationPresent,
    ManagedInstallationAbsent,
}

impl AgentTaskSuccessPredicate {
    pub const fn key(self) -> &'static str {
        match self {
            Self::EvidenceCollected => "evidence_collected",
            Self::EnvironmentDiagnosed => "environment_diagnosed",
            Self::RepairFindingResolved => "repair_finding_resolved",
            Self::RuntimeModelActive => "runtime_model_active",
            Self::RuntimeProfileActive => "runtime_profile_active",
            Self::RuntimeModelStopped => "runtime_model_stopped",
            Self::ModelAbsent => "model_absent",
            Self::CatalogResultsAvailable => "catalog_results_available",
            Self::RepositoryInspected => "repository_inspected",
            Self::DownloadPlanCreated => "download_plan_created",
            Self::EngineInstalled => "engine_installed",
            Self::EngineAbsent => "engine_absent",
            Self::IntegrationConfigured => "integration_configured",
            Self::IntegrationDisconnected => "integration_disconnected",
            Self::ManagedInstallationPresent => "managed_installation_present",
            Self::ManagedInstallationAbsent => "managed_installation_absent",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentWorkflowStep {
    Inspect,
    Plan,
    AwaitNativeConfirmation,
    ExecuteDeterministically,
    VerifyDesiredState,
    Summarize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentTaskConstraints {
    pub max_pending_action_plans: u8,
    pub max_replan_attempts: u8,
    pub requires_native_confirmation: bool,
}

impl AgentTaskConstraints {
    pub const fn read_only() -> Self {
        Self {
            max_pending_action_plans: 0,
            max_replan_attempts: 0,
            requires_native_confirmation: false,
        }
    }

    pub const fn controlled_mutation() -> Self {
        Self {
            max_pending_action_plans: 1,
            max_replan_attempts: 1,
            requires_native_confirmation: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentWorkflowDefinition {
    pub task_kind: AgentTaskKind,
    pub target_kind: AgentTaskTargetKind,
    pub desired_state: AgentTaskDesiredState,
    pub data_scope: AgentCapabilityDataScope,
    pub success_predicate: AgentTaskSuccessPredicate,
    pub constraints: AgentTaskConstraints,
    pub steps: &'static [AgentWorkflowStep],
}

const READ_ONLY_STEPS: &[AgentWorkflowStep] = &[
    AgentWorkflowStep::Inspect,
    AgentWorkflowStep::VerifyDesiredState,
    AgentWorkflowStep::Summarize,
];

const CONTROLLED_MUTATION_STEPS: &[AgentWorkflowStep] = &[
    AgentWorkflowStep::Inspect,
    AgentWorkflowStep::Plan,
    AgentWorkflowStep::AwaitNativeConfirmation,
    AgentWorkflowStep::ExecuteDeterministically,
    AgentWorkflowStep::VerifyDesiredState,
    AgentWorkflowStep::Summarize,
];

const AGENT_TASK_WORKFLOWS: [AgentWorkflowDefinition; AGENT_TASK_KIND_COUNT] = [
    workflow(
        AgentTaskKind::InspectSystem,
        AgentTaskTargetKind::System,
        AgentTaskDesiredState::Inspected,
        AgentCapabilityDataScope::SystemMetadata,
        AgentTaskSuccessPredicate::EvidenceCollected,
        false,
    ),
    workflow(
        AgentTaskKind::InspectRuntime,
        AgentTaskTargetKind::Runtime,
        AgentTaskDesiredState::Inspected,
        AgentCapabilityDataScope::RuntimeMetadata,
        AgentTaskSuccessPredicate::EvidenceCollected,
        false,
    ),
    workflow(
        AgentTaskKind::DiagnoseEnvironment,
        AgentTaskTargetKind::Environment,
        AgentTaskDesiredState::Diagnosed,
        AgentCapabilityDataScope::DiagnosticMetadata,
        AgentTaskSuccessPredicate::EnvironmentDiagnosed,
        false,
    ),
    workflow(
        AgentTaskKind::RepairEnvironment,
        AgentTaskTargetKind::Environment,
        AgentTaskDesiredState::Healthy,
        AgentCapabilityDataScope::DiagnosticMetadata,
        AgentTaskSuccessPredicate::RepairFindingResolved,
        true,
    ),
    workflow(
        AgentTaskKind::AnalyzeOperationalHistory,
        AgentTaskTargetKind::Environment,
        AgentTaskDesiredState::Inspected,
        AgentCapabilityDataScope::OperationalMetadata,
        AgentTaskSuccessPredicate::EvidenceCollected,
        false,
    ),
    workflow(
        AgentTaskKind::ObserveDeploymentHealth,
        AgentTaskTargetKind::Environment,
        AgentTaskDesiredState::Inspected,
        AgentCapabilityDataScope::OperationalMetadata,
        AgentTaskSuccessPredicate::EvidenceCollected,
        false,
    ),
    workflow(
        AgentTaskKind::StartModel,
        AgentTaskTargetKind::Model,
        AgentTaskDesiredState::Running,
        AgentCapabilityDataScope::RuntimeMetadata,
        AgentTaskSuccessPredicate::RuntimeModelActive,
        true,
    ),
    workflow(
        AgentTaskKind::ActivateRuntimeProfile,
        AgentTaskTargetKind::RuntimeProfile,
        AgentTaskDesiredState::Running,
        AgentCapabilityDataScope::RuntimeMetadata,
        AgentTaskSuccessPredicate::RuntimeProfileActive,
        true,
    ),
    workflow(
        AgentTaskKind::StopModel,
        AgentTaskTargetKind::Model,
        AgentTaskDesiredState::Stopped,
        AgentCapabilityDataScope::RuntimeMetadata,
        AgentTaskSuccessPredicate::RuntimeModelStopped,
        true,
    ),
    workflow(
        AgentTaskKind::RemoveModel,
        AgentTaskTargetKind::Model,
        AgentTaskDesiredState::Absent,
        AgentCapabilityDataScope::RuntimeMetadata,
        AgentTaskSuccessPredicate::ModelAbsent,
        true,
    ),
    workflow(
        AgentTaskKind::SearchModelCatalog,
        AgentTaskTargetKind::ModelCatalog,
        AgentTaskDesiredState::Inspected,
        AgentCapabilityDataScope::PublicCatalogMetadata,
        AgentTaskSuccessPredicate::CatalogResultsAvailable,
        false,
    ),
    workflow(
        AgentTaskKind::InspectModelRepository,
        AgentTaskTargetKind::ModelCatalog,
        AgentTaskDesiredState::Inspected,
        AgentCapabilityDataScope::PublicCatalogMetadata,
        AgentTaskSuccessPredicate::RepositoryInspected,
        false,
    ),
    workflow(
        AgentTaskKind::DownloadModel,
        AgentTaskTargetKind::ModelCatalog,
        AgentTaskDesiredState::DownloadPlanned,
        AgentCapabilityDataScope::PublicCatalogMetadata,
        AgentTaskSuccessPredicate::DownloadPlanCreated,
        true,
    ),
    workflow(
        AgentTaskKind::InstallEngine,
        AgentTaskTargetKind::LlamaCpp,
        AgentTaskDesiredState::Installed,
        AgentCapabilityDataScope::RuntimeMetadata,
        AgentTaskSuccessPredicate::EngineInstalled,
        true,
    ),
    workflow(
        AgentTaskKind::RemoveEngine,
        AgentTaskTargetKind::LlamaCpp,
        AgentTaskDesiredState::Removed,
        AgentCapabilityDataScope::RuntimeMetadata,
        AgentTaskSuccessPredicate::EngineAbsent,
        true,
    ),
    workflow(
        AgentTaskKind::InspectExternalAgent,
        AgentTaskTargetKind::ExternalAgent,
        AgentTaskDesiredState::Inspected,
        AgentCapabilityDataScope::IntegrationMetadata,
        AgentTaskSuccessPredicate::EvidenceCollected,
        false,
    ),
    workflow(
        AgentTaskKind::ConfigureExternalAgent,
        AgentTaskTargetKind::ExternalAgent,
        AgentTaskDesiredState::Configured,
        AgentCapabilityDataScope::IntegrationMetadata,
        AgentTaskSuccessPredicate::IntegrationConfigured,
        true,
    ),
    workflow(
        AgentTaskKind::DisconnectExternalAgent,
        AgentTaskTargetKind::ExternalAgent,
        AgentTaskDesiredState::Disconnected,
        AgentCapabilityDataScope::IntegrationMetadata,
        AgentTaskSuccessPredicate::IntegrationDisconnected,
        true,
    ),
    workflow(
        AgentTaskKind::InstallManagedExternalAgent,
        AgentTaskTargetKind::ExternalAgent,
        AgentTaskDesiredState::Installed,
        AgentCapabilityDataScope::IntegrationMetadata,
        AgentTaskSuccessPredicate::ManagedInstallationPresent,
        true,
    ),
    workflow(
        AgentTaskKind::RemoveManagedExternalAgent,
        AgentTaskTargetKind::ExternalAgent,
        AgentTaskDesiredState::Removed,
        AgentCapabilityDataScope::IntegrationMetadata,
        AgentTaskSuccessPredicate::ManagedInstallationAbsent,
        true,
    ),
];

const fn workflow(
    task_kind: AgentTaskKind,
    target_kind: AgentTaskTargetKind,
    desired_state: AgentTaskDesiredState,
    data_scope: AgentCapabilityDataScope,
    success_predicate: AgentTaskSuccessPredicate,
    controlled_mutation: bool,
) -> AgentWorkflowDefinition {
    AgentWorkflowDefinition {
        task_kind,
        target_kind,
        desired_state,
        data_scope,
        success_predicate,
        constraints: if controlled_mutation {
            AgentTaskConstraints::controlled_mutation()
        } else {
            AgentTaskConstraints::read_only()
        },
        steps: if controlled_mutation {
            CONTROLLED_MUTATION_STEPS
        } else {
            READ_ONLY_STEPS
        },
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct AgentTaskWorkflowRegistry;

impl AgentTaskWorkflowRegistry {
    pub const fn all() -> &'static [AgentWorkflowDefinition] {
        &AGENT_TASK_WORKFLOWS
    }

    pub fn for_kind(task_kind: AgentTaskKind) -> &'static AgentWorkflowDefinition {
        AGENT_TASK_WORKFLOWS
            .iter()
            .find(|workflow| workflow.task_kind == task_kind)
            .expect("every stable Agent task kind has one workflow")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTaskSpec {
    task_kind: AgentTaskKind,
    target: AgentTaskTarget,
    desired_state: AgentTaskDesiredState,
    provider_mode: AgentTaskProviderMode,
    data_scope: AgentCapabilityDataScope,
    constraints: AgentTaskConstraints,
}

impl AgentTaskSpec {
    pub fn new(
        task_kind: AgentTaskKind,
        target: AgentTaskTarget,
        provider_mode: AgentTaskProviderMode,
    ) -> Result<Self, AgentTaskSpecError> {
        let workflow = AgentTaskWorkflowRegistry::for_kind(task_kind);
        if target.kind() != workflow.target_kind {
            return Err(AgentTaskSpecError::IncompatibleTarget);
        }
        if matches!(
            task_kind,
            AgentTaskKind::InstallManagedExternalAgent | AgentTaskKind::RemoveManagedExternalAgent
        ) && target.resource_id()
            != Some(
                ExternalAgentIntegrationRegistry::descriptor(
                    ExternalAgentIntegrationId::PiCodingAgent,
                )
                .integration_id,
            )
        {
            return Err(AgentTaskSpecError::IncompatibleTarget);
        }
        Ok(Self {
            task_kind,
            target,
            desired_state: workflow.desired_state,
            provider_mode,
            data_scope: workflow.data_scope,
            constraints: workflow.constraints,
        })
    }

    pub const fn task_kind(&self) -> AgentTaskKind {
        self.task_kind
    }

    pub const fn target(&self) -> &AgentTaskTarget {
        &self.target
    }

    pub const fn desired_state(&self) -> AgentTaskDesiredState {
        self.desired_state
    }

    pub const fn provider_mode(&self) -> AgentTaskProviderMode {
        self.provider_mode
    }

    pub const fn data_scope(&self) -> AgentCapabilityDataScope {
        self.data_scope
    }

    pub const fn constraints(&self) -> AgentTaskConstraints {
        self.constraints
    }

    pub fn success_predicate(&self) -> AgentTaskSuccessPredicate {
        AgentTaskWorkflowRegistry::for_kind(self.task_kind).success_predicate
    }

    /// Restricts terminal evidence to the Rust-owned sources declared for this exact workflow.
    /// An unavailable observation may omit its source, but a present source must still match.
    pub const fn accepts_evidence_source(&self, source: AgentTaskEvidenceSource) -> bool {
        match self.task_kind {
            AgentTaskKind::InspectSystem => {
                matches!(source, AgentTaskEvidenceSource::SystemProbe)
            }
            AgentTaskKind::InspectRuntime => {
                matches!(source, AgentTaskEvidenceSource::RuntimeCatalog)
            }
            AgentTaskKind::DiagnoseEnvironment => {
                matches!(source, AgentTaskEvidenceSource::EnvironmentDiagnostics)
            }
            AgentTaskKind::RepairEnvironment => matches!(
                source,
                AgentTaskEvidenceSource::EnvironmentDiagnostics
                    | AgentTaskEvidenceSource::RepairDiagnosticRecheck
            ),
            AgentTaskKind::AnalyzeOperationalHistory => {
                matches!(source, AgentTaskEvidenceSource::OperationalHistory)
            }
            AgentTaskKind::ObserveDeploymentHealth => {
                matches!(source, AgentTaskEvidenceSource::OperationalHealth)
            }
            AgentTaskKind::StartModel | AgentTaskKind::StopModel => matches!(
                source,
                AgentTaskEvidenceSource::RuntimeCatalog | AgentTaskEvidenceSource::RuntimeRecheck
            ),
            AgentTaskKind::ActivateRuntimeProfile => matches!(
                source,
                AgentTaskEvidenceSource::RuntimeCatalog
                    | AgentTaskEvidenceSource::RuntimeProfileRecheck
            ),
            AgentTaskKind::RemoveModel => matches!(
                source,
                AgentTaskEvidenceSource::RuntimeCatalog
                    | AgentTaskEvidenceSource::ModelLibraryRecheck
            ),
            AgentTaskKind::SearchModelCatalog => {
                matches!(source, AgentTaskEvidenceSource::ModelCatalog)
            }
            AgentTaskKind::InspectModelRepository => {
                matches!(source, AgentTaskEvidenceSource::ModelRepository)
            }
            AgentTaskKind::DownloadModel => {
                matches!(source, AgentTaskEvidenceSource::ActionPlan)
            }
            AgentTaskKind::InstallEngine | AgentTaskKind::RemoveEngine => matches!(
                source,
                AgentTaskEvidenceSource::RuntimeCatalog | AgentTaskEvidenceSource::EngineRecheck
            ),
            AgentTaskKind::InspectExternalAgent => {
                matches!(source, AgentTaskEvidenceSource::ExternalIntegrationStatus)
            }
            AgentTaskKind::ConfigureExternalAgent | AgentTaskKind::DisconnectExternalAgent => {
                matches!(
                    source,
                    AgentTaskEvidenceSource::ExternalIntegrationStatus
                        | AgentTaskEvidenceSource::IntegrationRecheck
                )
            }
            AgentTaskKind::InstallManagedExternalAgent
            | AgentTaskKind::RemoveManagedExternalAgent => matches!(
                source,
                AgentTaskEvidenceSource::ExternalIntegrationStatus
                    | AgentTaskEvidenceSource::ManagedInstallationRecheck
            ),
        }
    }

    /// Derives the complete, canonical tool capability set from the Rust-owned workflow.
    pub fn required_capabilities(&self) -> AgentCapabilitySet {
        let mut capabilities = AgentCapabilitySet::new();
        capabilities.require(self.task_kind.primary_capability());
        capabilities
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTaskSpecError {
    IncompatibleTarget,
    InvalidResourceId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTaskPhase {
    Draft,
    Clarifying,
    Inspecting,
    Planning,
    AwaitingConfirmation,
    Executing,
    Verifying,
    Completed,
    Blocked,
    Failed,
    Cancelled,
}

impl AgentTaskPhase {
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Blocked | Self::Failed | Self::Cancelled
        )
    }

    pub const fn can_transition_to(self, next: Self) -> bool {
        match self {
            Self::Draft => matches!(next, Self::Clarifying | Self::Inspecting | Self::Cancelled),
            Self::Clarifying => {
                matches!(next, Self::Inspecting | Self::Blocked | Self::Cancelled)
            }
            Self::Inspecting => matches!(
                next,
                Self::Planning | Self::Verifying | Self::Blocked | Self::Failed | Self::Cancelled
            ),
            Self::Planning => matches!(
                next,
                Self::AwaitingConfirmation
                    | Self::Verifying
                    | Self::Blocked
                    | Self::Failed
                    | Self::Cancelled
            ),
            Self::AwaitingConfirmation => {
                matches!(next, Self::Executing | Self::Planning | Self::Cancelled)
            }
            Self::Executing => matches!(next, Self::Verifying | Self::Failed | Self::Cancelled),
            Self::Verifying => matches!(
                next,
                Self::Completed | Self::Planning | Self::Blocked | Self::Failed | Self::Cancelled
            ),
            Self::Completed | Self::Blocked | Self::Failed | Self::Cancelled => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentTaskState {
    phase: AgentTaskPhase,
    checkpoint_sequence: u32,
}

impl Default for AgentTaskState {
    fn default() -> Self {
        Self {
            phase: AgentTaskPhase::Draft,
            checkpoint_sequence: 0,
        }
    }
}

impl AgentTaskState {
    pub const fn phase(self) -> AgentTaskPhase {
        self.phase
    }

    pub const fn checkpoint_sequence(self) -> u32 {
        self.checkpoint_sequence
    }

    pub fn transition(&mut self, next: AgentTaskPhase) -> Result<(), AgentTaskTransitionError> {
        if !self.phase.can_transition_to(next) {
            return Err(AgentTaskTransitionError {
                current: self.phase,
                requested: next,
            });
        }
        self.phase = next;
        self.checkpoint_sequence = self.checkpoint_sequence.saturating_add(1);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentTaskTransitionError {
    pub current: AgentTaskPhase,
    pub requested: AgentTaskPhase,
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_task_kind_has_one_workflow_with_a_bounded_mutation_policy() {
        let workflows = AgentTaskWorkflowRegistry::all();
        assert_eq!(workflows.len(), AGENT_TASK_KIND_COUNT);
        let task_kinds = workflows
            .iter()
            .map(|workflow| workflow.task_kind)
            .collect::<HashSet<_>>();
        assert_eq!(task_kinds.len(), AGENT_TASK_KIND_COUNT);
        let primary_capabilities = workflows
            .iter()
            .map(|workflow| workflow.task_kind.primary_capability())
            .collect::<HashSet<_>>();
        assert_eq!(primary_capabilities.len(), AGENT_TASK_KIND_COUNT);
        let mut controlled_task_count = 0_usize;
        let mut native_action_kinds = Vec::new();

        for workflow in workflows {
            let mutating = workflow
                .steps
                .contains(&AgentWorkflowStep::ExecuteDeterministically);
            assert_eq!(
                workflow.constraints,
                if mutating {
                    AgentTaskConstraints::controlled_mutation()
                } else {
                    AgentTaskConstraints::read_only()
                }
            );
            assert_eq!(
                workflow
                    .steps
                    .iter()
                    .filter(|step| **step == AgentWorkflowStep::AwaitNativeConfirmation)
                    .count(),
                usize::from(mutating)
            );
            assert_eq!(workflow.steps.last(), Some(&AgentWorkflowStep::Summarize));
            assert_eq!(
                workflow.task_kind.allowed_action_kinds().is_empty(),
                !mutating
            );
            if mutating {
                controlled_task_count += 1;
                for action_kind in workflow.task_kind.allowed_action_kinds() {
                    if !native_action_kinds.contains(action_kind) {
                        native_action_kinds.push(*action_kind);
                    }
                }
            }

            let primary =
                AgentCapabilityRegistry::descriptor(workflow.task_kind.primary_capability());
            assert_eq!(primary.data_scope, workflow.data_scope);
            assert_eq!(
                primary.effect == AgentCapabilityEffect::ActionPlan,
                mutating
            );
            assert_eq!(
                primary.requires_native_confirmation,
                workflow.constraints.requires_native_confirmation
            );
        }
        assert_eq!(controlled_task_count, 12);
        assert_eq!(native_action_kinds.len(), 11);
        assert_eq!(
            AgentTaskKind::RepairEnvironment.allowed_action_kinds(),
            &[
                AgentActionKind::InstallLlamaCpp,
                AgentActionKind::ConfigureExternalAgent,
                AgentActionKind::RemoveModel,
            ]
        );
    }

    #[test]
    fn external_configuration_spec_uses_registry_identity_and_integration_scope() {
        let spec = AgentTaskSpec::new(
            AgentTaskKind::ConfigureExternalAgent,
            AgentTaskTarget::external_agent(ExternalAgentIntegrationId::OpenClaw),
            AgentTaskProviderMode::Local,
        )
        .expect("external Agent configuration task");

        assert_eq!(spec.target().resource_id(), Some("openclaw"));
        assert_eq!(spec.desired_state(), AgentTaskDesiredState::Configured);
        assert_eq!(
            spec.data_scope(),
            AgentCapabilityDataScope::IntegrationMetadata
        );
        assert_eq!(
            spec.constraints(),
            AgentTaskConstraints::controlled_mutation()
        );
        assert_eq!(
            spec.required_capabilities().iter().collect::<Vec<_>>(),
            vec![
                AgentCapabilityId::InspectExternalAgent,
                AgentCapabilityId::PlanExternalAgentConfiguration,
            ]
        );
    }

    #[test]
    fn task_specs_reject_incompatible_targets_and_invalid_resource_ids() {
        assert_eq!(
            AgentTaskSpec::new(
                AgentTaskKind::ConfigureExternalAgent,
                AgentTaskTarget::runtime(),
                AgentTaskProviderMode::Local,
            ),
            Err(AgentTaskSpecError::IncompatibleTarget)
        );
        assert_eq!(
            AgentTaskSpec::new(
                AgentTaskKind::InstallManagedExternalAgent,
                AgentTaskTarget::external_agent(ExternalAgentIntegrationId::OpenClaw),
                AgentTaskProviderMode::Local,
            ),
            Err(AgentTaskSpecError::IncompatibleTarget)
        );
        assert_eq!(
            AgentTaskTarget::model(Some(" model-1".to_owned())),
            Err(AgentTaskSpecError::InvalidResourceId)
        );
    }

    #[test]
    fn controlled_task_can_pause_for_confirmation_then_verify_and_complete() {
        let mut state = AgentTaskState::default();
        for phase in [
            AgentTaskPhase::Inspecting,
            AgentTaskPhase::Planning,
            AgentTaskPhase::AwaitingConfirmation,
            AgentTaskPhase::Executing,
            AgentTaskPhase::Verifying,
            AgentTaskPhase::Completed,
        ] {
            state.transition(phase).expect("valid task transition");
        }
        assert_eq!(state.phase(), AgentTaskPhase::Completed);
        assert_eq!(state.checkpoint_sequence(), 6);
        assert!(state.phase().is_terminal());
    }

    #[test]
    fn verification_can_request_one_more_plan_without_reopening_a_terminal_task() {
        let mut state = AgentTaskState::default();
        state
            .transition(AgentTaskPhase::Inspecting)
            .expect("inspect");
        state
            .transition(AgentTaskPhase::Verifying)
            .expect("verify read result");
        state
            .transition(AgentTaskPhase::Planning)
            .expect("plan next step");
        assert_eq!(state.phase(), AgentTaskPhase::Planning);

        state
            .transition(AgentTaskPhase::Blocked)
            .expect("block task");
        assert_eq!(
            state.transition(AgentTaskPhase::Inspecting),
            Err(AgentTaskTransitionError {
                current: AgentTaskPhase::Blocked,
                requested: AgentTaskPhase::Inspecting,
            })
        );
    }

    #[test]
    fn configuration_evaluation_manifest_has_unique_scenarios_and_known_task_targets() {
        let manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../../../contracts/agent-evals/v1-config-tasks.json"
        ))
        .expect("Agent configuration evaluation manifest");
        assert_eq!(manifest["schemaVersion"], 1);
        let scenarios = manifest["scenarios"]
            .as_array()
            .expect("evaluation scenarios");
        assert!(scenarios.len() >= 20);

        let mut ids = HashSet::new();
        let mut covered_task_kinds = HashSet::new();
        for scenario in scenarios {
            let id = scenario["id"].as_str().expect("scenario id");
            assert!(ids.insert(id), "duplicate scenario id: {id}");
            let disposition = scenario["expected"]["disposition"]
                .as_str()
                .expect("expected disposition");
            assert!(matches!(
                disposition,
                "task" | "clarify" | "reject" | "resume"
            ));

            if disposition == "task" {
                let task_key = scenario["expected"]["taskKind"]
                    .as_str()
                    .expect("task scenario kind");
                let task_kind = AgentTaskKind::from_key(task_key).expect("known task kind");
                covered_task_kinds.insert(task_kind);
                if let Some(target_id) = scenario["expected"]["targetId"].as_str() {
                    assert!(
                        ExternalAgentIntegrationRegistry::by_integration_id(target_id).is_some(),
                        "unknown external Agent target: {target_id}"
                    );
                }
            }
        }

        for required in [
            AgentTaskKind::ConfigureExternalAgent,
            AgentTaskKind::DisconnectExternalAgent,
            AgentTaskKind::InstallManagedExternalAgent,
            AgentTaskKind::RemoveManagedExternalAgent,
            AgentTaskKind::RepairEnvironment,
        ] {
            assert!(covered_task_kinds.contains(&required));
        }
    }

    #[test]
    fn success_predicate_manifest_covers_all_twenty_rust_owned_workflows() {
        let manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../../../contracts/agent-evals/v6-success-predicates.json"
        ))
        .expect("success predicate evaluation manifest");
        assert_eq!(manifest["schemaVersion"], 1);
        assert_eq!(
            manifest["checkpointSchemaVersion"],
            u64::from(hal100_protocol::AGENT_TASK_CHECKPOINT_SCHEMA_VERSION)
        );
        assert_eq!(manifest["thresholds"]["workflowPredicateCoverage"], 1);
        assert_eq!(manifest["thresholds"]["maxReplanAttempts"], 1);

        let workflows = manifest["workflows"]
            .as_array()
            .expect("predicate workflows");
        assert_eq!(workflows.len(), AGENT_TASK_KIND_COUNT);
        let mut covered = HashSet::new();
        for expected in workflows {
            let task_key = expected["taskKind"].as_str().expect("task key");
            let task_kind = AgentTaskKind::from_key(task_key).expect("known task kind");
            assert!(covered.insert(task_kind), "duplicate workflow {task_key}");
            let actual = AgentTaskWorkflowRegistry::for_kind(task_kind);
            assert_eq!(
                expected["predicate"].as_str(),
                Some(actual.success_predicate.key()),
                "predicate drift for {task_key}"
            );
            assert!(
                expected["completionEvidenceSources"]
                    .as_array()
                    .is_some_and(|sources| !sources.is_empty()),
                "missing bounded evidence source for {task_key}"
            );
            for source in expected["completionEvidenceSources"]
                .as_array()
                .expect("evidence sources")
            {
                let source: AgentTaskEvidenceSource =
                    serde_json::from_value(source.clone()).expect("known evidence source");
                let spec = AgentTaskSpec::new(
                    task_kind,
                    match actual.target_kind {
                        AgentTaskTargetKind::System => AgentTaskTarget::system(),
                        AgentTaskTargetKind::Runtime => AgentTaskTarget::runtime(),
                        AgentTaskTargetKind::RuntimeProfile => AgentTaskTarget::runtime_profile(),
                        AgentTaskTargetKind::Environment => AgentTaskTarget::environment(),
                        AgentTaskTargetKind::Model => {
                            AgentTaskTarget::model(Some("fixture".into())).expect("model target")
                        }
                        AgentTaskTargetKind::ModelCatalog => AgentTaskTarget::model_catalog(),
                        AgentTaskTargetKind::LlamaCpp => AgentTaskTarget::llama_cpp(),
                        AgentTaskTargetKind::ExternalAgent => AgentTaskTarget::external_agent(
                            ExternalAgentIntegrationId::PiCodingAgent,
                        ),
                    },
                    AgentTaskProviderMode::Local,
                )
                .expect("workflow spec");
                assert!(
                    spec.accepts_evidence_source(source),
                    "unaccepted evidence source for {task_key}: {source:?}"
                );
            }
        }
        assert_eq!(covered.len(), AGENT_TASK_KIND_COUNT);
    }
}
