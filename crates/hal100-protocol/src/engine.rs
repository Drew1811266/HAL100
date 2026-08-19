use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EngineInstallState {
    NotInstalled,
    Installed,
    VerificationFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EngineRuntimeState {
    Stopped,
    Starting,
    Running,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamaCppStatus {
    pub version: String,
    pub install_state: EngineInstallState,
    pub runtime_state: EngineRuntimeState,
    pub active_model_id: Option<String>,
    pub active_model_name: Option<String>,
    pub port: Option<u16>,
    pub last_error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInstallPlan {
    pub plan_id: String,
    pub expires_at_ms: i64,
    pub engine: String,
    pub version: String,
    pub archive_size_bytes: u64,
    pub publisher: String,
    pub action_summary: String,
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineRemovePlan {
    pub plan_id: String,
    pub expires_at_ms: i64,
    pub engine: String,
    pub version: String,
    pub install_path: String,
    pub action_summary: String,
    pub requires_confirmation: bool,
}
