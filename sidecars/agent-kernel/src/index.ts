import { AgentRunFailure, runPiAgent, validateAgentRunRequest } from "./agent-run.js";
import { probePiKernel } from "./pi-boundary.js";
import { runPiToolSimulation } from "./pi-tool-simulation.js";
import {
  AGENT_RPC_VERSION,
  type AgentRpcEnvelope,
  AgentRpcFrameDecoder,
  encodeAgentRpcFrame,
} from "./protocol.js";
import { ToolBrokerBridge } from "./tool-bridge.js";

const decoder = new AgentRpcFrameDecoder();
const piKernelStatus = probePiKernel();
const toolBridge = new ToolBrokerBridge(reply);
let activeSimulationId: string | undefined;
let activeRunId: string | undefined;
let shuttingDown = false;

function reply(envelope: AgentRpcEnvelope): void {
  process.stdout.write(encodeAgentRpcFrame(envelope));
}

function handle(envelope: AgentRpcEnvelope): void {
  if (envelope.protocolVersion !== AGENT_RPC_VERSION) {
    reply({
      protocolVersion: AGENT_RPC_VERSION,
      id: envelope.id,
      kind: "system.error",
      payload: { code: "unsupported_protocol", expected: AGENT_RPC_VERSION },
    });
    return;
  }

  if (envelope.kind === "system.ping") {
    reply({
      protocolVersion: AGENT_RPC_VERSION,
      id: envelope.id,
      kind: "system.pong",
      payload: { kernel: "hal100-agent-kernel", ...piKernelStatus },
    });
    return;
  }

  if (envelope.kind === "tool.call.result") {
    if (!toolBridge.acceptResult(envelope)) {
      reply({
        protocolVersion: AGENT_RPC_VERSION,
        id: envelope.id,
        kind: "system.error",
        payload: { code: "unexpected_tool_result" },
      });
    }
    return;
  }

  if (envelope.kind === "agent.simulation.start") {
    if (activeSimulationId || activeRunId) {
      reply({
        protocolVersion: AGENT_RPC_VERSION,
        id: envelope.id,
        kind: "system.error",
        payload: { code: "agent_busy" },
      });
      return;
    }

    activeSimulationId = envelope.id;
    void runPiToolSimulation(envelope.id, toolBridge)
      .then((result) => {
        if (!shuttingDown) {
          reply({
            protocolVersion: AGENT_RPC_VERSION,
            id: envelope.id,
            kind: "agent.simulation.completed",
            payload: result,
          });
        }
      })
      .catch(() => {
        if (!shuttingDown) {
          reply({
            protocolVersion: AGENT_RPC_VERSION,
            id: envelope.id,
            kind: "system.error",
            payload: { code: "simulation_failed" },
          });
        }
      })
      .finally(() => {
        activeSimulationId = undefined;
      });
    return;
  }

  if (envelope.kind === "agent.run.start") {
    if (activeSimulationId || activeRunId) {
      reply({
        protocolVersion: AGENT_RPC_VERSION,
        id: envelope.id,
        kind: "system.error",
        payload: { code: "agent_busy" },
      });
      return;
    }

    try {
      const request = validateAgentRunRequest(envelope.payload as never);
      activeRunId = envelope.id;
      void runPiAgent(envelope.id, request, toolBridge)
        .then((result) => {
          if (!shuttingDown) {
            reply({
              protocolVersion: AGENT_RPC_VERSION,
              id: envelope.id,
              kind: "agent.run.completed",
              payload: result,
            });
          }
        })
        .catch((error: unknown) => {
          if (!shuttingDown) {
            reply({
              protocolVersion: AGENT_RPC_VERSION,
              id: envelope.id,
              kind: "system.error",
              payload: {
                code: error instanceof AgentRunFailure ? error.code : "agent_run_failed",
              },
            });
          }
        })
        .finally(() => {
          activeRunId = undefined;
        });
    } catch {
      reply({
        protocolVersion: AGENT_RPC_VERSION,
        id: envelope.id,
        kind: "system.error",
        payload: { code: "invalid_agent_run" },
      });
    }
    return;
  }

  if (envelope.kind === "system.shutdown") {
    shuttingDown = true;
    toolBridge.cancelAll("Agent Kernel is shutting down");
    reply({
      protocolVersion: AGENT_RPC_VERSION,
      id: envelope.id,
      kind: "system.shutdown.ack",
      payload: {},
    });
    process.stdin.pause();
    return;
  }

  reply({
    protocolVersion: AGENT_RPC_VERSION,
    id: envelope.id,
    kind: "system.error",
    payload: { code: "unknown_message", receivedKind: envelope.kind },
  });
}

process.stdin.on("data", (chunk: Buffer) => {
  try {
    for (const envelope of decoder.push(chunk)) {
      handle(envelope);
    }
  } catch (error) {
    const message = error instanceof Error ? error.message : "unknown protocol error";
    process.stderr.write(`[hal100-agent-kernel] ${message}\n`);
    process.exitCode = 2;
    process.stdin.pause();
  }
});
