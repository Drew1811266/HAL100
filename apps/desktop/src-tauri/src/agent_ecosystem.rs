use hal100_core::{
    BUILT_IN_AGENT_RUNTIME, BuiltInAgentIsolation, CredentialIsolation,
    ExternalAgentIntegrationAvailability as CoreAvailability, ExternalAgentIntegrationRegistry,
    GatewayProtocol as CoreGatewayProtocol,
};
use hal100_protocol::{
    AgentEcosystemCatalog, BuiltInAgentRuntimeSummary, ExternalAgentGatewayProtocol,
    ExternalAgentIntegrationAvailability, ExternalAgentIntegrationSummary,
};

pub(super) fn catalog() -> AgentEcosystemCatalog {
    AgentEcosystemCatalog {
        built_in_runtime: BuiltInAgentRuntimeSummary {
            runtime_id: BUILT_IN_AGENT_RUNTIME.runtime_id.to_owned(),
            client_app_id: BUILT_IN_AGENT_RUNTIME.client_app_id.to_owned(),
            display_name: BUILT_IN_AGENT_RUNTIME.display_name.to_owned(),
            engine_name: BUILT_IN_AGENT_RUNTIME.engine_name.to_owned(),
            isolation_summary: match BUILT_IN_AGENT_RUNTIME.isolation {
                BuiltInAgentIsolation::PrivatePerTaskProcess => {
                    "HAL100 私有的按任务进程、临时 HOME、会话和凭据".to_owned()
                }
            },
        },
        integrations: ExternalAgentIntegrationRegistry::all()
            .iter()
            .map(|descriptor| ExternalAgentIntegrationSummary {
                integration_id: descriptor.integration_id.to_owned(),
                client_app_id: descriptor.client_app_id.to_owned(),
                display_name: descriptor.display_name.to_owned(),
                availability: match descriptor.availability {
                    CoreAvailability::Available => ExternalAgentIntegrationAvailability::Available,
                    CoreAvailability::Planned => ExternalAgentIntegrationAvailability::Planned,
                },
                supported_protocols: descriptor
                    .supported_protocols
                    .iter()
                    .copied()
                    .map(map_protocol)
                    .collect(),
                verified_protocols: descriptor
                    .verified_protocols
                    .iter()
                    .copied()
                    .map(map_protocol)
                    .collect(),
                preserves_default_model: descriptor.preserves_default_model,
                uses_isolated_credential: matches!(
                    descriptor.credential_isolation,
                    CredentialIsolation::DedicatedGatewayKey
                ),
            })
            .collect(),
    }
}

fn map_protocol(protocol: CoreGatewayProtocol) -> ExternalAgentGatewayProtocol {
    match protocol {
        CoreGatewayProtocol::OpenAiChatCompletions => {
            ExternalAgentGatewayProtocol::OpenAiChatCompletions
        }
        CoreGatewayProtocol::OpenAiResponses => ExternalAgentGatewayProtocol::OpenAiResponses,
        CoreGatewayProtocol::AnthropicMessages => ExternalAgentGatewayProtocol::AnthropicMessages,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_catalog_keeps_the_runtime_and_external_pi_visibly_separate() {
        let catalog = catalog();
        let pi = catalog
            .integrations
            .iter()
            .find(|integration| integration.integration_id == "pi-coding-agent")
            .expect("Pi Coding Agent descriptor");

        assert_eq!(catalog.built_in_runtime.runtime_id, "hal100-agent-runtime");
        assert_eq!(catalog.built_in_runtime.client_app_id, "hal100-agent");
        assert_eq!(catalog.built_in_runtime.engine_name, "Pi Agent Core");
        assert_eq!(pi.client_app_id, "pi-coding-agent");
        assert_eq!(
            pi.availability,
            ExternalAgentIntegrationAvailability::Available
        );
        assert!(pi.preserves_default_model);
        assert!(pi.uses_isolated_credential);
        assert_eq!(pi.supported_protocols, pi.verified_protocols);
    }

    #[test]
    fn desktop_catalog_exposes_every_completed_adapter_as_available() {
        let catalog = catalog();
        assert_eq!(catalog.integrations.len(), 4);
        assert!(catalog.integrations.iter().all(|integration| {
            integration.availability == ExternalAgentIntegrationAvailability::Available
                && integration.uses_isolated_credential
                && integration.preserves_default_model
                && !integration.verified_protocols.is_empty()
        }));
    }
}
