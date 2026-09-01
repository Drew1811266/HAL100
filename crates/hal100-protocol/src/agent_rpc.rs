use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::AgentProviderProtocol;

pub const AGENT_RPC_VERSION: u16 = 13;
pub const AGENT_RPC_MAX_FRAME_BYTES: usize = 1024 * 1024;
pub const AGENT_RPC_MAX_REQUIRED_TOOLS: usize = 4;
pub const AGENT_RPC_MAX_ACTION_PLANS: usize = 1;
pub const AGENT_RPC_MAX_TOOL_RESULT_BYTES: usize = 128 * 1024;
const LENGTH_PREFIX_BYTES: usize = 4;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRpcEnvelope {
    pub protocol_version: u16,
    pub id: String,
    pub kind: String,
    pub payload: Value,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunStartPayload {
    pub prompt: String,
    pub required_tools: Vec<String>,
    pub gateway_base_url: String,
    pub api_key: String,
    pub model_id: String,
    pub provider_protocol: AgentProviderProtocol,
    pub context_window_tokens: u32,
    pub max_output_tokens: u32,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentIntentStartPayload {
    pub prompt: String,
    pub gateway_base_url: String,
    pub api_key: String,
    pub model_id: String,
    pub provider_protocol: AgentProviderProtocol,
    pub context_window_tokens: u32,
    pub max_output_tokens: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentIntentCompletionStatus {
    Proposed,
    Invalid,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentIntentCompletedPayload {
    pub run_id: String,
    pub status: AgentIntentCompletionStatus,
    #[serde(default)]
    pub proposal: Option<Value>,
    #[serde(default)]
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunCompletedPayload {
    pub run_id: String,
    pub answer: String,
    pub registered_tool_count: u8,
    pub completed_tool_calls: u32,
    pub tool_names: Vec<String>,
    pub efficiency: AgentRunEfficiencyPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentRunEfficiencyPayload {
    pub context_window_tokens: u32,
    pub max_output_tokens: u32,
    pub execution_model_turn_count: u32,
    pub continuation_prompt_count: u32,
    pub provider_usage_available: bool,
    pub reported_input_tokens: u64,
    pub reported_output_tokens: u64,
    pub peak_reported_input_tokens: u64,
    pub peak_estimated_input_tokens: u64,
    pub task_system_prompt_bytes: u64,
    pub compacted_turn_count: u32,
    pub sent_tool_result_bytes: u64,
    pub sent_tool_result_token_estimate: u64,
    pub repeated_tool_result_bytes: u64,
    pub repeated_tool_result_token_estimate: u64,
}

#[derive(Debug, Error)]
pub enum AgentRpcFrameError {
    #[error("agent RPC frame exceeds {max} bytes: {actual}")]
    FrameTooLarge { actual: usize, max: usize },
    #[error("agent RPC frame contains invalid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
}

pub fn encode_agent_rpc_frame(envelope: &AgentRpcEnvelope) -> Result<Vec<u8>, AgentRpcFrameError> {
    let json = serde_json::to_vec(envelope)?;
    if json.len() > AGENT_RPC_MAX_FRAME_BYTES {
        return Err(AgentRpcFrameError::FrameTooLarge {
            actual: json.len(),
            max: AGENT_RPC_MAX_FRAME_BYTES,
        });
    }

    let mut frame = Vec::with_capacity(LENGTH_PREFIX_BYTES + json.len());
    frame.extend_from_slice(&(json.len() as u32).to_be_bytes());
    frame.extend_from_slice(&json);
    Ok(frame)
}

#[derive(Debug, Default)]
pub struct AgentRpcFrameDecoder {
    buffer: Vec<u8>,
}

impl AgentRpcFrameDecoder {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<AgentRpcEnvelope>, AgentRpcFrameError> {
        self.buffer.extend_from_slice(bytes);
        let mut envelopes = Vec::new();
        let mut consumed = 0;

        loop {
            let remaining = &self.buffer[consumed..];
            if remaining.len() < LENGTH_PREFIX_BYTES {
                break;
            }

            let payload_len = u32::from_be_bytes(
                remaining[..LENGTH_PREFIX_BYTES]
                    .try_into()
                    .expect("length prefix has exactly four bytes"),
            ) as usize;

            if payload_len > AGENT_RPC_MAX_FRAME_BYTES {
                return Err(AgentRpcFrameError::FrameTooLarge {
                    actual: payload_len,
                    max: AGENT_RPC_MAX_FRAME_BYTES,
                });
            }

            let frame_len = LENGTH_PREFIX_BYTES + payload_len;
            if remaining.len() < frame_len {
                break;
            }

            let json = &remaining[LENGTH_PREFIX_BYTES..frame_len];
            envelopes.push(serde_json::from_slice(json)?);
            consumed += frame_len;
        }

        if consumed > 0 {
            self.buffer.drain(..consumed);
        }

        Ok(envelopes)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn ping() -> AgentRpcEnvelope {
        AgentRpcEnvelope {
            protocol_version: AGENT_RPC_VERSION,
            id: "request-1".to_owned(),
            kind: "system.ping".to_owned(),
            payload: json!({}),
        }
    }

    #[test]
    fn decodes_fragmented_frame() {
        let frame = encode_agent_rpc_frame(&ping()).expect("frame should encode");
        let mut decoder = AgentRpcFrameDecoder::default();

        assert!(
            decoder
                .push(&frame[..3])
                .expect("prefix fragment")
                .is_empty()
        );
        let decoded = decoder.push(&frame[3..]).expect("remaining frame");

        assert_eq!(decoded, vec![ping()]);
    }

    #[test]
    fn decodes_multiple_frames_from_one_chunk() {
        let first = encode_agent_rpc_frame(&ping()).expect("first frame");
        let second = encode_agent_rpc_frame(&ping()).expect("second frame");
        let mut chunk = first;
        chunk.extend(second);

        let decoded = AgentRpcFrameDecoder::default()
            .push(&chunk)
            .expect("two frames should decode");

        assert_eq!(decoded.len(), 2);
    }

    #[test]
    fn rejects_declared_oversized_frame() {
        let oversized = (AGENT_RPC_MAX_FRAME_BYTES as u32 + 1).to_be_bytes();
        let error = AgentRpcFrameDecoder::default()
            .push(&oversized)
            .expect_err("oversized frame should fail");

        assert!(matches!(error, AgentRpcFrameError::FrameTooLarge { .. }));
    }

    #[test]
    fn run_payload_carries_an_explicit_provider_protocol() {
        let payload = AgentRunStartPayload {
            prompt: "检查 HAL100 后端".to_owned(),
            required_tools: vec![
                crate::RUNTIME_CATALOG_TOOL.to_owned(),
                crate::ENVIRONMENT_DIAGNOSTICS_TOOL.to_owned(),
            ],
            gateway_base_url: "http://127.0.0.1:10100/v1".to_owned(),
            api_key: "local-transient-session-key".to_owned(),
            model_id: "hal100-agent-cloud-test".to_owned(),
            provider_protocol: AgentProviderProtocol::CloudAnthropic,
            context_window_tokens: 128_000,
            max_output_tokens: 2_048,
        };
        let value = serde_json::to_value(payload).expect("Agent start payload");
        assert_eq!(value["providerProtocol"], "cloudAnthropic");
        assert_eq!(value["modelId"], "hal100-agent-cloud-test");
        assert_eq!(value["requiredTools"].as_array().map(Vec::len), Some(2));
        assert_eq!(value["contextWindowTokens"], 128_000);
        assert_eq!(value["maxOutputTokens"], 2_048);
        assert!(value.get("requiresRuntimeCatalog").is_none());
        assert!(value.get("backendApiKey").is_none());
    }

    #[test]
    fn rpc_version_matches_the_shared_v13_envelope_schema() {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../../../contracts/agent-rpc/v13.schema.json"))
                .expect("shared Agent RPC v13 schema");
        assert_eq!(schema["properties"]["protocolVersion"]["const"], 13);
        assert_eq!(AGENT_RPC_VERSION, 13);
    }

    #[test]
    fn rpc_limits_match_the_shared_v13_tool_policy() {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../../contracts/agent-rpc/v13-tools.json"))
                .expect("shared Agent RPC v13 tool policy");
        assert_eq!(manifest["protocolVersion"], AGENT_RPC_VERSION);
        assert_eq!(
            manifest["limits"]["maxRequiredTools"],
            AGENT_RPC_MAX_REQUIRED_TOOLS
        );
        assert_eq!(
            manifest["limits"]["maxActionPlans"],
            AGENT_RPC_MAX_ACTION_PLANS
        );
        assert_eq!(
            manifest["limits"]["maxToolResultBytes"],
            AGENT_RPC_MAX_TOOL_RESULT_BYTES
        );
        assert!(
            manifest["limits"]["maxToolResultBytes"]
                .as_u64()
                .expect("tool result budget")
                < AGENT_RPC_MAX_FRAME_BYTES as u64
        );
    }

    #[test]
    fn intent_completion_payload_rejects_unbounded_extra_fields() {
        let valid: AgentIntentCompletedPayload = serde_json::from_value(serde_json::json!({
            "runId": "run-intent-1",
            "status": "proposed",
            "proposal": {
                "schemaVersion": 1,
                "disposition": "task",
                "taskKind": "inspect_system"
            }
        }))
        .expect("bounded intent completion");
        assert_eq!(valid.status, AgentIntentCompletionStatus::Proposed);

        assert!(
            serde_json::from_value::<AgentIntentCompletedPayload>(serde_json::json!({
                "runId": "run-intent-1",
                "status": "invalid",
                "errorCode": "invalid_intent_output",
                "rawOutput": "arbitrary model text"
            }))
            .is_err()
        );
    }
}
