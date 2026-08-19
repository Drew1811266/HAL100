use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackendKind {
    ManagedLlamaCpp,
    ExternalOpenAi,
    ExternalAnthropic,
    ExternalOllama,
    ExternalVllm,
    ExternalLlamaCpp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackendAuthMethod {
    None,
    Bearer,
    AnthropicApiKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendSummary {
    pub id: String,
    pub display_name: String,
    pub kind: BackendKind,
    pub api_root: String,
    pub auth_method: BackendAuthMethod,
    pub credential_configured: bool,
    pub enabled: bool,
    pub runtime_available: bool,
    pub is_active: bool,
    pub consecutive_failures: usize,
    pub circuit_open: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendRouteSummary {
    pub alias: String,
    pub backend_id: String,
    pub resolved_model: String,
    pub runtime_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendCatalog {
    pub active_backend_id: Option<String>,
    pub backends: Vec<BackendSummary>,
    pub model_routes: Vec<BackendRouteSummary>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendDraft {
    pub id: Option<String>,
    pub display_name: String,
    pub kind: BackendKind,
    pub api_root: String,
    pub auth_method: BackendAuthMethod,
    pub api_key: Option<String>,
}

impl fmt::Debug for BackendDraft {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BackendDraft")
            .field("id", &self.id)
            .field("display_name", &self.display_name)
            .field("kind", &self.kind)
            .field("api_root", &self.api_root)
            .field("auth_method", &self.auth_method)
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendRouteDraft {
    pub alias: String,
    pub backend_id: String,
    pub resolved_model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalBackendCandidate {
    pub kind: BackendKind,
    pub display_name: String,
    pub api_root: String,
    pub evidence: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalBackendDiscovery {
    pub candidates: Vec<LocalBackendCandidate>,
    pub checked_targets: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackendProbeStatus {
    Healthy,
    AuthenticationFailed,
    UpstreamError,
    InvalidResponse,
    Unreachable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendProbeResult {
    pub backend_id: String,
    pub status: BackendProbeStatus,
    pub http_status: Option<u16>,
    pub latency_ms: u64,
    pub model_count: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_draft_debug_output_redacts_api_keys() {
        let draft = BackendDraft {
            id: None,
            display_name: "测试后端".to_owned(),
            kind: BackendKind::ExternalOpenAi,
            api_root: "http://127.0.0.1:8000/v1/".to_owned(),
            auth_method: BackendAuthMethod::Bearer,
            api_key: Some("never-print-this-secret".to_owned()),
        };

        let debug = format!("{draft:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("never-print-this-secret"));
    }
}
