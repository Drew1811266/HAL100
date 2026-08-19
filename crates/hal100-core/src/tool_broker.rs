use hal100_protocol::{
    ENVIRONMENT_DIAGNOSTICS_TOOL, OPENCODE_STATUS_TOOL, PLAN_DIAGNOSTIC_REPAIR_TOOL,
    PLAN_ENGINE_INSTALL_TOOL, PLAN_ENGINE_REMOVE_TOOL, PLAN_MODEL_REMOVAL_TOOL,
    PLAN_MODEL_START_TOOL, PLAN_OPENCODE_CONFIGURATION_TOOL, RUNTIME_CATALOG_TOOL,
    SYSTEM_SUMMARY_TOOL, ToolCallErrorPayload, ToolCallRequestPayload, ToolCallResultPayload,
};
use serde_json::{Map, Value, json};

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
    PlanDiagnosticRepair {
        report_id: String,
        finding_id: String,
    },
    PlanEngineInstall,
    PlanEngineRemove,
    InspectOpenCodeStatus,
    PlanOpenCodeConfiguration,
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
        match request.tool_name.as_str() {
            SYSTEM_SUMMARY_TOOL => {
                require_exact_summary_arguments(&request.arguments)?;
                Ok(AuthorizedAgentTool::InspectSystemSummary)
            }
            RUNTIME_CATALOG_TOOL => {
                require_exact_summary_arguments(&request.arguments)?;
                Ok(AuthorizedAgentTool::InspectRuntimeCatalog)
            }
            PLAN_MODEL_START_TOOL => Ok(AuthorizedAgentTool::PlanModelStart {
                model_id: exact_model_id(&request.arguments)?,
            }),
            PLAN_MODEL_REMOVAL_TOOL => Ok(AuthorizedAgentTool::PlanModelRemoval {
                model_id: exact_model_id(&request.arguments)?,
            }),
            ENVIRONMENT_DIAGNOSTICS_TOOL => {
                require_exact_target_arguments(&request.arguments, "full")?;
                Ok(AuthorizedAgentTool::InspectEnvironmentDiagnostics)
            }
            PLAN_DIAGNOSTIC_REPAIR_TOOL => {
                let (report_id, finding_id) = exact_diagnostic_repair_ids(&request.arguments)?;
                Ok(AuthorizedAgentTool::PlanDiagnosticRepair {
                    report_id,
                    finding_id,
                })
            }
            PLAN_ENGINE_INSTALL_TOOL => {
                require_exact_target_arguments(&request.arguments, "llama.cpp")?;
                Ok(AuthorizedAgentTool::PlanEngineInstall)
            }
            PLAN_ENGINE_REMOVE_TOOL => {
                require_exact_target_arguments(&request.arguments, "llama.cpp")?;
                Ok(AuthorizedAgentTool::PlanEngineRemove)
            }
            OPENCODE_STATUS_TOOL => {
                require_exact_target_arguments(&request.arguments, "opencode")?;
                Ok(AuthorizedAgentTool::InspectOpenCodeStatus)
            }
            PLAN_OPENCODE_CONFIGURATION_TOOL => {
                require_exact_target_arguments(&request.arguments, "opencode")?;
                Ok(AuthorizedAgentTool::PlanOpenCodeConfiguration)
            }
            _ => Err(ToolCallErrorPayload {
                code: "tool_not_allowed".to_owned(),
                message: "tool is not present in the Rust broker allowlist".to_owned(),
            }),
        }
    }
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

#[cfg(test)]
mod tests {
    use hal100_protocol::ToolCallResultStatus;

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
    fn authorizes_only_exact_engine_and_opencode_targets() {
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
            (
                OPENCODE_STATUS_TOOL,
                "opencode",
                AuthorizedAgentTool::InspectOpenCodeStatus,
            ),
            (
                PLAN_OPENCODE_CONFIGURATION_TOOL,
                "opencode",
                AuthorizedAgentTool::PlanOpenCodeConfiguration,
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
}
