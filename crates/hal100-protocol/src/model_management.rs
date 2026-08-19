use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadSource {
    #[serde(rename = "huggingFace")]
    HuggingFace,
    #[serde(rename = "modelScope")]
    ModelScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareRecommendation {
    pub summary: String,
    pub parameter_range: String,
    pub quantization: String,
    pub conservative_model_bytes: u64,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareProfile {
    pub chip: String,
    pub model_identifier: String,
    pub total_unified_memory_bytes: u64,
    pub physical_cpu_cores: u32,
    pub logical_cpu_cores: u32,
    pub model_storage_path: String,
    pub model_storage_available_bytes: u64,
    pub recommendation: HardwareRecommendation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelSource {
    #[serde(rename = "huggingFace")]
    HuggingFace,
    #[serde(rename = "modelScope")]
    ModelScope,
    #[serde(rename = "localFile")]
    LocalFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelOwnership {
    Managed,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LocalModelState {
    Ready,
    Missing,
    Changed,
    VerificationFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModelSummary {
    pub id: String,
    pub display_name: String,
    pub format: String,
    pub quantization: Option<String>,
    pub source: ModelSource,
    pub repository: Option<String>,
    pub revision: Option<String>,
    pub file_name: String,
    pub ownership: ModelOwnership,
    pub license: Option<String>,
    pub state: LocalModelState,
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelLibrary {
    pub default_download_source: Option<DownloadSource>,
    pub model_storage_path: String,
    pub models: Vec<LocalModelSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GgufImportPlan {
    pub plan_id: String,
    pub expires_at_ms: i64,
    pub source_path: String,
    pub display_name: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub gguf_version: u32,
    pub tensor_count: u64,
    pub metadata_count: u64,
    pub quantization: Option<String>,
    pub ownership: ModelOwnership,
    pub action_summary: String,
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GgufImportResult {
    pub imported: bool,
    pub model: LocalModelSummary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelRemovalKind {
    MoveManagedFileToTrash,
    RemoveMissingManagedIndex,
    RemoveExternalIndex,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRemovalPlan {
    pub plan_id: String,
    pub expires_at_ms: i64,
    pub model_id: String,
    pub display_name: String,
    pub ownership: ModelOwnership,
    pub size_bytes: u64,
    pub removal_kind: ModelRemovalKind,
    pub action_summary: String,
    pub source_file_preserved: bool,
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRemovalResult {
    pub removed: bool,
    pub model_id: String,
    pub display_name: String,
    pub ownership: ModelOwnership,
    pub removal_kind: ModelRemovalKind,
    pub source_file_preserved: bool,
}
