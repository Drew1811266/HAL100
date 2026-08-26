use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEcosystemCatalog {
    pub built_in_runtime: BuiltInAgentRuntimeSummary,
    pub integrations: Vec<ExternalAgentIntegrationSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltInAgentRuntimeSummary {
    pub runtime_id: String,
    pub client_app_id: String,
    pub display_name: String,
    pub engine_name: String,
    pub isolation_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalAgentIntegrationSummary {
    pub integration_id: String,
    pub client_app_id: String,
    pub display_name: String,
    pub availability: ExternalAgentIntegrationAvailability,
    pub supported_protocols: Vec<ExternalAgentGatewayProtocol>,
    pub verified_protocols: Vec<ExternalAgentGatewayProtocol>,
    pub preserves_default_model: bool,
    pub uses_isolated_credential: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExternalAgentIntegrationAvailability {
    Available,
    Planned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExternalAgentGatewayProtocol {
    OpenAiChatCompletions,
    OpenAiResponses,
    AnthropicMessages,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExternalAgentIntegrationState {
    NotInstalled,
    InstalledNotConfigured,
    Configured,
    NeedsRefresh,
    Conflict,
    ModifiedOutsideHal100,
    UnsupportedVersion,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalAgentModelProfile {
    pub model_id: String,
    pub display_name: String,
    pub context_window_tokens: u32,
    pub max_output_tokens: u32,
    pub input_modalities: Vec<ExternalAgentInputModality>,
    pub supports_tools: bool,
    pub supports_reasoning: bool,
    /// Changes whenever adapter-visible model capabilities change.
    pub revision: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExternalAgentInputModality {
    Text,
    Image,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalAgentDetection {
    pub integration_id: String,
    pub display_name: String,
    pub installed: bool,
    pub version: Option<String>,
    pub binary_path: Option<String>,
    pub config_path: String,
    pub config_exists: bool,
    pub integration_state: ExternalAgentIntegrationState,
    pub configured_protocol: Option<ExternalAgentGatewayProtocol>,
    pub model_profile_revision: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalAgentConfigurationPlan {
    pub plan_id: String,
    pub integration_id: String,
    pub expires_at_ms: i64,
    pub config_path: String,
    pub credential_path: String,
    pub changes: Vec<ExternalAgentConfigurationChange>,
    pub gateway_protocol: ExternalAgentGatewayProtocol,
    pub creates_backup: bool,
    pub preserves_default_model: bool,
    pub requires_confirmation: bool,
    pub model_profile_revision: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalAgentConfigurationChange {
    pub path: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalAgentConfigurationResult {
    pub configured: bool,
    pub integration_id: String,
    pub config_path: String,
    pub backup_path: Option<String>,
    pub credential_prefix: String,
    pub model_profile_revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalAgentDisconnectPlan {
    pub plan_id: String,
    pub integration_id: String,
    pub expires_at_ms: i64,
    pub config_path: String,
    pub credential_path: String,
    pub changes: Vec<ExternalAgentManagedChange>,
    pub creates_backup: bool,
    pub revokes_credential: bool,
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalAgentManagedChange {
    pub path: String,
    pub action: ExternalAgentManagedChangeAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExternalAgentManagedChangeAction {
    RemoveManagedFragment,
    RemoveManagedCredential,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalAgentDisconnectResult {
    pub disconnected: bool,
    pub integration_id: String,
    pub config_path: String,
    pub backup_path: Option<String>,
    pub credential_revoked: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ecosystem_catalog_uses_stable_desktop_wire_names() {
        let catalog = AgentEcosystemCatalog {
            built_in_runtime: BuiltInAgentRuntimeSummary {
                runtime_id: "hal100-agent-runtime".to_owned(),
                client_app_id: "hal100-agent".to_owned(),
                display_name: "HAL100 Agent".to_owned(),
                engine_name: "Pi Agent Core".to_owned(),
                isolation_summary: "private".to_owned(),
            },
            integrations: vec![ExternalAgentIntegrationSummary {
                integration_id: "pi-coding-agent".to_owned(),
                client_app_id: "pi-coding-agent".to_owned(),
                display_name: "Pi Coding Agent".to_owned(),
                availability: ExternalAgentIntegrationAvailability::Planned,
                supported_protocols: vec![ExternalAgentGatewayProtocol::OpenAiChatCompletions],
                verified_protocols: vec![ExternalAgentGatewayProtocol::OpenAiChatCompletions],
                preserves_default_model: true,
                uses_isolated_credential: true,
            }],
        };

        let value = serde_json::to_value(catalog).expect("catalog JSON");
        assert_eq!(value["builtInRuntime"]["runtimeId"], "hal100-agent-runtime");
        assert_eq!(value["integrations"][0]["availability"], "planned");
        assert_eq!(
            value["integrations"][0]["supportedProtocols"][0],
            "openAiChatCompletions"
        );
        assert_eq!(
            value["integrations"][0]["verifiedProtocols"][0],
            "openAiChatCompletions"
        );
        assert_eq!(value["integrations"][0]["preservesDefaultModel"], true);
        assert_eq!(value["integrations"][0]["usesIsolatedCredential"], true);
    }

    #[test]
    fn model_profile_uses_explicit_non_default_capabilities() {
        let profile = ExternalAgentModelProfile {
            model_id: "hal100-active".to_owned(),
            display_name: "HAL100 当前模型".to_owned(),
            context_window_tokens: 16_384,
            max_output_tokens: 768,
            input_modalities: vec![ExternalAgentInputModality::Text],
            supports_tools: true,
            supports_reasoning: false,
            revision: "managed-local-v1".to_owned(),
        };

        let value = serde_json::to_value(profile).expect("profile JSON");
        assert_eq!(value["contextWindowTokens"], 16_384);
        assert_eq!(value["maxOutputTokens"], 768);
        assert_eq!(value["inputModalities"][0], "text");
    }

    #[test]
    fn disconnect_plan_never_contains_secret_material() {
        let plan = ExternalAgentDisconnectPlan {
            plan_id: "plan-1".to_owned(),
            integration_id: "opencode".to_owned(),
            expires_at_ms: 123,
            config_path: "~/.config/opencode/opencode.json".to_owned(),
            credential_path: "~/Library/Application Support/HAL100/credentials/key".to_owned(),
            changes: vec![ExternalAgentManagedChange {
                path: "provider.hal100".to_owned(),
                action: ExternalAgentManagedChangeAction::RemoveManagedFragment,
            }],
            creates_backup: true,
            revokes_credential: true,
            requires_confirmation: true,
        };

        let json = serde_json::to_string(&plan).expect("disconnect JSON");
        assert!(json.contains("removeManagedFragment"));
        assert!(!json.contains("hal100_open_"));
    }

    #[test]
    fn generic_configuration_plan_carries_the_model_contract_revision() {
        let plan = ExternalAgentConfigurationPlan {
            plan_id: "plan-1".to_owned(),
            integration_id: "pi-coding-agent".to_owned(),
            expires_at_ms: 123,
            config_path: "~/.pi/agent/models.json".to_owned(),
            credential_path: "~/Library/Application Support/HAL100/credentials/pi.key".to_owned(),
            changes: vec![ExternalAgentConfigurationChange {
                path: "providers.hal100.models[0]".to_owned(),
                value: "HAL100 当前模型".to_owned(),
            }],
            gateway_protocol: ExternalAgentGatewayProtocol::OpenAiChatCompletions,
            creates_backup: true,
            preserves_default_model: true,
            requires_confirmation: true,
            model_profile_revision: "managed-route-v3".to_owned(),
            warnings: Vec::new(),
        };

        let value = serde_json::to_value(plan).expect("configuration plan JSON");
        assert_eq!(value["integrationId"], "pi-coding-agent");
        assert_eq!(value["modelProfileRevision"], "managed-route-v3");
        assert!(value.to_string().find("hal100_pi_").is_none());
    }
}
