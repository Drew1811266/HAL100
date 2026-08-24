import { Agent, type StreamFn } from "@earendil-works/pi-agent-core";
import { contentText, createModels, createProvider, type Model } from "@earendil-works/pi-ai";
import { anthropicMessagesApi } from "@earendil-works/pi-ai/api/anthropic-messages.lazy";
import { openAICompletionsApi } from "@earendil-works/pi-ai/api/openai-completions.lazy";
import {
  ENVIRONMENT_DIAGNOSTICS_TOOL,
  EXTERNAL_AGENT_STATUS_TOOL,
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
  type ToolBrokerBridge,
} from "./tool-bridge.js";

const MAX_PROMPT_BYTES = 4 * 1024;
const MAX_API_KEY_BYTES = 512;
const AGENT_MODEL_ALIAS = "hal100-agent";
const CLOUD_AGENT_ROUTE_PREFIX = "hal100-agent-cloud-";
const MAX_REQUIRED_TOOL_PROMPTS = 3;
export const MAX_REQUIRED_TOOLS = 4;
export const MAX_ACTION_PLANS = 1;

export type AgentProviderProtocol = "localOpenAi" | "cloudOpenAi" | "cloudAnthropic";

export interface AgentRunRequest {
  prompt: string;
  requiredTools: readonly string[];
  gatewayBaseUrl: string;
  apiKey: string;
  modelId: string;
  providerProtocol: AgentProviderProtocol;
}

export interface AgentRunResult {
  runId: string;
  answer: string;
  registeredToolCount: number;
  completedToolCalls: number;
  toolNames: string[];
}

export type AgentRunFailureCode =
  | "gateway_auth_failed"
  | "gateway_route_failed"
  | "gateway_request_invalid"
  | "gateway_unreachable"
  | "model_request_failed"
  | "empty_agent_answer";

type AgentToolRequirements = Pick<AgentRunRequest, "requiredTools">;

export const AGENT_TOOL_NAMES = [
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
] as const;

export const ACTION_PLAN_TOOLS = new Set<string>([
  PLAN_MODEL_START_TOOL,
  PLAN_MODEL_REMOVAL_TOOL,
  PLAN_DIAGNOSTIC_REPAIR_TOOL,
  PLAN_ENGINE_INSTALL_TOOL,
  PLAN_ENGINE_REMOVE_TOOL,
  PLAN_EXTERNAL_AGENT_CONFIGURATION_TOOL,
  PLAN_EXTERNAL_AGENT_DISCONNECTION_TOOL,
  PLAN_EXTERNAL_AGENT_INSTALLATION_TOOL,
  PLAN_MANAGED_EXTERNAL_AGENT_REMOVAL_TOOL,
  PLAN_MODEL_DOWNLOAD_TOOL,
]);

export const TOOL_PREREQUISITES = new Map<string, readonly string[]>([
  [PLAN_MODEL_START_TOOL, [RUNTIME_CATALOG_TOOL]],
  [PLAN_MODEL_REMOVAL_TOOL, [RUNTIME_CATALOG_TOOL]],
  [PLAN_DIAGNOSTIC_REPAIR_TOOL, [ENVIRONMENT_DIAGNOSTICS_TOOL]],
  [PLAN_ENGINE_INSTALL_TOOL, [RUNTIME_CATALOG_TOOL]],
  [PLAN_ENGINE_REMOVE_TOOL, [RUNTIME_CATALOG_TOOL]],
  [PLAN_EXTERNAL_AGENT_CONFIGURATION_TOOL, [EXTERNAL_AGENT_STATUS_TOOL]],
  [PLAN_EXTERNAL_AGENT_DISCONNECTION_TOOL, [EXTERNAL_AGENT_STATUS_TOOL]],
  [PLAN_EXTERNAL_AGENT_INSTALLATION_TOOL, [EXTERNAL_AGENT_STATUS_TOOL]],
  [PLAN_MANAGED_EXTERNAL_AGENT_REMOVAL_TOOL, [EXTERNAL_AGENT_STATUS_TOOL]],
  [MODEL_REPOSITORY_INSPECTION_TOOL, [MODEL_CATALOG_SEARCH_TOOL]],
  [PLAN_MODEL_DOWNLOAD_TOOL, [MODEL_REPOSITORY_INSPECTION_TOOL]],
]);

