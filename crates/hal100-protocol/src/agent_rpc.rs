use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::AgentProviderProtocol;

pub const AGENT_RPC_VERSION: u16 = 3;
pub const AGENT_RPC_MAX_FRAME_BYTES: usize = 1024 * 1024;
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
    pub requires_system_summary: bool,
    pub requires_runtime_catalog: bool,
    pub requires_model_start_plan: bool,
    pub requires_model_removal_plan: bool,
    pub requires_environment_diagnostics: bool,
    pub requires_diagnostic_repair_plan: bool,
    pub requires_engine_install_plan: bool,
    pub requires_engine_remove_plan: bool,
    #[serde(rename = "requiresOpenCodeStatus")]
    pub requires_opencode_status: bool,
    #[serde(rename = "requiresOpenCodeConfigurationPlan")]
    pub requires_opencode_configuration_plan: bool,
    pub gateway_base_url: String,
    pub api_key: String,
    pub model_id: String,
    pub provider_protocol: AgentProviderProtocol,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunCompletedPayload {
    pub run_id: String,
    pub answer: String,
    pub registered_tool_count: u8,
    pub completed_tool_calls: u32,
    pub tool_names: Vec<String>,
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
            requires_system_summary: false,
            requires_runtime_catalog: true,
            requires_model_start_plan: false,
            requires_model_removal_plan: false,
            requires_environment_diagnostics: true,
            requires_diagnostic_repair_plan: false,
            requires_engine_install_plan: false,
            requires_engine_remove_plan: false,
            requires_opencode_status: false,
            requires_opencode_configuration_plan: false,
            gateway_base_url: "http://127.0.0.1:10100/v1".to_owned(),
            api_key: "local-transient-session-key".to_owned(),
            model_id: "hal100-agent-cloud-test".to_owned(),
            provider_protocol: AgentProviderProtocol::CloudAnthropic,
        };
        let value = serde_json::to_value(payload).expect("Agent start payload");
        assert_eq!(value["providerProtocol"], "cloudAnthropic");
        assert_eq!(value["modelId"], "hal100-agent-cloud-test");
        assert!(value.get("backendApiKey").is_none());
    }
}
