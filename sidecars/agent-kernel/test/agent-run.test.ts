import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import type { StreamFn } from "@earendil-works/pi-agent-core";
import { createFauxCore, fauxAssistantMessage, fauxToolCall } from "@earendil-works/pi-ai";
import { Value } from "typebox/value";
import { describe, expect, it } from "vitest";
import {
  ACTION_PLAN_TOOLS,
  AGENT_MODEL_ALIAS,
  AGENT_TOOL_NAMES,
  AgentRunFailure,
  MAX_ACTION_PLANS,
  MAX_REQUIRED_TOOLS,
  nextRequiredAgentTool,
  runPiAgent,
  TOOL_PREREQUISITES,
  validateAgentRunRequest,
} from "../src/agent-run.js";
import { AGENT_RPC_VERSION } from "../src/protocol.js";
import {
  ENVIRONMENT_DIAGNOSTICS_TOOL,
  EXTERNAL_AGENT_STATUS_TOOL,
  MAX_TOOL_RESULT_BYTES,
  MODEL_CATALOG_SEARCH_TOOL,
  MODEL_REPOSITORY_INSPECTION_TOOL,
  OPERATIONAL_HEALTH_OBSERVATION_TOOL,
  OPERATIONAL_HISTORY_TOOL,
  PLAN_DIAGNOSTIC_REPAIR_TOOL,
  PLAN_ENGINE_INSTALL_TOOL,
  PLAN_ENGINE_REMOVE_TOOL,
  PLAN_EXTERNAL_AGENT_CONFIGURATION_TOOL,
  PLAN_EXTERNAL_AGENT_DISCONNECTION_TOOL,
  PLAN_EXTERNAL_AGENT_INSTALLATION_TOOL,
  PLAN_MANAGED_EXTERNAL_AGENT_REMOVAL_TOOL,
  PLAN_MODEL_DOWNLOAD_TOOL,
  PLAN_MODEL_REMOVAL_TOOL,
  PLAN_MODEL_START_TOOL,
  RUNTIME_CATALOG_TOOL,
  SYSTEM_SUMMARY_TOOL,
  ToolBrokerBridge,
} from "../src/tool-bridge.js";

const validRequest = {
  prompt: "检测这台 Mac，并用中文告诉我适合运行什么模型。",
  requiredTools: [SYSTEM_SUMMARY_TOOL],
  gatewayBaseUrl: "http://127.0.0.1:10100/v1",
  apiKey: "hal100_agent_test_key_1234567890",
  modelId: AGENT_MODEL_ALIAS,
  providerProtocol: "localOpenAi",
} as const;

