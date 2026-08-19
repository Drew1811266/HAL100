use serde::{Deserialize, Serialize};

use crate::{DownloadSource, LocalModelSummary};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteModelSearchItem {
    pub source: DownloadSource,
    pub repository: String,
    pub display_name: String,
    pub license: Option<String>,
    pub downloads: u64,
    pub likes: u64,
    pub parameter_count: Option<u64>,
    pub repository_size_bytes: Option<u64>,
    pub gated: bool,
    pub private: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteModelSearchResults {
    pub source: DownloadSource,
    pub query: String,
    pub items: Vec<RemoteModelSearchItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteGgufFile {
    pub path: String,
    pub size_bytes: u64,
    pub sha256: Option<String>,
    pub revision: String,
    pub quantization: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteModelRepository {
    pub source: DownloadSource,
    pub repository: String,
    pub display_name: String,
    pub license: Option<String>,
    pub gated: bool,
    pub private: bool,
    pub files: Vec<RemoteGgufFile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelDownloadState {
    Pending,
    Downloading,
    Paused,
    Verifying,
    Installing,
    Ready,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDownloadPlan {
    pub plan_id: String,
    pub expires_at_ms: i64,
    pub source: DownloadSource,
    pub repository: String,
    pub display_name: String,
    pub license: Option<String>,
    pub file: RemoteGgufFile,
    pub available_storage_bytes: u64,
    pub required_storage_bytes: u64,
    pub action_summary: String,
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDownloadSnapshot {
    pub download_id: String,
    pub source: DownloadSource,
    pub repository: String,
    pub file_name: String,
    pub state: ModelDownloadState,
    pub downloaded_bytes: u64,
    pub expected_size_bytes: u64,
    pub error_code: Option<String>,
    pub can_resume: bool,
    pub model: Option<LocalModelSummary>,
}
