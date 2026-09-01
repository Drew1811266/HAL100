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
  buildAgentSystemPrompt,
  CLOUD_AGENT_CONTEXT_WINDOW_TOKENS,
  CLOUD_AGENT_MAX_OUTPUT_TOKENS,
  compactAgentContext,
  LOCAL_AGENT_BASELINE_CONTEXT_WINDOW_TOKENS,
  LOCAL_AGENT_MAX_OUTPUT_TOKENS,
  LOCAL_AGENT_STANDARD_CONTEXT_WINDOW_TOKENS,
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
  PLAN_MODEL_STOP_TOOL,
  PLAN_RUNTIME_PROFILE_ACTIVATION_TOOL,
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
  contextWindowTokens: LOCAL_AGENT_STANDARD_CONTEXT_WINDOW_TOKENS,
  maxOutputTokens: LOCAL_AGENT_MAX_OUTPUT_TOKENS,
} as const;

describe("real HAL100 Agent run boundary", () => {
  it("matches the versioned Agent runtime capacity contract", () => {
    const contract = JSON.parse(
      readFileSync(
        new URL("../../../contracts/agent-runtime/v2-device-capacity.json", import.meta.url),
        "utf8",
      ),
    ) as {
      localProfiles: {
        baseline16k: { contextWindowTokens: number; maxOutputTokens: number; temperature: number };
        standard32k: { contextWindowTokens: number; maxOutputTokens: number; temperature: number };
      };
      cloudRuntime: {
        contextWindowTokens: number;
        maxOutputTokens: number;
      };
    };

    expect(LOCAL_AGENT_BASELINE_CONTEXT_WINDOW_TOKENS).toBe(
      contract.localProfiles.baseline16k.contextWindowTokens,
    );
    expect(LOCAL_AGENT_STANDARD_CONTEXT_WINDOW_TOKENS).toBe(
      contract.localProfiles.standard32k.contextWindowTokens,
    );
    expect(LOCAL_AGENT_MAX_OUTPUT_TOKENS).toBe(contract.localProfiles.standard32k.maxOutputTokens);
    expect(contract.localProfiles.standard32k.temperature).toBe(0);
    expect(CLOUD_AGENT_CONTEXT_WINDOW_TOKENS).toBe(contract.cloudRuntime.contextWindowTokens);
    expect(CLOUD_AGENT_MAX_OUTPUT_TOKENS).toBe(contract.cloudRuntime.maxOutputTokens);
  });

  it("keeps the RPC v13 tool catalog at the twenty compatible names", () => {
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
      PLAN_MODEL_STOP_TOOL,
      PLAN_RUNTIME_PROFILE_ACTIVATION_TOOL,
    ]);
    const manifest = JSON.parse(
      readFileSync(new URL("../../../contracts/agent-rpc/v13-tools.json", import.meta.url), "utf8"),
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

  it("accepts only Rust-listed capacity, a loopback endpoint, fixed alias and bounded prompt", () => {
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
    expect(() => validateAgentRunRequest({ ...validRequest, contextWindowTokens: 65_536 })).toThrow(
      /capacity/,
    );
    expect(() => validateAgentRunRequest({ ...validRequest, maxOutputTokens: 4_096 })).toThrow(
      /capacity/,
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
    expect(faux.state.callCount).toBe(3);
  });

  it("assembles only task-scoped instructions and the direct tool dependency", async () => {
    expect(buildAgentSystemPrompt([SYSTEM_SUMMARY_TOOL])).toContain(SYSTEM_SUMMARY_TOOL);
    expect(buildAgentSystemPrompt([SYSTEM_SUMMARY_TOOL])).not.toContain(PLAN_MODEL_DOWNLOAD_TOOL);
    for (const toolName of AGENT_TOOL_NAMES) {
      const prompt = buildAgentSystemPrompt([toolName]);
      expect(prompt, `missing task instruction for ${toolName}`).toContain(toolName);
      expect(prompt).toContain("原生确认");
      expect(prompt).toContain("通用Shell");
    }

    const runDownloadChain = async (contextAssemblyMode: "bounded" | "legacyFull") => {
      const faux = createFauxCore({
        api: "openai-completions",
        provider: `hal100-${contextAssemblyMode}`,
        models: [{ id: AGENT_MODEL_ALIAS }],
      });
      faux.setResponses([
        fauxAssistantMessage(
          fauxToolCall(MODEL_CATALOG_SEARCH_TOOL, { query: "qwen" }, { id: "tool-search" }),
          { stopReason: "toolUse" },
        ),
        fauxAssistantMessage(
          fauxToolCall(
            MODEL_REPOSITORY_INSPECTION_TOOL,
            { repository: "owner/model" },
            { id: "tool-repository" },
          ),
          { stopReason: "toolUse" },
        ),
        fauxAssistantMessage(
          fauxToolCall(
            PLAN_MODEL_DOWNLOAD_TOOL,
            { remotePath: "model-q4.gguf" },
            { id: "tool-download" },
          ),
          { stopReason: "toolUse" },
        ),
        fauxAssistantMessage("下载计划已生成，尚未执行，等待原生确认。", { stopReason: "stop" }),
      ]);
      let bridge: ToolBrokerBridge;
      bridge = new ToolBrokerBridge((envelope) => {
        const request = envelope.payload as { toolCallId: string; toolName: string };
        const padding = "上下文证据".repeat(128);
        const output =
          request.toolName === MODEL_CATALOG_SEARCH_TOOL
            ? { repositories: [{ repository: "owner/model", summary: padding }] }
            : request.toolName === MODEL_REPOSITORY_INSPECTION_TOOL
              ? { files: [{ remotePath: "model-q4.gguf", summary: padding }] }
              : { planId: "download-plan-1", requiresNativeConfirmation: true, summary: padding };
        queueMicrotask(() => {
          bridge.acceptResult({
            protocolVersion: AGENT_RPC_VERSION,
            id: envelope.id,
            kind: "tool.call.result",
            payload: { toolCallId: request.toolCallId, status: "success", output },
          });
        });
      });

      return runPiAgent(
        `run-${contextAssemblyMode}`,
        {
          ...validRequest,
          prompt: "搜索Qwen并为model-q4.gguf生成下载计划",
          requiredTools: [
            MODEL_CATALOG_SEARCH_TOOL,
            MODEL_REPOSITORY_INSPECTION_TOOL,
            PLAN_MODEL_DOWNLOAD_TOOL,
          ],
        },
        bridge,
        {
          streamFn: faux.streamSimple,
          model: faux.getModel() as never,
          contextAssemblyMode,
        },
      );
    };

    const legacy = await runDownloadChain("legacyFull");
    const bounded = await runDownloadChain("bounded");

    expect(legacy.efficiency.executionModelTurnCount).toBe(4);
    expect(legacy.efficiency.repeatedToolResultTokenEstimate).toBeGreaterThan(0);
    expect(bounded.efficiency).toMatchObject({
      executionModelTurnCount: 3,
      continuationPromptCount: 0,
      providerUsageAvailable: true,
      repeatedToolResultBytes: 0,
      repeatedToolResultTokenEstimate: 0,
      compactedTurnCount: 1,
    });
    expect(bounded.efficiency.sentToolResultTokenEstimate).toBeLessThanOrEqual(
      legacy.efficiency.sentToolResultTokenEstimate * 0.6,
    );
    expect(bounded.efficiency.peakEstimatedInputTokens).toBeLessThan(
      legacy.efficiency.peakEstimatedInputTokens,
    );

    const verticals = JSON.parse(
      readFileSync(
        new URL(
          "../../../contracts/agent-evals/v9-controlled-action-verticals.json",
          import.meta.url,
        ),
        "utf8",
      ),
    ) as { actionPaths: Array<{ requiredTools: string[] }> };
    const legacyTurns = verticals.actionPaths.reduce(
      (total, path) => total + path.requiredTools.length + 1,
      0,
    );
    const boundedTurns = verticals.actionPaths.reduce((total, path) => {
      expect(ACTION_PLAN_TOOLS.has(path.requiredTools.at(-1) ?? "")).toBe(true);
      return total + path.requiredTools.length;
    }, 0);
    expect(1 - boundedTurns / legacyTurns).toBeGreaterThanOrEqual(0.3);
  });

  it("fails safe without detaching a tool result from its assistant call", () => {
    const malformed = {
      messages: [
        { role: "user", content: "test", timestamp: 1 },
        {
          role: "toolResult",
          toolCallId: "missing-call",
          toolName: SYSTEM_SUMMARY_TOOL,
          content: [{ type: "text", text: "{}" }],
          isError: false,
          timestamp: 2,
        },
      ],
    } as never;
    expect(compactAgentContext(malformed).messages).toHaveLength(2);
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
      temperature: number | undefined;
      toolChoice: unknown;
      toolNames: string[];
      url: string | undefined;
    }> = [];
    const server = createServer(async (request, response) => {
      const chunks: Buffer[] = [];
      for await (const chunk of request) chunks.push(Buffer.from(chunk));
      const payload = JSON.parse(Buffer.concat(chunks).toString("utf8")) as {
        max_tokens?: number;
        temperature?: number;
        tool_choice?: unknown;
        tools?: Array<{ function?: { name?: string } }>;
      };
      requests.push({
        authorization: request.headers.authorization,
        maxTokens: payload.max_tokens,
        temperature: payload.temperature,
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
        temperature: 0,
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
              contextWindowTokens: CLOUD_AGENT_CONTEXT_WINDOW_TOKENS,
              maxOutputTokens: CLOUD_AGENT_MAX_OUTPUT_TOKENS,
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
