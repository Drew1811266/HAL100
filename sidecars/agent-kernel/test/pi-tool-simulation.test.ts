import { describe, expect, it } from "vitest";
import { runPiToolSimulation } from "../src/pi-tool-simulation.js";
import type { AgentRpcEnvelope } from "../src/protocol.js";
import {
  assertToolCallRequestPayload,
  SIMULATED_SYSTEM_SUMMARY_TOOL,
  ToolBrokerBridge,
} from "../src/tool-bridge.js";

describe("Pi to Rust Tool Broker boundary", () => {
  it("routes one deterministic Pi tool call through the host without direct execution", async () => {
    const requests: AgentRpcEnvelope[] = [];
    let bridge: ToolBrokerBridge;
    bridge = new ToolBrokerBridge((request) => {
      requests.push(request);
      const payload = assertToolCallRequestPayload(request.payload);
      queueMicrotask(() => {
        bridge.acceptResult({
          protocolVersion: 1,
          id: request.id,
          kind: "tool.call.result",
          payload: {
            toolCallId: payload.toolCallId,
            status: "success",
            output: {
              source: "rust_simulated_broker",
              simulated: true,
            },
          },
        });
      });
    });

    const result = await runPiToolSimulation("simulation-test-1", bridge);

    expect(result).toEqual({
      runId: "simulation-test-1",
      registeredToolCount: 1,
      brokerRoundTrips: 1,
      toolName: SIMULATED_SYSTEM_SUMMARY_TOOL,
      directSystemExecution: false,
      modelRequests: 0,
      networkRequests: 0,
    });
    expect(requests).toHaveLength(1);
    expect(assertToolCallRequestPayload(requests[0]?.payload).arguments).toEqual({
      detail: "summary",
    });
  });

  it("fails closed when a result carries a forged toolCallId", async () => {
    let bridge: ToolBrokerBridge;
    bridge = new ToolBrokerBridge((request) => {
      queueMicrotask(() => {
        bridge.acceptResult({
          protocolVersion: 1,
          id: request.id,
          kind: "tool.call.result",
          payload: {
            toolCallId: "forged-tool-call",
            status: "success",
            output: {},
          },
        });
      });
    });

    await expect(runPiToolSimulation("simulation-test-2", bridge)).rejects.toThrow(
      "did not complete exactly one broker round trip",
    );
  });
});
