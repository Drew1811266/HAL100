use std::collections::BTreeSet;

use hal100_protocol::{
    ENVIRONMENT_DIAGNOSTICS_TOOL, EXTERNAL_AGENT_STATUS_TOOL, MODEL_CATALOG_SEARCH_TOOL,
    MODEL_REPOSITORY_INSPECTION_TOOL, OPERATIONAL_HEALTH_OBSERVATION_TOOL,
    OPERATIONAL_HISTORY_TOOL, PLAN_DIAGNOSTIC_REPAIR_TOOL, PLAN_ENGINE_INSTALL_TOOL,
    PLAN_ENGINE_REMOVE_TOOL, PLAN_EXTERNAL_AGENT_CONFIGURATION_TOOL,
    PLAN_EXTERNAL_AGENT_DISCONNECTION_TOOL, PLAN_EXTERNAL_AGENT_INSTALLATION_TOOL,
    PLAN_MANAGED_EXTERNAL_AGENT_REMOVAL_TOOL, PLAN_MODEL_DOWNLOAD_TOOL, PLAN_MODEL_REMOVAL_TOOL,
    PLAN_MODEL_START_TOOL, RUNTIME_CATALOG_TOOL, SYSTEM_SUMMARY_TOOL,
};

/// Stable business identities for the capabilities exposed to HAL100 Agent.
///
/// RPC versions and tool argument schemas are adapters around these identities; they are not the
/// capability model itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum AgentCapabilityId {
    InspectSystemSummary = 0,
    InspectRuntimeCatalog = 1,
    PlanModelStart = 2,
    PlanModelRemoval = 3,
    InspectEnvironmentDiagnostics = 4,
    PlanDiagnosticRepair = 5,
    PlanEngineInstall = 6,
    PlanEngineRemove = 7,
    InspectExternalAgent = 8,
    PlanExternalAgentConfiguration = 9,
    PlanExternalAgentDisconnection = 10,
    SearchModelCatalog = 11,
    InspectModelRepository = 12,
    PlanModelDownload = 13,
    InspectOperationalHistory = 14,
    ObserveOperationalHealth = 15,
    PlanExternalAgentInstallation = 16,
    PlanManagedExternalAgentRemoval = 17,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentCapabilityEffect {
    ReadOnly,
    ActionPlan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentCapabilityRisk {
    ReadOnly,
    ControlledChange,
    DestructiveChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentCapabilityDataScope {
    SystemMetadata,
    RuntimeMetadata,
    DiagnosticMetadata,
    IntegrationMetadata,
    PublicCatalogMetadata,
    OperationalMetadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentCapabilityDescriptor {
    pub id: AgentCapabilityId,
    pub tool_name: &'static str,
    pub effect: AgentCapabilityEffect,
    pub maximum_risk: AgentCapabilityRisk,
    pub data_scope: AgentCapabilityDataScope,
    pub prerequisites: &'static [AgentCapabilityId],
    pub requires_native_confirmation: bool,
}

const NO_PREREQUISITES: &[AgentCapabilityId] = &[];
const RUNTIME_CATALOG_PREREQUISITE: &[AgentCapabilityId] =
    &[AgentCapabilityId::InspectRuntimeCatalog];
const ENVIRONMENT_DIAGNOSTICS_PREREQUISITE: &[AgentCapabilityId] =
    &[AgentCapabilityId::InspectEnvironmentDiagnostics];
const EXTERNAL_AGENT_STATUS_PREREQUISITE: &[AgentCapabilityId] =
    &[AgentCapabilityId::InspectExternalAgent];
const MODEL_CATALOG_SEARCH_PREREQUISITE: &[AgentCapabilityId] =
    &[AgentCapabilityId::SearchModelCatalog];
const MODEL_REPOSITORY_INSPECTION_PREREQUISITE: &[AgentCapabilityId] =
    &[AgentCapabilityId::InspectModelRepository];

const AGENT_CAPABILITIES: [AgentCapabilityDescriptor; 18] = [
    AgentCapabilityDescriptor {
        id: AgentCapabilityId::InspectSystemSummary,
        tool_name: SYSTEM_SUMMARY_TOOL,
        effect: AgentCapabilityEffect::ReadOnly,
        maximum_risk: AgentCapabilityRisk::ReadOnly,
        data_scope: AgentCapabilityDataScope::SystemMetadata,
        prerequisites: NO_PREREQUISITES,
        requires_native_confirmation: false,
    },
    AgentCapabilityDescriptor {
        id: AgentCapabilityId::InspectRuntimeCatalog,
        tool_name: RUNTIME_CATALOG_TOOL,
        effect: AgentCapabilityEffect::ReadOnly,
        maximum_risk: AgentCapabilityRisk::ReadOnly,
        data_scope: AgentCapabilityDataScope::RuntimeMetadata,
        prerequisites: NO_PREREQUISITES,
        requires_native_confirmation: false,
    },
    AgentCapabilityDescriptor {
        id: AgentCapabilityId::PlanModelStart,
        tool_name: PLAN_MODEL_START_TOOL,
        effect: AgentCapabilityEffect::ActionPlan,
        maximum_risk: AgentCapabilityRisk::ControlledChange,
        data_scope: AgentCapabilityDataScope::RuntimeMetadata,
        prerequisites: RUNTIME_CATALOG_PREREQUISITE,
        requires_native_confirmation: true,
    },
    AgentCapabilityDescriptor {
        id: AgentCapabilityId::PlanModelRemoval,
        tool_name: PLAN_MODEL_REMOVAL_TOOL,
        effect: AgentCapabilityEffect::ActionPlan,
        maximum_risk: AgentCapabilityRisk::DestructiveChange,
        data_scope: AgentCapabilityDataScope::RuntimeMetadata,
        prerequisites: RUNTIME_CATALOG_PREREQUISITE,
        requires_native_confirmation: true,
    },
    AgentCapabilityDescriptor {
        id: AgentCapabilityId::InspectEnvironmentDiagnostics,
        tool_name: ENVIRONMENT_DIAGNOSTICS_TOOL,
        effect: AgentCapabilityEffect::ReadOnly,
        maximum_risk: AgentCapabilityRisk::ReadOnly,
        data_scope: AgentCapabilityDataScope::DiagnosticMetadata,
        prerequisites: NO_PREREQUISITES,
        requires_native_confirmation: false,
    },
    AgentCapabilityDescriptor {
        id: AgentCapabilityId::PlanDiagnosticRepair,
        tool_name: PLAN_DIAGNOSTIC_REPAIR_TOOL,
        effect: AgentCapabilityEffect::ActionPlan,
        maximum_risk: AgentCapabilityRisk::ControlledChange,
        data_scope: AgentCapabilityDataScope::DiagnosticMetadata,
        prerequisites: ENVIRONMENT_DIAGNOSTICS_PREREQUISITE,
        requires_native_confirmation: true,
    },
    AgentCapabilityDescriptor {
        id: AgentCapabilityId::PlanEngineInstall,
        tool_name: PLAN_ENGINE_INSTALL_TOOL,
        effect: AgentCapabilityEffect::ActionPlan,
        maximum_risk: AgentCapabilityRisk::ControlledChange,
        data_scope: AgentCapabilityDataScope::RuntimeMetadata,
        prerequisites: RUNTIME_CATALOG_PREREQUISITE,
        requires_native_confirmation: true,
    },
    AgentCapabilityDescriptor {
        id: AgentCapabilityId::PlanEngineRemove,
        tool_name: PLAN_ENGINE_REMOVE_TOOL,
        effect: AgentCapabilityEffect::ActionPlan,
        maximum_risk: AgentCapabilityRisk::DestructiveChange,
        data_scope: AgentCapabilityDataScope::RuntimeMetadata,
        prerequisites: RUNTIME_CATALOG_PREREQUISITE,
        requires_native_confirmation: true,
    },
    AgentCapabilityDescriptor {
        id: AgentCapabilityId::InspectExternalAgent,
        tool_name: EXTERNAL_AGENT_STATUS_TOOL,
        effect: AgentCapabilityEffect::ReadOnly,
        maximum_risk: AgentCapabilityRisk::ReadOnly,
        data_scope: AgentCapabilityDataScope::IntegrationMetadata,
        prerequisites: NO_PREREQUISITES,
        requires_native_confirmation: false,
    },
    AgentCapabilityDescriptor {
        id: AgentCapabilityId::PlanExternalAgentConfiguration,
        tool_name: PLAN_EXTERNAL_AGENT_CONFIGURATION_TOOL,
        effect: AgentCapabilityEffect::ActionPlan,
        maximum_risk: AgentCapabilityRisk::ControlledChange,
        data_scope: AgentCapabilityDataScope::IntegrationMetadata,
        prerequisites: EXTERNAL_AGENT_STATUS_PREREQUISITE,
        requires_native_confirmation: true,
    },
    AgentCapabilityDescriptor {
        id: AgentCapabilityId::PlanExternalAgentDisconnection,
        tool_name: PLAN_EXTERNAL_AGENT_DISCONNECTION_TOOL,
        effect: AgentCapabilityEffect::ActionPlan,
        maximum_risk: AgentCapabilityRisk::ControlledChange,
        data_scope: AgentCapabilityDataScope::IntegrationMetadata,
        prerequisites: EXTERNAL_AGENT_STATUS_PREREQUISITE,
        requires_native_confirmation: true,
    },
    AgentCapabilityDescriptor {
        id: AgentCapabilityId::SearchModelCatalog,
        tool_name: MODEL_CATALOG_SEARCH_TOOL,
        effect: AgentCapabilityEffect::ReadOnly,
        maximum_risk: AgentCapabilityRisk::ReadOnly,
        data_scope: AgentCapabilityDataScope::PublicCatalogMetadata,
        prerequisites: NO_PREREQUISITES,
        requires_native_confirmation: false,
    },
    AgentCapabilityDescriptor {
        id: AgentCapabilityId::InspectModelRepository,
        tool_name: MODEL_REPOSITORY_INSPECTION_TOOL,
        effect: AgentCapabilityEffect::ReadOnly,
        maximum_risk: AgentCapabilityRisk::ReadOnly,
        data_scope: AgentCapabilityDataScope::PublicCatalogMetadata,
        prerequisites: MODEL_CATALOG_SEARCH_PREREQUISITE,
        requires_native_confirmation: false,
    },
    AgentCapabilityDescriptor {
        id: AgentCapabilityId::PlanModelDownload,
        tool_name: PLAN_MODEL_DOWNLOAD_TOOL,
        effect: AgentCapabilityEffect::ActionPlan,
        maximum_risk: AgentCapabilityRisk::ControlledChange,
        data_scope: AgentCapabilityDataScope::PublicCatalogMetadata,
        prerequisites: MODEL_REPOSITORY_INSPECTION_PREREQUISITE,
        requires_native_confirmation: true,
    },
    AgentCapabilityDescriptor {
        id: AgentCapabilityId::InspectOperationalHistory,
        tool_name: OPERATIONAL_HISTORY_TOOL,
        effect: AgentCapabilityEffect::ReadOnly,
        maximum_risk: AgentCapabilityRisk::ReadOnly,
        data_scope: AgentCapabilityDataScope::OperationalMetadata,
        prerequisites: NO_PREREQUISITES,
        requires_native_confirmation: false,
    },
    AgentCapabilityDescriptor {
        id: AgentCapabilityId::ObserveOperationalHealth,
        tool_name: OPERATIONAL_HEALTH_OBSERVATION_TOOL,
        effect: AgentCapabilityEffect::ReadOnly,
        maximum_risk: AgentCapabilityRisk::ReadOnly,
        data_scope: AgentCapabilityDataScope::OperationalMetadata,
        prerequisites: NO_PREREQUISITES,
        requires_native_confirmation: false,
    },
    AgentCapabilityDescriptor {
        id: AgentCapabilityId::PlanExternalAgentInstallation,
        tool_name: PLAN_EXTERNAL_AGENT_INSTALLATION_TOOL,
        effect: AgentCapabilityEffect::ActionPlan,
        maximum_risk: AgentCapabilityRisk::ControlledChange,
        data_scope: AgentCapabilityDataScope::IntegrationMetadata,
        prerequisites: EXTERNAL_AGENT_STATUS_PREREQUISITE,
        requires_native_confirmation: true,
    },
    AgentCapabilityDescriptor {
        id: AgentCapabilityId::PlanManagedExternalAgentRemoval,
        tool_name: PLAN_MANAGED_EXTERNAL_AGENT_REMOVAL_TOOL,
        effect: AgentCapabilityEffect::ActionPlan,
        maximum_risk: AgentCapabilityRisk::DestructiveChange,
        data_scope: AgentCapabilityDataScope::IntegrationMetadata,
        prerequisites: EXTERNAL_AGENT_STATUS_PREREQUISITE,
        requires_native_confirmation: true,
    },
];

pub const AGENT_CAPABILITY_COUNT: u8 = AGENT_CAPABILITIES.len() as u8;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AgentCapabilitySet(BTreeSet<AgentCapabilityId>);

impl AgentCapabilitySet {
    pub const fn new() -> Self {
        Self(BTreeSet::new())
    }

    /// Requires a capability and closes over all of its declared prerequisites.
    pub fn require(&mut self, id: AgentCapabilityId) {
        if !self.insert(id) {
            return;
        }
        for prerequisite in AgentCapabilityRegistry::descriptor(id).prerequisites {
            self.require(*prerequisite);
        }
    }

    pub fn contains(&self, id: AgentCapabilityId) -> bool {
        self.0.contains(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = AgentCapabilityId> + '_ {
        self.0.iter().copied()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    fn insert(&mut self, id: AgentCapabilityId) -> bool {
        self.0.insert(id)
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct AgentCapabilityRegistry;

impl AgentCapabilityRegistry {
    pub const fn all() -> &'static [AgentCapabilityDescriptor] {
        &AGENT_CAPABILITIES
    }

    pub const fn descriptor(id: AgentCapabilityId) -> &'static AgentCapabilityDescriptor {
        &AGENT_CAPABILITIES[id as usize]
    }

    pub fn by_tool_name(tool_name: &str) -> Option<&'static AgentCapabilityDescriptor> {
        AGENT_CAPABILITIES
            .iter()
            .find(|descriptor| descriptor.tool_name == tool_name)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn registry_has_one_descriptor_for_every_stable_capability() {
        assert_eq!(AgentCapabilityRegistry::all().len(), 18);
        assert_eq!(usize::from(AGENT_CAPABILITY_COUNT), 18);

        let ids = AgentCapabilityRegistry::all()
            .iter()
            .map(|descriptor| descriptor.id)
            .collect::<HashSet<_>>();
        let tool_names = AgentCapabilityRegistry::all()
            .iter()
            .map(|descriptor| descriptor.tool_name)
            .collect::<HashSet<_>>();
        assert_eq!(ids.len(), 18);
        assert_eq!(tool_names.len(), 18);
        for descriptor in AgentCapabilityRegistry::all() {
            assert_eq!(
                AgentCapabilityRegistry::descriptor(descriptor.id),
                descriptor
            );
            assert_eq!(
                AgentCapabilityRegistry::by_tool_name(descriptor.tool_name),
                Some(descriptor)
            );
        }
        assert!(AgentCapabilityRegistry::by_tool_name("shell.execute").is_none());
    }

    #[test]
    fn registry_matches_the_shared_rpc_v9_tool_manifest() {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../../contracts/agent-rpc/v9-tools.json"))
                .expect("shared Agent RPC v9 tool manifest");
        let tools = manifest["tools"].as_array().expect("tool manifest array");

        assert_eq!(manifest["protocolVersion"], 9);
        assert_eq!(tools.len(), AgentCapabilityRegistry::all().len());
        for (tool, descriptor) in tools.iter().zip(AgentCapabilityRegistry::all()) {
            assert_eq!(tool["name"], descriptor.tool_name);
            assert_eq!(
                tool["effect"],
                match descriptor.effect {
                    AgentCapabilityEffect::ReadOnly => "readOnly",
                    AgentCapabilityEffect::ActionPlan => "actionPlan",
                }
            );
            let prerequisites = tool["prerequisites"]
                .as_array()
                .expect("tool prerequisites")
                .iter()
                .map(|value| value.as_str().expect("prerequisite name"))
                .collect::<Vec<_>>();
            let registered_prerequisites = descriptor
                .prerequisites
                .iter()
                .map(|id| AgentCapabilityRegistry::descriptor(*id).tool_name)
                .collect::<Vec<_>>();
            assert_eq!(prerequisites, registered_prerequisites);
            assert_eq!(
                tool["requiresNativeConfirmation"],
                descriptor.requires_native_confirmation
            );
        }
    }

    #[test]
    fn action_plans_require_native_confirmation_and_reads_do_not() {
        for descriptor in AgentCapabilityRegistry::all() {
            assert_eq!(
                descriptor.requires_native_confirmation,
                descriptor.effect == AgentCapabilityEffect::ActionPlan
            );
            if descriptor.effect == AgentCapabilityEffect::ReadOnly {
                assert_eq!(descriptor.maximum_risk, AgentCapabilityRisk::ReadOnly);
            }
        }
    }

    #[test]
    fn requirement_set_closes_over_declared_prerequisites() {
        let mut requirements = AgentCapabilitySet::new();
        requirements.require(AgentCapabilityId::PlanModelStart);
        requirements.require(AgentCapabilityId::PlanDiagnosticRepair);
        requirements.require(AgentCapabilityId::PlanExternalAgentConfiguration);

        for expected in [
            AgentCapabilityId::PlanModelStart,
            AgentCapabilityId::InspectRuntimeCatalog,
            AgentCapabilityId::PlanDiagnosticRepair,
            AgentCapabilityId::InspectEnvironmentDiagnostics,
            AgentCapabilityId::PlanExternalAgentConfiguration,
            AgentCapabilityId::InspectExternalAgent,
        ] {
            assert!(requirements.contains(expected));
        }
        assert_eq!(requirements.len(), 6);
        assert!(!requirements.contains(AgentCapabilityId::InspectSystemSummary));
    }
}
