use hal100_protocol::{ToolCallErrorPayload, ToolCallRequestPayload, ToolCallResultPayload};
use serde_json::{Map, Value, json};

use crate::{
    AgentCapabilityId, AgentCapabilityRegistry, ExternalAgentIntegrationId,
    ExternalAgentIntegrationRegistry,
};

const MAX_CORRELATION_ID_BYTES: usize = 128;

/// Iteration 1 broker used to prove that Pi can only request tools through Rust.
/// It performs no system calls and always returns a fixed, explicitly simulated summary.
#[derive(Debug, Default, Clone, Copy)]
pub struct SimulatedToolBroker;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizedAgentTool {
    InspectSystemSummary,
    InspectRuntimeCatalog,
    PlanModelStart {
        model_id: String,
    },
    PlanModelRemoval {
        model_id: String,
    },
    InspectEnvironmentDiagnostics,
    InspectOperationalHistory,
    ObserveOperationalHealth,
    PlanDiagnosticRepair {
        report_id: String,
        finding_id: String,
    },
    PlanEngineInstall,
    PlanEngineRemove,
    InspectExternalAgent {
        integration_id: ExternalAgentIntegrationId,
    },
    PlanExternalAgentConfiguration {
        integration_id: ExternalAgentIntegrationId,
    },
    PlanExternalAgentDisconnection {
        integration_id: ExternalAgentIntegrationId,
    },
    PlanExternalAgentInstallation {
        integration_id: ExternalAgentIntegrationId,
    },
    PlanManagedExternalAgentRemoval {
        integration_id: ExternalAgentIntegrationId,
    },
    SearchModelCatalog {
        query: String,
    },
    InspectModelRepository {
        repository: String,
    },
    PlanModelDownload {
        remote_path: String,
    },
}

#[derive(Debug, Default, Clone, Copy)]
pub struct AgentToolPolicy;

