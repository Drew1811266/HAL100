/// HAL100's private Agent runtime is a product component, not an installation of the Pi CLI.
pub const BUILT_IN_AGENT_RUNTIME: BuiltInAgentRuntimeDescriptor = BuiltInAgentRuntimeDescriptor {
    runtime_id: "hal100-agent-runtime",
    client_app_id: "hal100-agent",
    display_name: "HAL100 Agent",
    engine_name: "Pi Agent Core",
    isolation: BuiltInAgentIsolation::PrivatePerTaskProcess,
};

pub const OPENCODE_INTEGRATION: ExternalAgentIntegrationDescriptor =
    ExternalAgentIntegrationDescriptor {
        id: ExternalAgentIntegrationId::OpenCode,
        integration_id: "opencode",
        client_app_id: "opencode",
        credential_id: "opencode-gateway-key",
        display_name: "OpenCode",
        availability: ExternalAgentIntegrationAvailability::Available,
        supported_protocols: &[GatewayProtocol::OpenAiChatCompletions],
        verified_protocols: &[GatewayProtocol::OpenAiChatCompletions],
        configuration_ownership: ConfigurationOwnership::ManagedFragment,
        credential_isolation: CredentialIsolation::DedicatedGatewayKey,
        preserves_default_model: true,
    };

pub const PI_CODING_AGENT_INTEGRATION: ExternalAgentIntegrationDescriptor =
    ExternalAgentIntegrationDescriptor {
        id: ExternalAgentIntegrationId::PiCodingAgent,
        integration_id: "pi-coding-agent",
        client_app_id: "pi-coding-agent",
        credential_id: "pi-coding-agent-gateway-key",
        display_name: "Pi Coding Agent",
        availability: ExternalAgentIntegrationAvailability::Available,
        supported_protocols: &[GatewayProtocol::OpenAiChatCompletions],
        verified_protocols: &[GatewayProtocol::OpenAiChatCompletions],
        configuration_ownership: ConfigurationOwnership::ManagedFragment,
        credential_isolation: CredentialIsolation::DedicatedGatewayKey,
        preserves_default_model: true,
    };

pub const OPENCLAW_INTEGRATION: ExternalAgentIntegrationDescriptor =
    ExternalAgentIntegrationDescriptor {
        id: ExternalAgentIntegrationId::OpenClaw,
        integration_id: "openclaw",
        client_app_id: "openclaw",
        credential_id: "openclaw-gateway-key",
        display_name: "OpenClaw",
        availability: ExternalAgentIntegrationAvailability::Available,
        supported_protocols: &[
            GatewayProtocol::OpenAiChatCompletions,
            GatewayProtocol::OpenAiResponses,
            GatewayProtocol::AnthropicMessages,
        ],
        verified_protocols: &[
            GatewayProtocol::OpenAiChatCompletions,
            GatewayProtocol::OpenAiResponses,
            GatewayProtocol::AnthropicMessages,
        ],
        configuration_ownership: ConfigurationOwnership::ManagedFragment,
        credential_isolation: CredentialIsolation::DedicatedGatewayKey,
        preserves_default_model: true,
    };

pub const HERMES_AGENT_INTEGRATION: ExternalAgentIntegrationDescriptor =
    ExternalAgentIntegrationDescriptor {
        id: ExternalAgentIntegrationId::HermesAgent,
        integration_id: "hermes-agent",
        client_app_id: "hermes-agent",
        credential_id: "hermes-agent-gateway-key",
        display_name: "Hermes Agent",
        availability: ExternalAgentIntegrationAvailability::Available,
        supported_protocols: &[GatewayProtocol::OpenAiChatCompletions],
        verified_protocols: &[GatewayProtocol::OpenAiChatCompletions],
        configuration_ownership: ConfigurationOwnership::ManagedFragment,
        credential_isolation: CredentialIsolation::DedicatedGatewayKey,
        preserves_default_model: true,
    };

