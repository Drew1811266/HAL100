import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  AGENT_RPC_MAX_FRAME_BYTES,
  type AgentRpcEnvelope,
  AgentRpcFrameDecoder,
  encodeAgentRpcFrame,
} from "../src/protocol.js";
import { assertToolCallRequestPayload, assertToolCallResultPayload } from "../src/tool-bridge.js";

const pingFixtureUrl = new URL("../../../tests/fixtures/agent-rpc/ping.json", import.meta.url);
const toolRequestFixtureUrl = new URL(
  "../../../tests/fixtures/agent-rpc/tool-call-request.json",
  import.meta.url,
);
const toolResultFixtureUrl = new URL(
  "../../../tests/fixtures/agent-rpc/tool-call-result.json",
  import.meta.url,
);

function pingFixture(): AgentRpcEnvelope {
  return JSON.parse(readFileSync(pingFixtureUrl, "utf8")) as AgentRpcEnvelope;
}

describe("Agent RPC v3 framing", () => {
  it("decodes the shared fixture when input is fragmented", () => {
    const fixture = pingFixture();
    const frame = encodeAgentRpcFrame(fixture);
    const decoder = new AgentRpcFrameDecoder();

    expect(decoder.push(frame.subarray(0, 2))).toEqual([]);
    expect(decoder.push(frame.subarray(2))).toEqual([fixture]);
  });

  it("decodes adjacent frames", () => {
    const frame = encodeAgentRpcFrame(pingFixture());
    const decoded = new AgentRpcFrameDecoder().push(Buffer.concat([frame, frame]));

    expect(decoded).toHaveLength(2);
  });

  it("rejects a declared oversized frame", () => {
    const prefix = Buffer.alloc(4);
    prefix.writeUInt32BE(AGENT_RPC_MAX_FRAME_BYTES + 1);

    expect(() => new AgentRpcFrameDecoder().push(prefix)).toThrow(RangeError);
  });

  it("validates the shared Tool Broker fixtures", () => {
    const request = JSON.parse(readFileSync(toolRequestFixtureUrl, "utf8")) as AgentRpcEnvelope;
    const result = JSON.parse(readFileSync(toolResultFixtureUrl, "utf8")) as AgentRpcEnvelope;

    expect(assertToolCallRequestPayload(request.payload).toolName).toBe(
      "hal100.inspect_system_summary",
    );
    expect(assertToolCallResultPayload(result.payload).status).toBe("success");
  });
});
