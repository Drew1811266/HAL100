import { Agent, type StreamFn } from "@earendil-works/pi-agent-core";

const disabledStream: StreamFn = () => {
  throw new Error("HAL100 Pi probe cannot invoke a model");
};

export const piIntegrationPolicy = Object.freeze({
  codingAgentEnabled: false,
  dynamicExtensionsEnabled: false,
  resourceDiscoveryEnabled: false,
  directToolExecutionEnabled: false,
});

export type PiKernelStatus = Readonly<{
  piEnabled: true;
  registeredToolCount: 0;
  codingAgentEnabled: false;
  dynamicExtensionsEnabled: false;
  resourceDiscoveryEnabled: false;
  directToolExecutionEnabled: false;
}>;

export function probePiKernel(): PiKernelStatus {
  const agent = new Agent({
    streamFn: disabledStream,
    initialState: {
      systemPrompt: "HAL100 controlled local inference operations agent",
      tools: [],
      messages: [],
    },
  });

  if (agent.state.tools.length !== 0) {
    throw new Error("HAL100 Pi boundary must start without tools");
  }

  return Object.freeze({
    piEnabled: true,
    registeredToolCount: 0,
    ...piIntegrationPolicy,
  });
}
