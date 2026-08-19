use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeDetection {
    pub installed: bool,
    pub version: Option<String>,
    pub binary_path: Option<String>,
    pub config_path: String,
    pub config_exists: bool,
    pub config_format: OpenCodeConfigFormat,
    pub integration_state: OpenCodeIntegrationState,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OpenCodeConfigFormat {
    Json,
    Jsonc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OpenCodeIntegrationState {
    NotConfigured,
    Configured,
    Conflict,
    ModifiedOutsideHal100,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeConfigChange {
    pub path: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeConfigPlan {
    pub plan_id: String,
    pub expires_at_ms: i64,
    pub config_path: String,
    pub credential_path: String,
    pub changes: Vec<OpenCodeConfigChange>,
    pub creates_backup: bool,
    pub preserves_default_model: bool,
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeApplyResult {
    pub configured: bool,
    pub config_path: String,
    pub backup_path: Option<String>,
    pub credential_prefix: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeProjectDiagnosis {
    pub project_path: String,
    pub config_path: Option<String>,
    pub overrides_hal100_provider: bool,
    pub overrides_default_model: bool,
    pub warnings: Vec<String>,
}
