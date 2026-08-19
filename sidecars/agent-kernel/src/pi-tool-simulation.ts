import { Agent } from "@earendil-works/pi-agent-core";
import { createFauxCore, fauxAssistantMessage, fauxToolCall } from "@earendil-works/pi-ai";
import { SIMULATED_SYSTEM_SUMMARY_TOOL, type ToolBrokerBridge } from "./tool-bridge.js";

export interface PiToolSimulationResult {
  runId: string;
  registeredToolCount: 1;
  brokerRoundTrips: 1;
  toolName: typeof SIMULATED_SYSTEM_SUMMARY_TOOL;
  directSystemExecution: false;
  modelRequests: 0;
  networkRequests: 0;
}

export async function runPiToolSimulation(
  runId: string,
  bridge: ToolBrokerBridge,
): Promise<PiToolSimulationResult> {
  const faux = createFauxCore({
    api: "hal100-simulation",
    provider: "hal100-simulation",
    models: [{ id: "deterministic-tool-simulation" }],
  });
  faux.setResponses([
    fauxAssistantMessage(
      fauxToolCall(SIMULATED_SYSTEM_SUMMARY_TOOL, { detail: "summary" }, { id: "pi-sim-tool-1" }),
      { stopReason: "toolUse" },
    ),
    fauxAssistantMessage("Rust Tool Broker 已返回模拟系统摘要。", { stopReason: "stop" }),
  ]);

  const tool = bridge.createSystemSummaryTool(runId);
  const agent = new Agent({
    streamFn: faux.streamSimple,
    toolExecution: "sequential",
    initialState: {
      systemPrompt: "HAL100 controlled tool boundary simulation",
      model: faux.getModel(),
      tools: [tool],
      messages: [],
    },
  });

  let completedToolCalls = 0;
  agent.subscribe((event) => {
    if (event.type === "tool_execution_end" && !event.isError) {
      completedToolCalls += 1;
    }
  });

  await agent.prompt("通过唯一允许的只读工具请求模拟系统摘要。");

  if (completedToolCalls !== 1 || faux.state.callCount !== 2) {
    throw new Error("Pi tool boundary simulation did not complete exactly one broker round trip");
  }

  return {
    runId,
    registeredToolCount: 1,
    brokerRoundTrips: 1,
    toolName: SIMULATED_SYSTEM_SUMMARY_TOOL,
    directSystemExecution: false,
    modelRequests: 0,
    networkRequests: 0,
  };
}
