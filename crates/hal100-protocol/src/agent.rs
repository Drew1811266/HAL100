use serde::{Deserialize, Serialize};

use crate::{BackendKind, EngineInstallState, EngineRuntimeState, EnvironmentDiagnosticReport};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentComponentState {
    Unavailable,
    Stopped,
    Starting,
    Running,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub kernel_state: AgentComponentState,
    pub model_runtime_state: AgentComponentState,
    pub pi_version: String,
    pub model_name: String,
    pub model_prepared: bool,
    pub model_size_bytes: u64,
    pub idle_timeout_seconds: u32,
    pub active_run_id: Option<String>,
    pub cancellation_requested: bool,
    pub last_error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPromptRequest {
    pub prompt: String,
    #[serde(default)]
    pub cloud_target: Option<AgentCloudTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentProviderProtocol {
    LocalOpenAi,
    CloudOpenAi,
    CloudAnthropic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCloudTarget {
    pub backend_id: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCloudRunPreview {
    pub backend_id: String,
    pub backend_name: String,
    pub backend_kind: BackendKind,
    pub api_root: String,
    pub model: String,
    pub prompt_bytes: u32,
    pub sends_system_instructions: bool,
    pub may_send_tool_results: bool,
    pub sends_credentials_to_sidecar: bool,
    pub sends_local_paths: bool,
    pub confirmation_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCloudSessionPreview {
    pub backend_id: String,
    pub backend_name: String,
    pub backend_kind: BackendKind,
    pub api_root: String,
    pub model: String,
    pub sends_future_prompts: bool,
    pub sends_system_instructions: bool,
    pub may_send_tool_results: bool,
    pub stores_conversation_history: bool,
    pub sends_credentials_to_sidecar: bool,
    pub sends_local_paths: bool,
    pub confirmation_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCloudSessionStatus {
    pub active: bool,
    pub available: bool,
    pub backend_id: Option<String>,
    pub backend_name: Option<String>,
    pub backend_kind: Option<BackendKind>,
    pub api_root: Option<String>,
    pub model: Option<String>,
    pub provider_protocol: Option<AgentProviderProtocol>,
    pub activated_at_ms: Option<i64>,
    pub last_error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolEvent {
    pub tool_call_id: String,
    pub tool_name: String,
    pub label: String,
    pub status: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunResult {
    pub run_id: String,
    pub answer: String,
    pub tool_events: Vec<AgentToolEvent>,
    pub action_plans: Vec<AgentActionPlan>,
    pub model_name: String,
    pub started_at_ms: i64,
    pub completed_at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentActionKind {
    StartOrSwitchModel,
    RemoveModel,
    InstallLlamaCpp,
    RemoveLlamaCpp,
    ConfigureOpenCode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentActionPlan {
    pub plan_id: String,
    pub run_id: String,
    pub action_kind: AgentActionKind,
    pub target_id: String,
    pub target_name: String,
    pub current_state: Option<String>,
    pub details: Vec<String>,
    pub expires_at_ms: i64,
    pub action_summary: String,
    pub requires_native_confirmation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentActionResult {
    pub plan_id: String,
    pub action_kind: AgentActionKind,
    pub target_id: String,
    pub target_name: String,
    pub outcome_summary: String,
    pub runtime_state: Option<EngineRuntimeState>,
    pub diagnostic_report: Option<EnvironmentDiagnosticReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeModel {
    pub id: String,
    pub display_name: String,
    pub quantization: Option<String>,
    pub size_bytes: u64,
    pub ready: bool,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeCatalog {
    pub engine_install_state: EngineInstallState,
    pub engine_runtime_state: EngineRuntimeState,
    pub active_model_id: Option<String>,
    pub active_model_name: Option<String>,
    pub active_backend_id: Option<String>,
    pub configured_backend_count: u32,
    pub models: Vec<AgentRuntimeModel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSystemSummary {
    pub source: String,
    pub platform: String,
    pub architecture: String,
    pub chip: String,
    pub model_identifier: String,
    pub total_unified_memory_bytes: u64,
    pub physical_cpu_cores: u32,
    pub logical_cpu_cores: u32,
    pub model_storage_available_bytes: u64,
    pub recommendation_summary: String,
    pub recommended_parameter_range: String,
    pub recommended_quantization: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_request_defaults_to_the_local_provider_for_older_clients() {
        let request: AgentPromptRequest =
            serde_json::from_str(r#"{"prompt":"检查 HAL100 本地模型"}"#)
                .expect("compatible Agent request");
        assert_eq!(request.cloud_target, None);
    }

    #[test]
    fn cloud_target_contains_a_reference_but_never_a_credential() {
        let request = AgentPromptRequest {
            prompt: "检查 HAL100 后端配置".to_owned(),
            cloud_target: Some(AgentCloudTarget {
                backend_id: "cloud-openai".to_owned(),
                model: "gpt-test".to_owned(),
            }),
        };
        let serialized = serde_json::to_string(&request).expect("serialize Agent request");
        assert!(serialized.contains("cloud-openai"));
        assert!(!serialized.to_lowercase().contains("api_key"));
        assert!(!serialized.to_lowercase().contains("apikey"));
    }

    #[test]
    fn inactive_cloud_session_status_contains_no_target_state() {
        let status = AgentCloudSessionStatus {
            active: false,
            available: false,
            backend_id: None,
            backend_name: None,
            backend_kind: None,
            api_root: None,
            model: None,
            provider_protocol: None,
            activated_at_ms: None,
            last_error_code: None,
        };
        assert!(!status.active);
        assert!(status.backend_id.is_none());
        assert!(status.model.is_none());
    }
}
