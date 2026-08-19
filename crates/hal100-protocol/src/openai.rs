use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OpenAiChatRequestMetadata {
    pub model: String,
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OpenAiResponsesRequestMetadata {
    pub model: String,
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAiPromptTokenDetails {
    #[serde(default)]
    pub cached_tokens: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAiUsage {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub prompt_tokens_details: Option<OpenAiPromptTokenDetails>,
}

impl OpenAiUsage {
    pub fn cached_tokens(&self) -> u64 {
        self.prompt_tokens_details
            .as_ref()
            .map_or(0, |details| details.cached_tokens)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAiResponsesInputTokenDetails {
    #[serde(default)]
    pub cached_tokens: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAiResponsesUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub input_tokens_details: Option<OpenAiResponsesInputTokenDetails>,
}

impl OpenAiResponsesUsage {
    pub fn cached_tokens(&self) -> u64 {
        self.input_tokens_details
            .as_ref()
            .map_or(0, |details| details.cached_tokens)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAiErrorEnvelope {
    pub error: OpenAiError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAiError {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: String,
    pub code: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_cached_tokens_from_backend_usage() {
        let usage: OpenAiUsage = serde_json::from_value(serde_json::json!({
            "prompt_tokens": 11,
            "completion_tokens": 7,
            "total_tokens": 18,
            "prompt_tokens_details": { "cached_tokens": 3 }
        }))
        .expect("valid OpenAI usage");

        assert_eq!(usage.cached_tokens(), 3);
        assert_eq!(usage.total_tokens, 18);
    }

    #[test]
    fn reads_responses_usage_and_cached_input_tokens() {
        let usage: OpenAiResponsesUsage = serde_json::from_value(serde_json::json!({
            "input_tokens": 23,
            "output_tokens": 5,
            "total_tokens": 28,
            "input_tokens_details": { "cached_tokens": 7 },
            "output_tokens_details": { "reasoning_tokens": 2 }
        }))
        .expect("valid OpenAI Responses usage");

        assert_eq!(usage.cached_tokens(), 7);
        assert_eq!(usage.total_tokens, 28);
    }
}