impl AgentToolPolicy {
    pub fn authorize(
        &self,
        request: &ToolCallRequestPayload,
    ) -> Result<AuthorizedAgentTool, ToolCallErrorPayload> {
        if !is_valid_correlation_id(&request.run_id)
            || !is_valid_correlation_id(&request.tool_call_id)
        {
            return Err(ToolCallErrorPayload {
                code: "invalid_correlation".to_owned(),
                message: "runId and toolCallId must be between 1 and 128 bytes".to_owned(),
            });
        }
        let capability =
            AgentCapabilityRegistry::by_tool_name(&request.tool_name).ok_or_else(|| {
                ToolCallErrorPayload {
                    code: "tool_not_allowed".to_owned(),
                    message: "tool is not present in the Rust capability registry".to_owned(),
                }
            })?;
        match capability.id {
            AgentCapabilityId::InspectSystemSummary => {
                require_exact_summary_arguments(&request.arguments)?;
                Ok(AuthorizedAgentTool::InspectSystemSummary)
            }
            AgentCapabilityId::InspectRuntimeCatalog => {
                require_exact_summary_arguments(&request.arguments)?;
                Ok(AuthorizedAgentTool::InspectRuntimeCatalog)
            }
            AgentCapabilityId::PlanModelStart => Ok(AuthorizedAgentTool::PlanModelStart {
                model_id: exact_model_id(&request.arguments)?,
            }),
            AgentCapabilityId::PlanModelRemoval => Ok(AuthorizedAgentTool::PlanModelRemoval {
                model_id: exact_model_id(&request.arguments)?,
            }),
            AgentCapabilityId::InspectEnvironmentDiagnostics => {
                require_exact_target_arguments(&request.arguments, "full")?;
                Ok(AuthorizedAgentTool::InspectEnvironmentDiagnostics)
            }
            AgentCapabilityId::InspectOperationalHistory => {
                require_exact_target_arguments(&request.arguments, "recent")?;
                Ok(AuthorizedAgentTool::InspectOperationalHistory)
            }
            AgentCapabilityId::ObserveOperationalHealth => {
                require_exact_operational_observation_arguments(&request.arguments)?;
                Ok(AuthorizedAgentTool::ObserveOperationalHealth)
            }
            AgentCapabilityId::PlanDiagnosticRepair => {
                let (report_id, finding_id) = exact_diagnostic_repair_ids(&request.arguments)?;
                Ok(AuthorizedAgentTool::PlanDiagnosticRepair {
                    report_id,
                    finding_id,
                })
            }
            AgentCapabilityId::PlanEngineInstall => {
                require_exact_target_arguments(&request.arguments, "llama.cpp")?;
                Ok(AuthorizedAgentTool::PlanEngineInstall)
            }
            AgentCapabilityId::PlanEngineRemove => {
                require_exact_target_arguments(&request.arguments, "llama.cpp")?;
                Ok(AuthorizedAgentTool::PlanEngineRemove)
            }
            AgentCapabilityId::InspectExternalAgent => {
                Ok(AuthorizedAgentTool::InspectExternalAgent {
                    integration_id: exact_external_agent_integration(&request.arguments)?,
                })
            }
            AgentCapabilityId::PlanExternalAgentConfiguration => {
                Ok(AuthorizedAgentTool::PlanExternalAgentConfiguration {
                    integration_id: exact_external_agent_integration(&request.arguments)?,
                })
            }
            AgentCapabilityId::PlanExternalAgentDisconnection => {
                Ok(AuthorizedAgentTool::PlanExternalAgentDisconnection {
                    integration_id: exact_external_agent_integration(&request.arguments)?,
                })
            }
            AgentCapabilityId::PlanExternalAgentInstallation => {
                Ok(AuthorizedAgentTool::PlanExternalAgentInstallation {
                    integration_id: exact_external_agent_integration(&request.arguments)?,
                })
            }
            AgentCapabilityId::PlanManagedExternalAgentRemoval => {
                Ok(AuthorizedAgentTool::PlanManagedExternalAgentRemoval {
                    integration_id: exact_external_agent_integration(&request.arguments)?,
                })
            }
            AgentCapabilityId::SearchModelCatalog => Ok(AuthorizedAgentTool::SearchModelCatalog {
                query: exact_bounded_string(&request.arguments, "query", 2, 100)?,
            }),
            AgentCapabilityId::InspectModelRepository => {
                let repository = exact_bounded_string(&request.arguments, "repository", 3, 200)?;
                if !is_safe_repository(&repository) {
                    return Err(invalid_arguments(
                        "expected exactly one public owner/name repository string",
                    ));
                }
                Ok(AuthorizedAgentTool::InspectModelRepository { repository })
            }
            AgentCapabilityId::PlanModelDownload => {
                let remote_path = exact_bounded_string(&request.arguments, "remotePath", 1, 512)?;
                if !is_safe_remote_path(&remote_path) {
                    return Err(invalid_arguments(
                        "expected exactly one safe relative remotePath string",
                    ));
                }
                Ok(AuthorizedAgentTool::PlanModelDownload { remote_path })
            }
        }
    }
}

fn exact_external_agent_integration(
    arguments: &Value,
) -> Result<ExternalAgentIntegrationId, ToolCallErrorPayload> {
    let integration_id = exact_bounded_string(arguments, "integrationId", 1, 64)?;
    ExternalAgentIntegrationRegistry::by_integration_id(&integration_id)
        .map(|descriptor| descriptor.id)
        .ok_or_else(|| {
            invalid_arguments("integrationId must name a registered external Agent integration")
        })
}

impl SimulatedToolBroker {
    pub fn execute(&self, request: &ToolCallRequestPayload) -> ToolCallResultPayload {
        match AgentToolPolicy.authorize(request) {
            Ok(AuthorizedAgentTool::InspectSystemSummary) => {}
            Ok(_) => {
                return ToolCallResultPayload::error(
                    &request.tool_call_id,
                    "simulation_tool_not_supported",
                    "iteration-1 simulated broker only supports the system summary fixture",
                );
            }
            Err(error) => {
                return ToolCallResultPayload::error(
                    &request.tool_call_id,
                    error.code,
                    error.message,
                );
            }
        }

        ToolCallResultPayload::success(
            &request.tool_call_id,
            json!({
                "source": "rust_simulated_broker",
                "platform": "macos",
                "architecture": "arm64",
                "supported": true,
                "simulated": true
            }),
        )
    }
}

fn is_valid_correlation_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_CORRELATION_ID_BYTES
}

