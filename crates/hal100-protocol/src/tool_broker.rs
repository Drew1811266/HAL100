use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SYSTEM_SUMMARY_TOOL: &str = "hal100.inspect_system_summary";
pub const SIMULATED_SYSTEM_SUMMARY_TOOL: &str = SYSTEM_SUMMARY_TOOL;
pub const RUNTIME_CATALOG_TOOL: &str = "hal100.inspect_runtime_catalog";
pub const PLAN_MODEL_START_TOOL: &str = "hal100.plan_model_start";
pub const PLAN_MODEL_STOP_TOOL: &str = "hal100.plan_model_stop";
pub const PLAN_RUNTIME_PROFILE_ACTIVATION_TOOL: &str = "hal100.plan_runtime_profile_activation";
pub const PLAN_MODEL_REMOVAL_TOOL: &str = "hal100.plan_model_removal";
pub const ENVIRONMENT_DIAGNOSTICS_TOOL: &str = "hal100.inspect_environment_diagnostics";
pub const OPERATIONAL_HISTORY_TOOL: &str = "hal100.inspect_operational_history";
pub const OPERATIONAL_HEALTH_OBSERVATION_TOOL: &str = "hal100.observe_operational_health";
pub const PLAN_DIAGNOSTIC_REPAIR_TOOL: &str = "hal100.plan_diagnostic_repair";
pub const PLAN_ENGINE_INSTALL_TOOL: &str = "hal100.plan_engine_install";
pub const PLAN_ENGINE_REMOVE_TOOL: &str = "hal100.plan_engine_remove";
pub const EXTERNAL_AGENT_STATUS_TOOL: &str = "hal100.inspect_external_agent";
pub const PLAN_EXTERNAL_AGENT_INSTALLATION_TOOL: &str = "hal100.plan_external_agent_installation";
pub const PLAN_MANAGED_EXTERNAL_AGENT_REMOVAL_TOOL: &str =
    "hal100.plan_managed_external_agent_removal";
pub const PLAN_EXTERNAL_AGENT_CONFIGURATION_TOOL: &str = "hal100.plan_external_agent_configuration";
pub const PLAN_EXTERNAL_AGENT_DISCONNECTION_TOOL: &str = "hal100.plan_external_agent_disconnection";
pub const MODEL_CATALOG_SEARCH_TOOL: &str = "hal100.search_model_catalog";
pub const MODEL_REPOSITORY_INSPECTION_TOOL: &str = "hal100.inspect_model_repository";
pub const PLAN_MODEL_DOWNLOAD_TOOL: &str = "hal100.plan_model_download";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallRequestPayload {
    pub run_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallResultStatus {
    Success,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallErrorPayload {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallResultPayload {
    pub tool_call_id: String,
    pub status: ToolCallResultStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ToolCallErrorPayload>,
}

impl ToolCallResultPayload {
    pub fn success(tool_call_id: impl Into<String>, output: Value) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            status: ToolCallResultStatus::Success,
            output: Some(output),
            error: None,
        }
    }

    pub fn error(
        tool_call_id: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            status: ToolCallResultStatus::Error,
            output: None,
            error: Some(ToolCallErrorPayload {
                code: code.into(),
                message: message.into(),
            }),
        }
    }
}
