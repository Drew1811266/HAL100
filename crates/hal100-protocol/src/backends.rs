use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{InferenceEngineKind, InferenceEngineOwnership, InferenceProtocol};

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

impl BackendKind {
    pub const fn engine_kind(self) -> Option<InferenceEngineKind> {
        match self {
            Self::ManagedLlamaCpp | Self::ExternalLlamaCpp => Some(InferenceEngineKind::LlamaCpp),
            Self::ExternalOllama => Some(InferenceEngineKind::Ollama),
            Self::ExternalVllm => Some(InferenceEngineKind::Vllm),
            Self::ExternalOpenAi | Self::ExternalAnthropic => None,
        }
    }

    pub const fn ownership(self) -> InferenceEngineOwnership {
        match self {
            Self::ManagedLlamaCpp => InferenceEngineOwnership::Managed,
            Self::ExternalOpenAi
            | Self::ExternalAnthropic
            | Self::ExternalOllama
            | Self::ExternalVllm
            | Self::ExternalLlamaCpp => InferenceEngineOwnership::External,
        }
    }

    pub const fn gateway_protocol(self) -> InferenceProtocol {
        match self {
            Self::ExternalAnthropic => InferenceProtocol::Anthropic,
            Self::ManagedLlamaCpp
            | Self::ExternalOpenAi
            | Self::ExternalOllama
            | Self::ExternalVllm
            | Self::ExternalLlamaCpp => InferenceProtocol::OpenAi,
        }
    }

    /// Legacy/default adapter binding for backend kinds that historically encoded an engine.
    ///
    /// New OpenAI-compatible backends carry `engine` and `adapter_variant` explicitly instead of
    /// growing this enum for every inference engine.
    pub const fn default_adapter_variant(self) -> Option<&'static str> {
        match self {
            Self::ManagedLlamaCpp => Some("hal100-managed-metal"),
            Self::ExternalLlamaCpp | Self::ExternalVllm => Some("official-openai-server"),
            Self::ExternalOllama => Some("official-loopback-api"),
            Self::ExternalOpenAi | Self::ExternalAnthropic => None,
        }
    }
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
    /// Exact engine identity is orthogonal to the gateway protocol represented by `kind`.
    pub engine: Option<InferenceEngineKind>,
    pub adapter_variant: Option<String>,
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
    #[serde(default)]
    pub engine: Option<InferenceEngineKind>,
    #[serde(default)]
    pub adapter_variant: Option<String>,
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
            .field("engine", &self.engine)
            .field("adapter_variant", &self.adapter_variant)
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
    /// Optional explicit engine identity. Legacy fixed targets may omit this; engine-aware
    /// candidates use it to preserve the adapter binding when the user accepts the suggestion.
    #[serde(default)]
    pub engine: Option<InferenceEngineKind>,
    #[serde(default)]
    pub adapter_variant: Option<String>,
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
    #[serde(default)]
    pub external_engines: Vec<crate::ExternalEngineSnapshot>,
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
            engine: None,
            adapter_variant: None,
            api_root: "http://127.0.0.1:8000/v1/".to_owned(),
            auth_method: BackendAuthMethod::Bearer,
            api_key: Some("never-print-this-secret".to_owned()),
        };

        let debug = format!("{draft:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("never-print-this-secret"));
    }

    #[test]
    fn legacy_backend_kinds_map_to_separate_engine_protocol_and_ownership_dimensions() {
        assert_eq!(
            BackendKind::ManagedLlamaCpp.engine_kind(),
            Some(InferenceEngineKind::LlamaCpp)
        );
        assert_eq!(
            BackendKind::ExternalVllm.ownership(),
            InferenceEngineOwnership::External
        );
        assert_eq!(
            BackendKind::ExternalVllm.gateway_protocol(),
            InferenceProtocol::OpenAi
        );
        assert_eq!(BackendKind::ExternalOpenAi.engine_kind(), None);
        assert_eq!(BackendKind::ExternalOpenAi.default_adapter_variant(), None);
        assert_eq!(
            BackendKind::ExternalVllm.default_adapter_variant(),
            Some("official-openai-server")
        );
        assert_eq!(
            BackendKind::ExternalAnthropic.gateway_protocol(),
            InferenceProtocol::Anthropic
        );
    }
}