export function nextRequiredAgentTool(
  request: AgentToolRequirements,
  completedToolNames: Iterable<string>,
  diagnosticRepairAvailable: boolean | undefined = true,
): string | undefined {
  const completedTools = new Set(completedToolNames);
  for (const toolName of request.requiredTools) {
    if (toolName === PLAN_DIAGNOSTIC_REPAIR_TOOL && diagnosticRepairAvailable === false) {
      continue;
    }
    if (!completedTools.has(toolName)) return toolName;
  }
  return undefined;
}

export class AgentRunFailure extends Error {
  constructor(readonly code: AgentRunFailureCode) {
    super(code);
    this.name = "AgentRunFailure";
  }
}

interface AgentRuntimeOverride {
  streamFn: StreamFn;
  model: Model<"openai-completions" | "anthropic-messages">;
  failureCode?: () => AgentRunFailureCode | undefined;
}

export async function runPiAgent(
  runId: string,
  request: AgentRunRequest,
  bridge: ToolBrokerBridge,
  runtimeOverride?: AgentRuntimeOverride,
): Promise<AgentRunResult> {
  const validated = validateAgentRunRequest(request);
  const runtime = runtimeOverride ?? createGatewayRuntime(validated);
  const tools = bridge.createAgentTools(runId);
  const completedToolNames: string[] = [];
  const agent = new Agent({
    streamFn: runtime.streamFn,
    toolExecution: "sequential",
    maxRetryDelayMs: 0,
    initialState: {
      systemPrompt:
        "你是 HAL100 的本地配置 Agent，只负责 HAL100、本地模型和推理环境。" +
        "不得充当通用聊天助手；超出范围时只说明职责边界。" +
        "不得声称执行未提供的系统操作，不得生成或假装运行 shell 命令。" +
        "安装、卸载、删除或改变配置只能使用 HAL100 提供的白名单计划工具；计划必须经用户原生确认后才能执行。" +
        "用户要求检测电脑配置时，必须调用 hal100.inspect_system_summary；" +
        "用户要求查看模型、引擎或后端状态时，必须调用 hal100.inspect_runtime_catalog。" +
        "用户要求启动或切换本地模型时，必须先调用 hal100.inspect_runtime_catalog，" +
        "再从返回结果复制精确 modelId 调用 hal100.plan_model_start；计划工具绝不代表操作已经执行，必须明确提醒用户在 HAL100 原生窗口确认。" +
        "用户要求删除或移除本地模型时，必须先读取运行环境，再从返回结果复制精确 modelId 调用 hal100.plan_model_removal；托管文件移到废纸篓，外部文件只移除索引。" +
        "用户要求全面诊断HAL100环境时，必须调用 hal100.inspect_environment_diagnostics。" +
        "用户要求分析近期失败、错误历史或调试操作链路时，必须调用 hal100.inspect_operational_history；该工具只返回脱敏事件，不能据此猜测路径或原始日志。" +
        "用户要求部署前检查、运行监测或稳定性观察时，必须调用 hal100.observe_operational_health；该工具只执行固定3次短时采样，不代表长期后台监控。" +
        "用户明确要求诊断并修复时，必须先读取诊断报告；只能选择报告中带 repairKind 的一项，复制精确 reportId 和 findingId 调用 hal100.plan_diagnostic_repair。每次只修复一项，无法自动修复的问题必须如实说明。" +
        "用户要求安装或卸载 llama.cpp 时，必须先读取运行环境，再分别调用 hal100.plan_engine_install 或 hal100.plan_engine_remove。" +
        "用户要求检查 OpenCode、Pi Coding Agent、OpenClaw 或 Hermes Agent 时，必须按用户点名的软件复制固定 integrationId，调用 hal100.inspect_external_agent。" +
        "用户明确要求安装外部 Agent 时，必须先检查同一 integrationId，再调用 hal100.plan_external_agent_installation；当前只有 Pi Coding Agent 提供经过验收的 HAL100 私有部署配方，计划工具本身不安装。" +
        "用户明确要求卸载 HAL100 私有 Pi 运行时时，必须先检查同一 integrationId，并且只在状态明确返回 managedInstallation=true 时调用 hal100.plan_managed_external_agent_removal；不得把断开配置、用户自行安装的 Pi 或含糊的卸载请求解释为私有卸载。" +
        "用户要求配置或重新配置外部 Agent 时，必须先检查同一 integrationId，再调用 hal100.plan_external_agent_configuration。" +
        "用户要求断开外部 Agent 时，必须先检查同一 integrationId，再调用 hal100.plan_external_agent_disconnection；只允许移除 HAL100 受管配置和专属凭据。" +
        "不得为任意路径写入、通用Shell、桌面控制或强制切换编造工具调用；当前没有这些能力。" +
        "只能依据工具返回的结构化结果，用简洁中文解释，不得猜测路径、凭据或系统信息。",
      model: runtime.model,
      thinkingLevel: "off",
      tools,
      messages: [],
    },
  });
  agent.subscribe((event) => {
    if (event.type === "tool_execution_end" && !event.isError) {
      completedToolNames.push(event.toolName);
    }
  });

  try {
    let nextPrompt = validated.prompt;
    for (let promptAttempt = 0; promptAttempt < MAX_REQUIRED_TOOL_PROMPTS; promptAttempt += 1) {
      await agent.prompt(nextPrompt);
      const providerFailure = runtime.failureCode?.();
      if (providerFailure) throw new AgentRunFailure(providerFailure);
      const missingTool = nextRequiredAgentTool(
        validated,
        completedToolNames,
        bridge.diagnosticRepairAvailable(),
      );
      if (!missingTool) break;
      nextPrompt =
        `继续完成同一项 HAL100 任务。必须调用 ${missingTool}，` +
        "只能使用现有工具结果中的精确字段；不要只给文字说明。";
    }
  } catch (error) {
    throw new AgentRunFailure(runtime.failureCode?.() ?? classifyModelFailure(error));
  }
  if (agent.state.errorMessage) {
    throw new AgentRunFailure(
      runtime.failureCode?.() ?? classifyModelFailure(agent.state.errorMessage),
    );
  }
  const answer = [...agent.state.messages]
    .reverse()
    .find((message) => message.role === "assistant");
  const answerText = answer ? contentText(answer.content).trim() : "";
  if (!answerText) {
    throw new AgentRunFailure("empty_agent_answer");
  }

  return {
    runId,
    answer: answerText,
    registeredToolCount: tools.length,
    completedToolCalls: completedToolNames.length,
    toolNames: completedToolNames,
  };
}