fn require_exact_summary_arguments(arguments: &Value) -> Result<(), ToolCallErrorPayload> {
    let expected = Map::from_iter([("detail".to_owned(), Value::String("summary".to_owned()))]);
    if arguments.as_object() == Some(&expected) {
        Ok(())
    } else {
        Err(ToolCallErrorPayload {
            code: "invalid_arguments".to_owned(),
            message: "expected exactly {\"detail\":\"summary\"}".to_owned(),
        })
    }
}

fn exact_model_id(arguments: &Value) -> Result<String, ToolCallErrorPayload> {
    let Some(arguments) = arguments.as_object() else {
        return Err(invalid_model_id_arguments());
    };
    if arguments.len() != 1 {
        return Err(invalid_model_id_arguments());
    }
    let Some(model_id) = arguments.get("modelId").and_then(Value::as_str) else {
        return Err(invalid_model_id_arguments());
    };
    if model_id.is_empty() || model_id.len() > 128 || model_id.chars().any(char::is_control) {
        return Err(invalid_model_id_arguments());
    }
    Ok(model_id.to_owned())
}

fn require_exact_target_arguments(
    arguments: &Value,
    expected_target: &str,
) -> Result<(), ToolCallErrorPayload> {
    let expected = Map::from_iter([(
        "target".to_owned(),
        Value::String(expected_target.to_owned()),
    )]);
    if arguments.as_object() == Some(&expected) {
        Ok(())
    } else {
        Err(ToolCallErrorPayload {
            code: "invalid_arguments".to_owned(),
            message: format!("expected exactly {{\"target\":\"{expected_target}\"}}"),
        })
    }
}

fn require_exact_operational_observation_arguments(
    arguments: &Value,
) -> Result<(), ToolCallErrorPayload> {
    let expected = Map::from_iter([
        ("target".to_owned(), Value::String("deployment".to_owned())),
        ("sampleCount".to_owned(), Value::from(3)),
    ]);
    if arguments.as_object() == Some(&expected) {
        Ok(())
    } else {
        Err(ToolCallErrorPayload {
            code: "invalid_arguments".to_owned(),
            message: "expected exactly {\"target\":\"deployment\",\"sampleCount\":3}".to_owned(),
        })
    }
}

fn exact_diagnostic_repair_ids(
    arguments: &Value,
) -> Result<(String, String), ToolCallErrorPayload> {
    let Some(arguments) = arguments.as_object() else {
        return Err(invalid_diagnostic_repair_arguments());
    };
    if arguments.len() != 2 {
        return Err(invalid_diagnostic_repair_arguments());
    }
    let Some(report_id) = arguments.get("reportId").and_then(Value::as_str) else {
        return Err(invalid_diagnostic_repair_arguments());
    };
    let Some(finding_id) = arguments.get("findingId").and_then(Value::as_str) else {
        return Err(invalid_diagnostic_repair_arguments());
    };
    if !is_valid_correlation_id(report_id) || !is_valid_correlation_id(finding_id) {
        return Err(invalid_diagnostic_repair_arguments());
    }
    Ok((report_id.to_owned(), finding_id.to_owned()))
}

fn invalid_diagnostic_repair_arguments() -> ToolCallErrorPayload {
    ToolCallErrorPayload {
        code: "invalid_arguments".to_owned(),
        message: "expected exactly bounded reportId and findingId strings".to_owned(),
    }
}

fn invalid_model_id_arguments() -> ToolCallErrorPayload {
    ToolCallErrorPayload {
        code: "invalid_arguments".to_owned(),
        message: "expected exactly one bounded modelId string".to_owned(),
    }
}

fn exact_bounded_string(
    arguments: &Value,
    key: &str,
    minimum_chars: usize,
    maximum_chars: usize,
) -> Result<String, ToolCallErrorPayload> {
    let Some(object) = arguments.as_object() else {
        return Err(invalid_arguments("expected exactly one bounded string"));
    };
    let Some(value) = (object.len() == 1)
        .then(|| object.get(key))
        .flatten()
        .and_then(Value::as_str)
    else {
        return Err(invalid_arguments("expected exactly one bounded string"));
    };
    let character_count = value.chars().count();
    if !(minimum_chars..=maximum_chars).contains(&character_count)
        || value.chars().any(char::is_control)
        || value.trim() != value
    {
        return Err(invalid_arguments("expected exactly one bounded string"));
    }
    Ok(value.to_owned())
}

