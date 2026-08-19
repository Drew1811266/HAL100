use hal100_protocol::{
    AGENT_RPC_VERSION, AgentRpcEnvelope, SIMULATED_SYSTEM_SUMMARY_TOOL, ToolCallRequestPayload,
    ToolCallResultPayload, ToolCallResultStatus,
};

#[test]
fn shared_ping_fixture_matches_rust_contract() {
    let fixture = include_str!("../../../tests/fixtures/agent-rpc/ping.json");
    let envelope: AgentRpcEnvelope = serde_json::from_str(fixture).expect("valid fixture");

    assert_eq!(envelope.protocol_version, AGENT_RPC_VERSION);
    assert_eq!(envelope.kind, "system.ping");
}

#[test]
fn shared_tool_request_fixture_matches_rust_contract() {
    let fixture = include_str!("../../../tests/fixtures/agent-rpc/tool-call-request.json");
    let envelope: AgentRpcEnvelope = serde_json::from_str(fixture).expect("valid fixture");
    let payload: ToolCallRequestPayload =
        serde_json::from_value(envelope.payload).expect("valid request payload");

    assert_eq!(envelope.kind, "tool.call.request");
    assert_eq!(payload.tool_name, SIMULATED_SYSTEM_SUMMARY_TOOL);
    assert_eq!(payload.arguments["detail"], "summary");
}

#[test]
fn shared_tool_result_fixture_matches_rust_contract() {
    let fixture = include_str!("../../../tests/fixtures/agent-rpc/tool-call-result.json");
    let envelope: AgentRpcEnvelope = serde_json::from_str(fixture).expect("valid fixture");
    let payload: ToolCallResultPayload =
        serde_json::from_value(envelope.payload).expect("valid result payload");

    assert_eq!(envelope.kind, "tool.call.result");
    assert_eq!(payload.status, ToolCallResultStatus::Success);
    assert_eq!(payload.output.expect("fixture output")["simulated"], true);
}