function classifyModelFailure(error: unknown): AgentRunFailureCode {
  const message = (error instanceof Error ? error.message : String(error)).toLowerCase();
  if (
    message.includes("401") ||
    message.includes("unauthorized") ||
    message.includes("authentication") ||
    message.includes("api key")
  ) {
    return "gateway_auth_failed";
  }
  if (
    message.includes("404") ||
    message.includes("not found") ||
    message.includes("unknown model")
  ) {
    return "gateway_route_failed";
  }
  if (
    message.includes("400") ||
    message.includes("bad request") ||
    message.includes("invalid request")
  ) {
    return "gateway_request_invalid";
  }
  if (
    message.includes("fetch failed") ||
    message.includes("connection") ||
    message.includes("econnrefused")
  ) {
    return "gateway_unreachable";
  }
  return "model_request_failed";
}

export function validateAgentRunRequest(request: AgentRunRequest): AgentRunRequest {
  if (
    typeof request.prompt !== "string" ||
    request.prompt.trim().length === 0 ||
    Buffer.byteLength(request.prompt, "utf8") > MAX_PROMPT_BYTES
  ) {
    throw new TypeError("agent prompt must contain between 1 and 4096 UTF-8 bytes");
  }
  if (!Array.isArray(request.requiredTools)) {
    throw new TypeError("agent required tool set is invalid");
  }
  const allowedTools = new Set<string>(AGENT_TOOL_NAMES);
  const requiredTools = request.requiredTools;
  const requiredToolSet = new Set(requiredTools);
  const canonicalTools = AGENT_TOOL_NAMES.filter((toolName) => requiredToolSet.has(toolName));
  if (
    requiredTools.length > MAX_REQUIRED_TOOLS ||
    requiredToolSet.size !== requiredTools.length ||
    requiredTools.some((toolName) => typeof toolName !== "string" || !allowedTools.has(toolName)) ||
    requiredTools.some(
      (toolName) =>
        TOOL_PREREQUISITES.get(toolName)?.some(
          (prerequisite) => !requiredToolSet.has(prerequisite),
        ) === true,
    ) ||
    requiredTools.filter((toolName) => ACTION_PLAN_TOOLS.has(toolName)).length > MAX_ACTION_PLANS ||
    canonicalTools.length !== requiredTools.length ||
    canonicalTools.some((toolName, index) => toolName !== requiredTools[index])
  ) {
    throw new TypeError("agent required tool set is invalid");
  }
  if (
    typeof request.apiKey !== "string" ||
    request.apiKey.length < 24 ||
    Buffer.byteLength(request.apiKey, "utf8") > MAX_API_KEY_BYTES
  ) {
    throw new TypeError("agent Gateway key is invalid");
  }
  if (
    request.providerProtocol !== "localOpenAi" &&
    request.providerProtocol !== "cloudOpenAi" &&
    request.providerProtocol !== "cloudAnthropic"
  ) {
    throw new TypeError("agent provider protocol is invalid");
  }
  if (
    (request.providerProtocol === "localOpenAi" && request.modelId !== AGENT_MODEL_ALIAS) ||
    (request.providerProtocol !== "localOpenAi" &&
      (!request.modelId.startsWith(CLOUD_AGENT_ROUTE_PREFIX) ||
        request.modelId.length > 256 ||
        /[\s\p{Cc}]/u.test(request.modelId)))
  ) {
    throw new TypeError("agent model alias is invalid");
  }
  const gateway = new URL(request.gatewayBaseUrl);
  if (
    gateway.protocol !== "http:" ||
    gateway.hostname !== "127.0.0.1" ||
    gateway.username ||
    gateway.password ||
    gateway.search ||
    gateway.hash ||
    gateway.pathname.replace(/\/$/, "") !== "/v1"
  ) {
    throw new TypeError("agent Gateway must be an IPv4 loopback /v1 endpoint");
  }

  return {
    prompt: request.prompt.trim(),
    requiredTools: [...requiredTools],
    gatewayBaseUrl: gateway.toString().replace(/\/$/, ""),
    apiKey: request.apiKey,
    modelId: request.modelId,
    providerProtocol: request.providerProtocol,
  };
}

