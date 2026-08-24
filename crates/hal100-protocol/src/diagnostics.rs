use serde::{Deserialize, Serialize};

use crate::{EngineInstallState, EngineRuntimeState, OpenCodeIntegrationState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EnvironmentHealthStatus {
    Healthy,
    NeedsAttention,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticComponent {
    Gateway,
    InferenceEngine,
    ModelLibrary,
    OpenCode,
    PiCodingAgent,
    OpenClaw,
    HermesAgent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticRepairKind {
    InstallLlamaCpp,
    ConfigureExternalAgent,
    RemoveModelIndex,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentDiagnosticFinding {
    pub finding_id: String,
    pub code: String,
    pub component: DiagnosticComponent,
    pub severity: DiagnosticSeverity,
    pub title: String,
    pub summary: String,
    pub target_id: Option<String>,
    pub repair_kind: Option<DiagnosticRepairKind>,
    pub repair_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentDiagnosticReport {
    pub report_id: String,
    pub generated_at_ms: i64,
    pub status: EnvironmentHealthStatus,
    pub engine_install_state: EngineInstallState,
    pub engine_runtime_state: EngineRuntimeState,
    pub ready_model_count: u32,
    pub unhealthy_model_count: u32,
    pub configured_backend_count: u32,
    pub open_code_installed: bool,
    pub open_code_integration_state: OpenCodeIntegrationState,
    pub installed_external_agent_count: u32,
    pub configured_external_agent_count: u32,
    pub attention_external_agent_count: u32,
    pub warning_count: u32,
    pub error_count: u32,
    pub omitted_finding_count: u32,
    pub findings: Vec<EnvironmentDiagnosticFinding>,
}
