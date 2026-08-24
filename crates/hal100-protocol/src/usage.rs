use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageTotals {
    pub request_count: u64,
    pub input_tokens: u64,
    pub cached_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRequestSummary {
    pub request_id: String,
    pub client_app_id: String,
    pub client_display_name: String,
    pub requested_model: String,
    pub resolved_model: String,
    pub backend_id: String,
    pub started_at_ms: i64,
    pub completed_at_ms: i64,
    pub input_tokens: Option<u64>,
    pub cached_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub status: String,
    pub usage_accuracy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDailySummary {
    pub date: String,
    pub request_count: u64,
    pub input_tokens: u64,
    pub cached_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageHourlySummary {
    pub hour: u8,
    pub request_count: u64,
    pub input_tokens: u64,
    pub cached_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDashboard {
    pub totals: UsageTotals,
    pub recent_requests: Vec<UsageRequestSummary>,
    pub daily_usage: Vec<UsageDailySummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageScopeQuery {
    pub start_at_ms: i64,
    pub end_at_ms_exclusive: i64,
    pub series_start_at_ms: i64,
    pub series_end_at_ms_exclusive: i64,
    pub client_app_id: Option<String>,
    pub resolved_model: Option<String>,
    pub backend_id: Option<String>,
    pub status: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDimensionSummary {
    pub id: String,
    pub display_name: String,
    pub request_count: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageScopeSummary {
    pub totals: UsageTotals,
    pub measured_request_count: u64,
    pub succeeded_request_count: u64,
    pub client_usage: Vec<UsageDimensionSummary>,
    pub recent_requests: Vec<UsageRequestSummary>,
    pub daily_usage: Vec<UsageDailySummary>,
    pub hourly_usage: Vec<UsageHourlySummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageFilterOption {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageFilterOptions {
    pub earliest_usage_at_ms: Option<i64>,
    pub latest_usage_at_ms: Option<i64>,
    pub clients: Vec<UsageFilterOption>,
    pub models: Vec<UsageFilterOption>,
    pub backends: Vec<UsageFilterOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTestResult {
    pub content: String,
    pub model: String,
    pub input_tokens: Option<u64>,
    pub cached_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub elapsed_ms: u64,
    pub request_id: Option<String>,
}