export function createGatewayRuntime(request: AgentRunRequest): AgentRuntimeOverride {
  let lastFailureCode: AgentRunFailureCode | undefined;
  const monitoredFetch: typeof globalThis.fetch = async (input, init) => {
    try {
      const response = await globalThis.fetch(input, init);
      lastFailureCode = classifyHttpStatus(response.status);
      return response;
    } catch (error) {
      lastFailureCode = "gateway_unreachable";
      throw error;
    }
  };
  const isAnthropic = request.providerProtocol === "cloudAnthropic";
  const providerBaseUrl = isAnthropic
    ? new URL(request.gatewayBaseUrl).origin
    : request.gatewayBaseUrl;
  const providerId =
    request.providerProtocol === "localOpenAi" ? "hal100-local-agent" : "hal100-cloud-agent";
  const model = (
    isAnthropic
      ? {
          id: request.modelId,
          name: "HAL100 Agent 云端模型",
          api: "anthropic-messages" as const,
          provider: providerId,
          baseUrl: providerBaseUrl,
          reasoning: false,
          input: ["text"] as const,
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 128_000,
          maxTokens: 2_048,
        }
      : {
          id: request.modelId,
          name:
            request.providerProtocol === "localOpenAi"
              ? "HAL100 Agent 本地模型"
              : "HAL100 Agent 云端模型",
          api: "openai-completions" as const,
          provider: providerId,
          baseUrl: providerBaseUrl,
          reasoning: false,
          input: ["text"] as const,
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          // Pi keeps a fixed 4096-token safety reserve. The local runtime therefore
          // uses 6144 so a bounded 768-token answer remains possible after tool context.
          contextWindow: request.providerProtocol === "localOpenAi" ? 6_144 : 128_000,
          maxTokens: request.providerProtocol === "localOpenAi" ? 768 : 2_048,
          samplingParams:
            request.providerProtocol === "localOpenAi"
              ? {
                  top_p: 0.8,
                  top_k: 20,
                  min_p: 0,
                  presence_penalty: 1.5,
                  repetition_penalty: 1,
                }
              : undefined,
          compat: {
            supportsStore: false,
            supportsDeveloperRole: false,
            supportsReasoningEffort: false,
            supportsUsageInStreaming: true,
            maxTokensField: "max_tokens" as const,
            thinkingFormat:
              request.providerProtocol === "localOpenAi" ? ("qwen" as const) : undefined,
            supportsStrictMode: false,
          },
        }
  ) satisfies Model<"openai-completions"> | Model<"anthropic-messages">;
  const provider = createProvider({
    id: providerId,
    name:
      request.providerProtocol === "localOpenAi"
        ? "HAL100 Local Agent Gateway"
        : "HAL100 Cloud Agent Gateway",
    baseUrl: providerBaseUrl,
    auth: {
      apiKey: {
        name: "HAL100 Agent session key",
        resolve: async () => ({ auth: { apiKey: request.apiKey } }),
      },
    },
    models: [model],
    api: isAnthropic ? anthropicMessagesApi() : openAICompletionsApi(),
  });
  const models = createModels();
  models.setProvider(provider);
  const streamFn: StreamFn = (selectedModel, context, options) => {
    const completedTools = new Set(
      context.messages
        .filter((message) => message.role === "toolResult")
        .map((message) => (message as { toolName?: string }).toolName)
        .filter((name): name is string => typeof name === "string"),
    );
    const nextRequiredTool = nextRequiredAgentTool(request, completedTools);
    const constrainedContext = {
      ...context,
      tools: nextRequiredTool
        ? context.tools?.filter((tool) => tool.name === nextRequiredTool)
        : [],
    };
    return models.streamSimple(selectedModel, constrainedContext, {
      ...options,
      fetch: monitoredFetch,
      toolChoice: nextRequiredTool ? (isAnthropic ? "auto" : "required") : "none",
      onPayload:
        isAnthropic && nextRequiredTool
          ? (payload: unknown) => ({
              ...(typeof payload === "object" && payload !== null ? payload : {}),
              tool_choice: { type: "tool", name: nextRequiredTool },
            })
          : options?.onPayload,
    } as never);
  };
  return {
    streamFn,
    model: model as Model<"openai-completions" | "anthropic-messages">,
    failureCode: () => lastFailureCode,
  };
}

function classifyHttpStatus(status: number): AgentRunFailureCode | undefined {
  if (status === 401 || status === 403) return "gateway_auth_failed";
  if (status === 404) return "gateway_route_failed";
  if (status === 400 || status === 422) return "gateway_request_invalid";
  if (status >= 500) return "model_request_failed";
  return undefined;
}

export { AGENT_MODEL_ALIAS };
