use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AnthropicMessagesRequestMetadata {
    pub model: String,
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnthropicUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
}

impl AnthropicUsage {
    pub fn normalized_input_tokens(&self) -> u64 {
        self.input_tokens
            .saturating_add(self.cache_creation_input_tokens)
            .saturating_add(self.cache_read_input_tokens)
    }

    pub fn normalized_total_tokens(&self) -> u64 {
        self.normalized_input_tokens()
            .saturating_add(self.output_tokens)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnthropicErrorEnvelope {
    #[serde(rename = "type")]
    pub envelope_type: String,
    pub error: AnthropicError,
    pub request_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnthropicError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_anthropic_cache_categories_without_double_counting() {
        let usage = AnthropicUsage {
            input_tokens: 10,
            cache_creation_input_tokens: 4,
            cache_read_input_tokens: 20,
            output_tokens: 6,
        };

        assert_eq!(usage.normalized_input_tokens(), 34);
        assert_eq!(usage.normalized_total_tokens(), 40);
    }
}