describe("real HAL100 Agent run boundary", () => {
  it("keeps the RPC v9 tool catalog at the eighteen compatible names", () => {
    const bridge = new ToolBrokerBridge(() => undefined);
    const tools = bridge.createAgentTools("run-contract");
    const registeredTools = tools.map((tool) => tool.name);
    expect(registeredTools).toEqual([
      SYSTEM_SUMMARY_TOOL,
      RUNTIME_CATALOG_TOOL,
      PLAN_MODEL_START_TOOL,
      PLAN_MODEL_REMOVAL_TOOL,
      ENVIRONMENT_DIAGNOSTICS_TOOL,
      PLAN_DIAGNOSTIC_REPAIR_TOOL,
      PLAN_ENGINE_INSTALL_TOOL,
      PLAN_ENGINE_REMOVE_TOOL,
      EXTERNAL_AGENT_STATUS_TOOL,
      PLAN_EXTERNAL_AGENT_CONFIGURATION_TOOL,
      PLAN_EXTERNAL_AGENT_DISCONNECTION_TOOL,
      MODEL_CATALOG_SEARCH_TOOL,
      MODEL_REPOSITORY_INSPECTION_TOOL,
      PLAN_MODEL_DOWNLOAD_TOOL,
      OPERATIONAL_HISTORY_TOOL,
      OPERATIONAL_HEALTH_OBSERVATION_TOOL,
      PLAN_EXTERNAL_AGENT_INSTALLATION_TOOL,
      PLAN_MANAGED_EXTERNAL_AGENT_REMOVAL_TOOL,
    ]);
    const manifest = JSON.parse(
      readFileSync(new URL("../../../contracts/agent-rpc/v9-tools.json", import.meta.url), "utf8"),
    ) as {
      protocolVersion: number;
      limits: {
        maxRequiredTools: number;
        maxActionPlans: number;
        maxToolResultBytes: number;
      };
      tools: Array<{
        name: string;
        effect: "readOnly" | "actionPlan";
        prerequisites: string[];
        requiresNativeConfirmation: boolean;
        validArguments: unknown;
        invalidArguments: unknown[];
      }>;
    };
    expect(manifest.protocolVersion).toBe(AGENT_RPC_VERSION);
    expect(manifest.limits).toEqual({
      maxRequiredTools: MAX_REQUIRED_TOOLS,
      maxActionPlans: MAX_ACTION_PLANS,
      maxToolResultBytes: MAX_TOOL_RESULT_BYTES,
    });
    expect(manifest.tools.map((tool) => tool.name)).toEqual(AGENT_TOOL_NAMES);
    expect(registeredTools).toEqual(manifest.tools.map((tool) => tool.name));

    for (const contract of manifest.tools) {
      const tool = tools.find((candidate) => candidate.name === contract.name);
      expect(tool, `registered ${contract.name}`).toBeDefined();
      if (!tool) continue;
      expect(ACTION_PLAN_TOOLS.has(contract.name)).toBe(contract.effect === "actionPlan");
      expect(TOOL_PREREQUISITES.get(contract.name) ?? []).toEqual(contract.prerequisites);
      expect(contract.requiresNativeConfirmation).toBe(contract.effect === "actionPlan");
      expect(Value.Check(tool.parameters, contract.validArguments)).toBe(true);
      for (const invalidArguments of contract.invalidArguments) {
        expect(
          Value.Check(tool.parameters, invalidArguments),
          `${contract.name} accepted ${JSON.stringify(invalidArguments)}`,
        ).toBe(false);
      }
    }
  });

  it("runs one Pi tool loop while Rust remains the source of system information", async () => {
    const faux = createFauxCore({
      api: "openai-completions",
      provider: "hal100-test",
      models: [{ id: AGENT_MODEL_ALIAS }],
    });
    faux.setResponses([
      fauxAssistantMessage(
        fauxToolCall(SYSTEM_SUMMARY_TOOL, { detail: "summary" }, { id: "tool-real-1" }),
        { stopReason: "toolUse" },
      ),
      fauxAssistantMessage("这台 Mac 使用 Apple M1 和 16 GB 统一内存，建议优先使用量化小模型。", {
        stopReason: "stop",
      }),
    ]);

    let bridge: ToolBrokerBridge;
    bridge = new ToolBrokerBridge((envelope) => {
      queueMicrotask(() => {
        bridge.acceptResult({
          protocolVersion: AGENT_RPC_VERSION,
          id: envelope.id,
          kind: "tool.call.result",
          payload: {
            toolCallId: (envelope.payload as { toolCallId: string }).toolCallId,
            status: "success",
            output: {
              source: "rust_platform_probe",
              chip: "Apple M1",
              unifiedMemoryBytes: 16 * 1024 * 1024 * 1024,
            },
          },
        });
      });
    });

    const result = await runPiAgent("run-1", validRequest, bridge, {
      streamFn: faux.streamSimple,
      model: faux.getModel() as never,
    });

    expect(result.completedToolCalls).toBe(1);
    expect(result.toolNames).toEqual([SYSTEM_SUMMARY_TOOL]);
    expect(result.answer).toContain("Apple M1");
    expect(faux.state.callCount).toBe(2);
  });

  it("accepts only a loopback v1 endpoint, fixed model alias and bounded prompt", () => {
    expect(validateAgentRunRequest(validRequest).gatewayBaseUrl).toBe("http://127.0.0.1:10100/v1");
    expect(() =>
      validateAgentRunRequest({ ...validRequest, gatewayBaseUrl: "https://example.com/v1" }),
    ).toThrow(/loopback/);
    expect(() => validateAgentRunRequest({ ...validRequest, modelId: "other" as never })).toThrow(
      /alias/,
    );
    expect(() => validateAgentRunRequest({ ...validRequest, prompt: "x".repeat(4097) })).toThrow(
      /4096/,
    );
    expect(() =>
      validateAgentRunRequest({
        ...validRequest,
        requiredTools: [PLAN_MODEL_START_TOOL],
      }),
    ).toThrow(/required tool set/);
  });

  it("uses the Rust runtime catalog before creating a non-executing model plan", async () => {
    const faux = createFauxCore({
      api: "openai-completions",
      provider: "hal100-test",
      models: [{ id: AGENT_MODEL_ALIAS }],
    });
    faux.setResponses([
      fauxAssistantMessage(
        fauxToolCall(RUNTIME_CATALOG_TOOL, { detail: "summary" }, { id: "tool-catalog-1" }),
        { stopReason: "toolUse" },
      ),
      fauxAssistantMessage("我已经读取目录，但暂时只给出文字说明。", {
        stopReason: "stop",
      }),
      fauxAssistantMessage(
        fauxToolCall(PLAN_MODEL_START_TOOL, { modelId: "managed-model-1" }, { id: "tool-plan-1" }),
        { stopReason: "toolUse" },
      ),
      fauxAssistantMessage("计划已经生成，但尚未执行；请在 HAL100 原生窗口确认。", {
        stopReason: "stop",
      }),
    ]);
    let bridge: ToolBrokerBridge;
    bridge = new ToolBrokerBridge((envelope) => {
      const request = envelope.payload as { toolCallId: string; toolName: string };
      queueMicrotask(() => {
        bridge.acceptResult({
          protocolVersion: AGENT_RPC_VERSION,
          id: envelope.id,
          kind: "tool.call.result",
          payload: {
            toolCallId: request.toolCallId,
            status: "success",
            output:
              request.toolName === RUNTIME_CATALOG_TOOL
                ? { models: [{ id: "managed-model-1", displayName: "Qwen" }] }
                : { planId: "agent-plan-1", requiresNativeConfirmation: true },
          },
        });
      });
    });

    const result = await runPiAgent(
      "run-plan",
      {
        ...validRequest,
        prompt: "切换到 Qwen 模型",
        requiredTools: [RUNTIME_CATALOG_TOOL, PLAN_MODEL_START_TOOL],
      },
      bridge,
      { streamFn: faux.streamSimple, model: faux.getModel() as never },
    );

    expect(result.toolNames).toEqual([RUNTIME_CATALOG_TOOL, PLAN_MODEL_START_TOOL]);
    expect(result.completedToolCalls).toBe(2);
    expect(faux.state.callCount).toBe(4);
  });

  it("forces the exact required tool sequence instead of letting the model choose any tool", () => {
    const requirements = {
      requiredTools: [SYSTEM_SUMMARY_TOOL, RUNTIME_CATALOG_TOOL, PLAN_MODEL_START_TOOL],
    };

    expect(nextRequiredAgentTool(requirements, [])).toBe(SYSTEM_SUMMARY_TOOL);
    expect(nextRequiredAgentTool(requirements, [SYSTEM_SUMMARY_TOOL])).toBe(RUNTIME_CATALOG_TOOL);
    expect(nextRequiredAgentTool(requirements, [SYSTEM_SUMMARY_TOOL, RUNTIME_CATALOG_TOOL])).toBe(
      PLAN_MODEL_START_TOOL,
    );
    expect(
      nextRequiredAgentTool(requirements, [
        SYSTEM_SUMMARY_TOOL,
        RUNTIME_CATALOG_TOOL,
        PLAN_MODEL_START_TOOL,
      ]),
    ).toBeUndefined();
  });

  it("orders engine and external Agent plans behind their required Rust inspections", () => {
    const modelRemovalRequirements = {
      requiredTools: [RUNTIME_CATALOG_TOOL, PLAN_MODEL_REMOVAL_TOOL],
    };
    expect(nextRequiredAgentTool(modelRemovalRequirements, [])).toBe(RUNTIME_CATALOG_TOOL);
    expect(nextRequiredAgentTool(modelRemovalRequirements, [RUNTIME_CATALOG_TOOL])).toBe(
      PLAN_MODEL_REMOVAL_TOOL,
    );

    const engineRequirements = {
      requiredTools: [RUNTIME_CATALOG_TOOL, PLAN_ENGINE_INSTALL_TOOL],
    };
    expect(nextRequiredAgentTool(engineRequirements, [])).toBe(RUNTIME_CATALOG_TOOL);
    expect(nextRequiredAgentTool(engineRequirements, [RUNTIME_CATALOG_TOOL])).toBe(
      PLAN_ENGINE_INSTALL_TOOL,
    );

    const externalAgentRequirements = {
      requiredTools: [EXTERNAL_AGENT_STATUS_TOOL, PLAN_EXTERNAL_AGENT_CONFIGURATION_TOOL],
    };
    expect(nextRequiredAgentTool(externalAgentRequirements, [])).toBe(EXTERNAL_AGENT_STATUS_TOOL);
    expect(nextRequiredAgentTool(externalAgentRequirements, [EXTERNAL_AGENT_STATUS_TOOL])).toBe(
      PLAN_EXTERNAL_AGENT_CONFIGURATION_TOOL,
    );

    const disconnectionRequirements = {
      requiredTools: [EXTERNAL_AGENT_STATUS_TOOL, PLAN_EXTERNAL_AGENT_DISCONNECTION_TOOL],
    };
    expect(nextRequiredAgentTool(disconnectionRequirements, [EXTERNAL_AGENT_STATUS_TOOL])).toBe(
      PLAN_EXTERNAL_AGENT_DISCONNECTION_TOOL,
    );
  });

  it("orders model download planning behind bounded search and repository inspection", () => {
    const requirements = {
      requiredTools: [
        MODEL_CATALOG_SEARCH_TOOL,
        MODEL_REPOSITORY_INSPECTION_TOOL,
        PLAN_MODEL_DOWNLOAD_TOOL,
      ],
    };
    expect(nextRequiredAgentTool(requirements, [])).toBe(MODEL_CATALOG_SEARCH_TOOL);
    expect(nextRequiredAgentTool(requirements, [MODEL_CATALOG_SEARCH_TOOL])).toBe(
      MODEL_REPOSITORY_INSPECTION_TOOL,
    );
    expect(
      nextRequiredAgentTool(requirements, [
        MODEL_CATALOG_SEARCH_TOOL,
        MODEL_REPOSITORY_INSPECTION_TOOL,
      ]),
    ).toBe(PLAN_MODEL_DOWNLOAD_TOOL);
    expect(() =>
      validateAgentRunRequest({
        ...validRequest,
        requiredTools: [MODEL_REPOSITORY_INSPECTION_TOOL, PLAN_MODEL_DOWNLOAD_TOOL],
      }),
    ).toThrow(/required tool set/);
  });

  it("orders a diagnostic repair behind the exact report produced in the same run", () => {
    const requirements = {
      requiredTools: [ENVIRONMENT_DIAGNOSTICS_TOOL, PLAN_DIAGNOSTIC_REPAIR_TOOL],
    };
    expect(nextRequiredAgentTool(requirements, [])).toBe(ENVIRONMENT_DIAGNOSTICS_TOOL);
    expect(nextRequiredAgentTool(requirements, [ENVIRONMENT_DIAGNOSTICS_TOOL])).toBe(
      PLAN_DIAGNOSTIC_REPAIR_TOOL,
    );
    expect(
      nextRequiredAgentTool(requirements, [
        ENVIRONMENT_DIAGNOSTICS_TOOL,
        PLAN_DIAGNOSTIC_REPAIR_TOOL,
      ]),
    ).toBeUndefined();
    expect(
      nextRequiredAgentTool(requirements, [ENVIRONMENT_DIAGNOSTICS_TOOL], false),
    ).toBeUndefined();
  });

  it("runs the diagnosis-to-single-repair-plan Pi tool loop without executing a repair", async () => {
    const faux = createFauxCore({
      api: "openai-completions",
      provider: "hal100-test",
      models: [{ id: AGENT_MODEL_ALIAS }],
    });
    faux.setResponses([
      fauxAssistantMessage(
        fauxToolCall(ENVIRONMENT_DIAGNOSTICS_TOOL, { target: "full" }, { id: "tool-diagnostic-1" }),
        { stopReason: "toolUse" },
      ),
      fauxAssistantMessage(
        fauxToolCall(
          PLAN_DIAGNOSTIC_REPAIR_TOOL,
          { reportId: "diagnostic-1", findingId: "finding-1" },
          { id: "tool-repair-1" },
        ),
        { stopReason: "toolUse" },
      ),
      fauxAssistantMessage("已生成一项修复计划，尚未执行，等待原生确认。", {
        stopReason: "stop",
      }),
    ]);
    let bridge: ToolBrokerBridge;
    bridge = new ToolBrokerBridge((envelope) => {
      const request = envelope.payload as { toolCallId: string; toolName: string };
      queueMicrotask(() => {
        bridge.acceptResult({
          protocolVersion: AGENT_RPC_VERSION,
          id: envelope.id,
          kind: "tool.call.result",
          payload: {
            toolCallId: request.toolCallId,
            status: "success",
            output:
              request.toolName === ENVIRONMENT_DIAGNOSTICS_TOOL
                ? {
                    reportId: "diagnostic-1",
                    findings: [
                      {
                        findingId: "finding-1",
                        repairKind: "installLlamaCpp",
                      },
                    ],
                  }
                : { planId: "agent-plan-diagnostic-1", requiresNativeConfirmation: true },
          },
        });
      });
    });

    const result = await runPiAgent(
      "run-diagnostic-repair",
      {
        ...validRequest,
        prompt: "诊断并修复当前最高优先级问题",
        requiredTools: [ENVIRONMENT_DIAGNOSTICS_TOOL, PLAN_DIAGNOSTIC_REPAIR_TOOL],
      },
      bridge,
      { streamFn: faux.streamSimple, model: faux.getModel() as never },
    );

    expect(result.toolNames).toEqual([ENVIRONMENT_DIAGNOSTICS_TOOL, PLAN_DIAGNOSTIC_REPAIR_TOOL]);
    expect(result.answer).toContain("尚未执行");
  });

  it("finishes after diagnosis when Rust reports no safely repairable finding", async () => {
    const faux = createFauxCore({
      api: "openai-completions",
      provider: "hal100-test",
      models: [{ id: AGENT_MODEL_ALIAS }],
    });
    faux.setResponses([
      fauxAssistantMessage(
        fauxToolCall(ENVIRONMENT_DIAGNOSTICS_TOOL, { target: "full" }, { id: "tool-clean-1" }),
        { stopReason: "toolUse" },
      ),
      fauxAssistantMessage("当前报告没有可安全自动修复的问题，因此未生成计划。", {
        stopReason: "stop",
      }),
    ]);
    let bridge: ToolBrokerBridge;
    bridge = new ToolBrokerBridge((envelope) => {
      const request = envelope.payload as { toolCallId: string };
      queueMicrotask(() => {
        bridge.acceptResult({
          protocolVersion: AGENT_RPC_VERSION,
          id: envelope.id,
          kind: "tool.call.result",
          payload: {
            toolCallId: request.toolCallId,
            status: "success",
            output: {
              reportId: "diagnostic-clean",
              findings: [{ findingId: "finding-info", repairKind: null }],
            },
          },
        });
      });
    });

    const result = await runPiAgent(
      "run-diagnostic-clean",
      {
        ...validRequest,
        prompt: "诊断并修复 HAL100 当前问题",
        requiredTools: [ENVIRONMENT_DIAGNOSTICS_TOOL, PLAN_DIAGNOSTIC_REPAIR_TOOL],
      },
      bridge,
      { streamFn: faux.streamSimple, model: faux.getModel() as never },
    );

    expect(result.toolNames).toEqual([ENVIRONMENT_DIAGNOSTICS_TOOL]);
    expect(result.answer).toContain("未生成计划");
  });

  it("rejects a diagnostic repair request without an environment report", () => {
    expect(() =>
      validateAgentRunRequest({
        ...validRequest,
        requiredTools: [PLAN_DIAGNOSTIC_REPAIR_TOOL],
      }),
    ).toThrow(/required tool set/);
  });

  it("rejects a request that tries to require multiple mutating plans", () => {
    expect(() =>
      validateAgentRunRequest({
        ...validRequest,
        requiredTools: [RUNTIME_CATALOG_TOOL, PLAN_MODEL_START_TOOL, PLAN_ENGINE_INSTALL_TOOL],
      }),
    ).toThrow(/required tool set/);
  });

  it("bounds required-tool correction prompts when the model keeps returning text", async () => {
    const faux = createFauxCore({
      api: "openai-completions",
      provider: "hal100-test",
      models: [{ id: AGENT_MODEL_ALIAS }],
    });
    faux.setResponses([
      fauxAssistantMessage("第一次没有调用工具。", { stopReason: "stop" }),
      fauxAssistantMessage("第二次仍然没有调用工具。", { stopReason: "stop" }),
      fauxAssistantMessage("第三次仍然没有调用工具。", { stopReason: "stop" }),
    ]);

    const result = await runPiAgent(
      "run-bounded-correction",
      validRequest,
      new ToolBrokerBridge(() => undefined),
      { streamFn: faux.streamSimple, model: faux.getModel() as never },
    );

    expect(result.completedToolCalls).toBe(0);
    expect(faux.state.callCount).toBe(3);
  });

  it("reduces provider failures to fixed diagnostic codes", async () => {
    const faux = createFauxCore({
      api: "openai-completions",
      provider: "hal100-test",
      models: [{ id: AGENT_MODEL_ALIAS }],
    });
    const bridge = new ToolBrokerBridge(() => undefined);
    const failingStream: StreamFn = () => {
      throw new Error("HTTP 401 included a sensitive provider response");
    };

    await expect(
      runPiAgent("run-auth", validRequest, bridge, {
        streamFn: failingStream,
        model: faux.getModel() as never,
      }),
    ).rejects.toEqual(new AgentRunFailure("gateway_auth_failed"));
  });

  it("sends the session key to the configured loopback Gateway", async () => {
    const requests: Array<{
      authorization: string | undefined;
      maxTokens: number | undefined;
      toolChoice: unknown;
      toolNames: string[];
      url: string | undefined;
    }> = [];
    const server = createServer(async (request, response) => {
      const chunks: Buffer[] = [];
      for await (const chunk of request) chunks.push(Buffer.from(chunk));
      const payload = JSON.parse(Buffer.concat(chunks).toString("utf8")) as {
        max_tokens?: number;
        tool_choice?: unknown;
        tools?: Array<{ function?: { name?: string } }>;
      };
      requests.push({
        authorization: request.headers.authorization,
        maxTokens: payload.max_tokens,
        toolChoice: payload.tool_choice,
        toolNames:
          payload.tools?.flatMap((tool) => (tool.function?.name ? [tool.function.name] : [])) ?? [],
        url: request.url,
      });
      response.writeHead(401, { "content-type": "application/json" });
      response.end(JSON.stringify({ error: { message: "test rejection", type: "auth" } }));
    });
    await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
    const address = server.address();
    if (!address || typeof address === "string") throw new Error("test listener unavailable");
    const bridge = new ToolBrokerBridge(() => undefined);

    try {
      await expect(
        runPiAgent(
          "run-http",
          { ...validRequest, gatewayBaseUrl: `http://127.0.0.1:${address.port}/v1` },
          bridge,
        ),
      ).rejects.toEqual(new AgentRunFailure("gateway_auth_failed"));
    } finally {
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
    }

    expect(requests).toHaveLength(1);
    for (const captured of requests) {
      expect(captured).toEqual({
        authorization: `Bearer ${validRequest.apiKey}`,
        maxTokens: 768,
        toolChoice: "required",
        toolNames: [SYSTEM_SUMMARY_TOOL],
        url: "/v1/chat/completions",
      });
    }
  });

  it.each([
    {
      protocol: "cloudOpenAi" as const,
      expectedPath: "/v1/chat/completions",
      expectedToolChoice: "required",
      expectedAuthHeader: "authorization",
    },
    {
      protocol: "cloudAnthropic" as const,
      expectedPath: "/v1/messages",
      expectedToolChoice: { type: "tool", name: SYSTEM_SUMMARY_TOOL },
      expectedAuthHeader: "x-api-key",
    },
  ])(
    "keeps $protocol on the loopback Gateway without receiving an upstream secret",
    async ({ protocol, expectedPath, expectedToolChoice, expectedAuthHeader }) => {
      const requests: Array<{
        authorization: string | undefined;
        localApiKey: string | undefined;
        maxTokens: number | undefined;
        model: string | undefined;
        toolChoice: unknown;
        toolNames: string[];
        url: string | undefined;
      }> = [];
      const server = createServer(async (request, response) => {
        const chunks: Buffer[] = [];
        for await (const chunk of request) chunks.push(Buffer.from(chunk));
        const payload = JSON.parse(Buffer.concat(chunks).toString("utf8")) as {
          max_tokens?: number;
          model?: string;
          tool_choice?: unknown;
          tools?: Array<{ name?: string; function?: { name?: string } }>;
        };
        requests.push({
          authorization: request.headers.authorization,
          localApiKey:
            typeof request.headers["x-api-key"] === "string"
              ? request.headers["x-api-key"]
              : undefined,
          maxTokens: payload.max_tokens,
          model: payload.model,
          toolChoice: payload.tool_choice,
          toolNames:
            payload.tools?.flatMap((tool) =>
              tool.name ? [tool.name] : tool.function?.name ? [tool.function.name] : [],
            ) ?? [],
          url: request.url,
        });
        response.writeHead(401, { "content-type": "application/json" });
        response.end(JSON.stringify({ error: { message: "test rejection", type: "auth" } }));
      });
      await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
      const address = server.address();
      if (!address || typeof address === "string") throw new Error("test listener unavailable");
      const modelId = "hal100-agent-cloud-contract";

      try {
        await expect(
          runPiAgent(
            `run-${protocol}`,
            {
              ...validRequest,
              gatewayBaseUrl: `http://127.0.0.1:${address.port}/v1`,
              modelId,
              providerProtocol: protocol,
            },
            new ToolBrokerBridge(() => undefined),
          ),
        ).rejects.toEqual(new AgentRunFailure("gateway_auth_failed"));
      } finally {
        await new Promise<void>((resolve, reject) =>
          server.close((error) => (error ? reject(error) : resolve())),
        );
      }

      expect(requests).toHaveLength(1);
      expect(requests[0]).toMatchObject({
        maxTokens: 2_048,
        model: modelId,
        toolChoice: expectedToolChoice,
        toolNames: [SYSTEM_SUMMARY_TOOL],
        url: expectedPath,
      });
      expect(
        expectedAuthHeader === "authorization"
          ? requests[0]?.authorization
          : requests[0]?.localApiKey,
      ).toBe(
        expectedAuthHeader === "authorization"
          ? `Bearer ${validRequest.apiKey}`
          : validRequest.apiKey,
      );
      expect(JSON.stringify(requests)).not.toContain("upstream-cloud-secret");
    },
  );
});