fn is_safe_repository(repository: &str) -> bool {
    let mut components = repository.split('/');
    let valid_component = |component: &str| {
        !component.is_empty()
            && component != "."
            && component != ".."
            && component
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "-_.".contains(character))
    };
    components.next().is_some_and(valid_component)
        && components.next().is_some_and(valid_component)
        && components.next().is_none()
}

fn is_safe_remote_path(remote_path: &str) -> bool {
    !remote_path.starts_with('/')
        && !remote_path.contains('\\')
        && remote_path
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

fn invalid_arguments(message: &str) -> ToolCallErrorPayload {
    ToolCallErrorPayload {
        code: "invalid_arguments".to_owned(),
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use hal100_protocol::{
        ENVIRONMENT_DIAGNOSTICS_TOOL, EXTERNAL_AGENT_STATUS_TOOL, MODEL_CATALOG_SEARCH_TOOL,
        MODEL_REPOSITORY_INSPECTION_TOOL, PLAN_DIAGNOSTIC_REPAIR_TOOL, PLAN_ENGINE_INSTALL_TOOL,
        PLAN_ENGINE_REMOVE_TOOL, PLAN_EXTERNAL_AGENT_CONFIGURATION_TOOL,
        PLAN_EXTERNAL_AGENT_DISCONNECTION_TOOL, PLAN_EXTERNAL_AGENT_INSTALLATION_TOOL,
        PLAN_MANAGED_EXTERNAL_AGENT_REMOVAL_TOOL, PLAN_MODEL_DOWNLOAD_TOOL,
        PLAN_MODEL_REMOVAL_TOOL, PLAN_MODEL_START_TOOL, RUNTIME_CATALOG_TOOL, SYSTEM_SUMMARY_TOOL,
        ToolCallResultStatus,
    };

    use super::*;

    fn allowed_request(arguments: Value) -> ToolCallRequestPayload {
        ToolCallRequestPayload {
            run_id: "simulation-1".to_owned(),
            tool_call_id: "pi-tool-1".to_owned(),
            tool_name: SYSTEM_SUMMARY_TOOL.to_owned(),
            arguments,
        }
    }

    #[test]
    fn returns_only_the_fixed_simulated_summary_for_the_allowlisted_tool() {
        let result = SimulatedToolBroker.execute(&allowed_request(json!({ "detail": "summary" })));

        assert_eq!(result.status, ToolCallResultStatus::Success);
        assert_eq!(result.output.expect("fixed output")["simulated"], true);
        assert!(result.error.is_none());
    }

    #[test]
    fn rejects_unknown_tools() {
        let mut request = allowed_request(json!({ "detail": "summary" }));
        request.tool_name = "shell.execute".to_owned();

        let result = SimulatedToolBroker.execute(&request);

        assert_eq!(result.status, ToolCallResultStatus::Error);
        assert_eq!(result.error.expect("policy error").code, "tool_not_allowed");
    }

    #[test]
    fn rejects_extra_or_changed_arguments() {
        for arguments in [
            json!({ "detail": "full" }),
            json!({ "detail": "summary", "path": "/" }),
            json!({}),
        ] {
            let result = SimulatedToolBroker.execute(&allowed_request(arguments));
            assert_eq!(result.status, ToolCallResultStatus::Error);
            assert_eq!(
                result.error.expect("argument error").code,
                "invalid_arguments"
            );
        }
    }

    #[test]
    fn rejects_invalid_correlation_ids_before_tool_dispatch() {
        let mut request = allowed_request(json!({ "detail": "summary" }));
        request.run_id.clear();

        let result = SimulatedToolBroker.execute(&request);

        assert_eq!(result.status, ToolCallResultStatus::Error);
        assert_eq!(
            result.error.expect("correlation error").code,
            "invalid_correlation"
        );
    }

    #[test]
    fn authorizes_runtime_inspection_and_exact_model_plans() {
        let mut request = allowed_request(json!({ "detail": "summary" }));
        request.tool_name = RUNTIME_CATALOG_TOOL.to_owned();
        assert_eq!(
            AgentToolPolicy.authorize(&request).expect("catalog tool"),
            AuthorizedAgentTool::InspectRuntimeCatalog
        );

        request.tool_name = PLAN_MODEL_START_TOOL.to_owned();
        request.arguments = json!({ "modelId": "managed-model-1" });
        assert_eq!(
            AgentToolPolicy
                .authorize(&request)
                .expect("model plan tool"),
            AuthorizedAgentTool::PlanModelStart {
                model_id: "managed-model-1".to_owned()
            }
        );

        request.tool_name = PLAN_MODEL_REMOVAL_TOOL.to_owned();
        assert_eq!(
            AgentToolPolicy
                .authorize(&request)
                .expect("model removal plan tool"),
            AuthorizedAgentTool::PlanModelRemoval {
                model_id: "managed-model-1".to_owned()
            }
        );

        request.tool_name = ENVIRONMENT_DIAGNOSTICS_TOOL.to_owned();
        request.arguments = json!({ "target": "full" });
        assert_eq!(
            AgentToolPolicy
                .authorize(&request)
                .expect("diagnostic tool"),
            AuthorizedAgentTool::InspectEnvironmentDiagnostics
        );

        request.tool_name = PLAN_DIAGNOSTIC_REPAIR_TOOL.to_owned();
        request.arguments = json!({
            "reportId": "diagnostic-1",
            "findingId": "finding-1"
        });
        assert_eq!(
            AgentToolPolicy
                .authorize(&request)
                .expect("diagnostic repair tool"),
            AuthorizedAgentTool::PlanDiagnosticRepair {
                report_id: "diagnostic-1".to_owned(),
                finding_id: "finding-1".to_owned(),
            }
        );
    }

    #[test]
    fn authorizes_only_exact_bounded_model_discovery_arguments() {
        let mut request = allowed_request(json!({ "query": "Qwen GGUF" }));
        request.tool_name = MODEL_CATALOG_SEARCH_TOOL.to_owned();
        assert_eq!(
            AgentToolPolicy.authorize(&request).expect("catalog search"),
            AuthorizedAgentTool::SearchModelCatalog {
                query: "Qwen GGUF".to_owned()
            }
        );

        request.tool_name = MODEL_REPOSITORY_INSPECTION_TOOL.to_owned();
        request.arguments = json!({ "repository": "Qwen/Qwen3-GGUF" });
        assert_eq!(
            AgentToolPolicy
                .authorize(&request)
                .expect("repository inspection"),
            AuthorizedAgentTool::InspectModelRepository {
                repository: "Qwen/Qwen3-GGUF".to_owned()
            }
        );

        request.tool_name = PLAN_MODEL_DOWNLOAD_TOOL.to_owned();
        request.arguments = json!({ "remotePath": "Qwen3-Q4_K_M.gguf" });
        assert_eq!(
            AgentToolPolicy.authorize(&request).expect("download plan"),
            AuthorizedAgentTool::PlanModelDownload {
                remote_path: "Qwen3-Q4_K_M.gguf".to_owned()
            }
        );

        for arguments in [
            json!({ "remotePath": "../secret" }),
            json!({ "remotePath": "/absolute.gguf" }),
            json!({ "remotePath": "safe.gguf", "extra": true }),
        ] {
            request.arguments = arguments;
            assert_eq!(
                AgentToolPolicy
                    .authorize(&request)
                    .expect_err("unsafe path")
                    .code,
                "invalid_arguments"
            );
        }
    }

    #[test]
    fn model_plan_rejects_extra_fields_or_unbounded_identifiers() {
        for arguments in [
            json!({ "modelId": "managed-model-1", "force": true }),
            json!({ "modelId": "" }),
            json!({ "modelId": "x".repeat(129) }),
        ] {
            let mut request = allowed_request(arguments);
            request.tool_name = PLAN_MODEL_START_TOOL.to_owned();
            let error = AgentToolPolicy
                .authorize(&request)
                .expect_err("invalid model plan arguments");
            assert_eq!(error.code, "invalid_arguments");
        }

        for arguments in [
            json!({ "reportId": "diagnostic-1" }),
            json!({ "reportId": "diagnostic-1", "findingId": "" }),
            json!({ "reportId": "diagnostic-1", "findingId": "finding-1", "force": true }),
        ] {
            let mut request = allowed_request(arguments);
            request.tool_name = PLAN_DIAGNOSTIC_REPAIR_TOOL.to_owned();
            let error = AgentToolPolicy
                .authorize(&request)
                .expect_err("invalid diagnostic repair arguments");
            assert_eq!(error.code, "invalid_arguments");
        }
    }

    #[test]
    fn authorizes_only_exact_engine_targets() {
        for (tool_name, target, expected) in [
            (
                PLAN_ENGINE_INSTALL_TOOL,
                "llama.cpp",
                AuthorizedAgentTool::PlanEngineInstall,
            ),
            (
                PLAN_ENGINE_REMOVE_TOOL,
                "llama.cpp",
                AuthorizedAgentTool::PlanEngineRemove,
            ),
        ] {
            let mut request = allowed_request(json!({ "target": target }));
            request.tool_name = tool_name.to_owned();
            assert_eq!(
                AgentToolPolicy.authorize(&request).expect("allowed"),
                expected
            );

            request.arguments = json!({ "target": target, "force": true });
            assert_eq!(
                AgentToolPolicy
                    .authorize(&request)
                    .expect_err("extra field")
                    .code,
                "invalid_arguments"
            );
        }
    }

    #[test]
    fn authorizes_only_registered_external_agent_integrations() {
        for (tool_name, expected) in [
            (
                EXTERNAL_AGENT_STATUS_TOOL,
                AuthorizedAgentTool::InspectExternalAgent {
                    integration_id: ExternalAgentIntegrationId::PiCodingAgent,
                },
            ),
            (
                PLAN_EXTERNAL_AGENT_CONFIGURATION_TOOL,
                AuthorizedAgentTool::PlanExternalAgentConfiguration {
                    integration_id: ExternalAgentIntegrationId::PiCodingAgent,
                },
            ),
            (
                PLAN_EXTERNAL_AGENT_DISCONNECTION_TOOL,
                AuthorizedAgentTool::PlanExternalAgentDisconnection {
                    integration_id: ExternalAgentIntegrationId::PiCodingAgent,
                },
            ),
            (
                PLAN_EXTERNAL_AGENT_INSTALLATION_TOOL,
                AuthorizedAgentTool::PlanExternalAgentInstallation {
                    integration_id: ExternalAgentIntegrationId::PiCodingAgent,
                },
            ),
            (
                PLAN_MANAGED_EXTERNAL_AGENT_REMOVAL_TOOL,
                AuthorizedAgentTool::PlanManagedExternalAgentRemoval {
                    integration_id: ExternalAgentIntegrationId::PiCodingAgent,
                },
            ),
        ] {
            let mut request = allowed_request(json!({ "integrationId": "pi-coding-agent" }));
            request.tool_name = tool_name.to_owned();
            assert_eq!(
                AgentToolPolicy.authorize(&request).expect("allowed"),
                expected
            );

            for arguments in [
                json!({ "integrationId": "unknown-agent" }),
                json!({ "integrationId": "pi-coding-agent", "force": true }),
            ] {
                request.arguments = arguments;
                assert_eq!(
                    AgentToolPolicy
                        .authorize(&request)
                        .expect_err("rejected")
                        .code,
                    "invalid_arguments"
                );
            }
        }
    }

    #[test]
    fn rust_argument_policy_matches_shared_v9_fixtures() {
        let manifest: Value =
            serde_json::from_str(include_str!("../../../contracts/agent-rpc/v9-tools.json"))
                .expect("shared Agent RPC v9 tool policy");
        let tools = manifest["tools"].as_array().expect("tool policy array");
        assert_eq!(tools.len(), AgentCapabilityRegistry::all().len());

        for tool in tools {
            let tool_name = tool["name"].as_str().expect("tool name");
            let request = ToolCallRequestPayload {
                run_id: "contract-run".to_owned(),
                tool_call_id: "contract-tool".to_owned(),
                tool_name: tool_name.to_owned(),
                arguments: tool["validArguments"].clone(),
            };
            AgentToolPolicy
                .authorize(&request)
                .unwrap_or_else(|error| panic!("valid {tool_name} fixture failed: {}", error.code));

            for invalid_arguments in tool["invalidArguments"]
                .as_array()
                .expect("invalid argument fixtures")
            {
                let mut invalid = request.clone();
                invalid.arguments = invalid_arguments.clone();
                assert_eq!(
                    AgentToolPolicy.authorize(&invalid).unwrap_err().code,
                    "invalid_arguments",
                    "invalid {tool_name} fixture was accepted"
                );
            }
        }
    }
}
