use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopSettings {
    pub onboarding_completed: bool,
    pub onboarding_step: u8,
    pub launch_at_login_asked: bool,
    pub launch_at_login_enabled: bool,
    pub usage_retention_days: Option<u16>,
    pub audit_retention_days: Option<u16>,
    pub gateway_base_url: String,
    pub close_behavior: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingCompletion {
    pub launch_at_login: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionSettingsDraft {
    pub usage_retention_days: Option<u16>,
    pub audit_retention_days: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditDetail {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEventSummary {
    pub id: String,
    pub event_type: String,
    pub target_type: String,
    pub target_id: String,
    pub details: Vec<AuditDetail>,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditLog {
    pub total_count: u64,
    pub events: Vec<AuditEventSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataCleanupPreview {
    pub usage_request_count: u64,
    pub audit_event_count: u64,
    pub usage_retention_days: Option<u16>,
    pub audit_retention_days: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataCleanupResult {
    pub usage_requests_deleted: u64,
    pub audit_events_deleted: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenericClientSummary {
    pub client_app_id: String,
    pub display_name: String,
    pub display_prefix: String,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenericClientCatalog {
    pub gateway_base_url: String,
    pub clients: Vec<GenericClientSummary>,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenericClientCredential {
    pub client: GenericClientSummary,
    pub api_key: String,
}