const EXTERNAL_AGENT_INTEGRATIONS: [ExternalAgentIntegrationDescriptor; 4] = [
    OPENCODE_INTEGRATION,
    PI_CODING_AGENT_INTEGRATION,
    OPENCLAW_INTEGRATION,
    HERMES_AGENT_INTEGRATION,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltInAgentRuntimeDescriptor {
    pub runtime_id: &'static str,
    pub client_app_id: &'static str,
    pub display_name: &'static str,
    pub engine_name: &'static str,
    pub isolation: BuiltInAgentIsolation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltInAgentIsolation {
    PrivatePerTaskProcess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExternalAgentIntegrationId {
    OpenCode,
    PiCodingAgent,
    OpenClaw,
    HermesAgent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalAgentIntegrationAvailability {
    Available,
    Planned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GatewayProtocol {
    OpenAiChatCompletions,
    OpenAiResponses,
    AnthropicMessages,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigurationOwnership {
    ManagedFragment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialIsolation {
    DedicatedGatewayKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExternalAgentIntegrationDescriptor {
    pub id: ExternalAgentIntegrationId,
    pub integration_id: &'static str,
    pub client_app_id: &'static str,
    pub credential_id: &'static str,
    pub display_name: &'static str,
    pub availability: ExternalAgentIntegrationAvailability,
    /// Protocols the upstream client can express according to its published contract.
    pub supported_protocols: &'static [GatewayProtocol],
    /// Protocols HAL100 has exercised end-to-end with the real upstream client.
    pub verified_protocols: &'static [GatewayProtocol],
    pub configuration_ownership: ConfigurationOwnership,
    pub credential_isolation: CredentialIsolation,
    pub preserves_default_model: bool,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ExternalAgentIntegrationRegistry;

impl ExternalAgentIntegrationRegistry {
    pub const fn all() -> &'static [ExternalAgentIntegrationDescriptor] {
        &EXTERNAL_AGENT_INTEGRATIONS
    }

    pub fn descriptor(
        id: ExternalAgentIntegrationId,
    ) -> &'static ExternalAgentIntegrationDescriptor {
        EXTERNAL_AGENT_INTEGRATIONS
            .iter()
            .find(|descriptor| descriptor.id == id)
            .expect("every stable external Agent integration has one descriptor")
    }

    pub fn by_integration_id(
        integration_id: &str,
    ) -> Option<&'static ExternalAgentIntegrationDescriptor> {
        EXTERNAL_AGENT_INTEGRATIONS
            .iter()
            .find(|descriptor| descriptor.integration_id == integration_id)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn external_integrations_have_unique_lifecycles_and_never_impersonate_the_runtime() {
        let mut ids = HashSet::new();
        let mut integration_ids = HashSet::new();
        let mut client_app_ids = HashSet::new();
        let mut credential_ids = HashSet::new();

        for descriptor in ExternalAgentIntegrationRegistry::all() {
            assert!(ids.insert(descriptor.id));
            assert!(integration_ids.insert(descriptor.integration_id));
            assert!(client_app_ids.insert(descriptor.client_app_id));
            assert!(credential_ids.insert(descriptor.credential_id));
            assert_ne!(descriptor.integration_id, BUILT_IN_AGENT_RUNTIME.runtime_id);
            assert_ne!(
                descriptor.client_app_id,
                BUILT_IN_AGENT_RUNTIME.client_app_id
            );
            assert!(descriptor.preserves_default_model);
            assert_eq!(
                descriptor.configuration_ownership,
                ConfigurationOwnership::ManagedFragment
            );
            assert_eq!(
                descriptor.credential_isolation,
                CredentialIsolation::DedicatedGatewayKey
            );
            assert!(
                descriptor
                    .supported_protocols
                    .contains(&GatewayProtocol::OpenAiChatCompletions)
            );
            assert!(!descriptor.verified_protocols.is_empty());
            assert!(
                descriptor
                    .verified_protocols
                    .iter()
                    .all(|protocol| descriptor.supported_protocols.contains(protocol))
            );
        }
    }

    #[test]
    fn pi_coding_agent_is_an_external_client_not_the_built_in_runtime() {
        let pi =
            ExternalAgentIntegrationRegistry::descriptor(ExternalAgentIntegrationId::PiCodingAgent);

        assert_eq!(pi.integration_id, "pi-coding-agent");
        assert_eq!(pi.client_app_id, "pi-coding-agent");
        assert_eq!(BUILT_IN_AGENT_RUNTIME.runtime_id, "hal100-agent-runtime");
        assert_eq!(BUILT_IN_AGENT_RUNTIME.client_app_id, "hal100-agent");
        assert_eq!(BUILT_IN_AGENT_RUNTIME.engine_name, "Pi Agent Core");
    }

    #[test]
    fn registry_resolves_only_declared_integration_ids() {
        assert_eq!(
            ExternalAgentIntegrationRegistry::by_integration_id("opencode"),
            Some(&OPENCODE_INTEGRATION)
        );
        assert!(ExternalAgentIntegrationRegistry::by_integration_id("pi").is_none());
        assert!(ExternalAgentIntegrationRegistry::by_integration_id("hal100-agent").is_none());
    }

    #[test]
    fn openclaw_has_verified_each_advertised_protocol() {
        let openclaw =
            ExternalAgentIntegrationRegistry::descriptor(ExternalAgentIntegrationId::OpenClaw);

        assert_eq!(openclaw.supported_protocols.len(), 3);
        assert_eq!(openclaw.verified_protocols, openclaw.supported_protocols);
    }

    #[test]
    fn all_four_external_agents_are_independently_available_after_iteration_18() {
        let integrations = ExternalAgentIntegrationRegistry::all();
        assert_eq!(integrations.len(), 4);
        assert!(integrations.iter().all(|integration| {
            integration.availability == ExternalAgentIntegrationAvailability::Available
                && integration.credential_isolation == CredentialIsolation::DedicatedGatewayKey
                && integration.configuration_ownership == ConfigurationOwnership::ManagedFragment
                && integration.preserves_default_model
        }));
        assert_eq!(
            integrations
                .iter()
                .map(|integration| integration.client_app_id)
                .collect::<HashSet<_>>(),
            HashSet::from(["opencode", "pi-coding-agent", "openclaw", "hermes-agent"])
        );
    }
}
