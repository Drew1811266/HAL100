import { Agent, type StreamFn } from "@earendil-works/pi-agent-core";
import {
  type Context,
  contentText,
  createModels,
  createProvider,
  type Message,
  type Model,
  type Usage,
} from "@earendil-works/pi-ai";
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
  PLAN_MODEL_STOP_TOOL,
  PLAN_RUNTIME_PROFILE_ACTIVATION_TOOL,
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
export const LOCAL_AGENT_BASELINE_CONTEXT_WINDOW_TOKENS = 16_384;
export const LOCAL_AGENT_STANDARD_CONTEXT_WINDOW_TOKENS = 32_768;
export const LOCAL_AGENT_MAX_OUTPUT_TOKENS = 768;
export const CLOUD_AGENT_CONTEXT_WINDOW_TOKENS = 128_000;
export const CLOUD_AGENT_MAX_OUTPUT_TOKENS = 2_048;

export type AgentProviderProtocol = "localOpenAi" | "cloudOpenAi" | "cloudAnthropic";

export interface AgentRunRequest {
  prompt: string;
  requiredTools: readonly string[];
  gatewayBaseUrl: string;
  apiKey: string;
  modelId: string;
  providerProtocol: AgentProviderProtocol;
  contextWindowTokens: number;
  maxOutputTokens: number;
}

export interface AgentRunResult {
  runId: string;
  answer: string;
  registeredToolCount: number;
  completedToolCalls: number;
  toolNames: string[];
  efficiency: AgentRunEfficiency;
}

export interface AgentRunEfficiency {
  contextWindowTokens: number;
  maxOutputTokens: number;
  executionModelTurnCount: number;
  continuationPromptCount: number;
  providerUsageAvailable: boolean;
  reportedInputTokens: number;
  reportedOutputTokens: number;
  peakReportedInputTokens: number;
  peakEstimatedInputTokens: number;
  taskSystemPromptBytes: number;
  compactedTurnCount: number;
  sentToolResultBytes: number;
  sentToolResultTokenEstimate: number;
  repeatedToolResultBytes: number;
  repeatedToolResultTokenEstimate: number;
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
  PLAN_MODEL_STOP_TOOL,
  PLAN_RUNTIME_PROFILE_ACTIVATION_TOOL,
] as const;

export const ACTION_PLAN_TOOLS = new Set<string>([
  PLAN_MODEL_START_TOOL,
  PLAN_MODEL_STOP_TOOL,
  PLAN_MODEL_REMOVAL_TOOL,
  PLAN_DIAGNOSTIC_REPAIR_TOOL,
  PLAN_ENGINE_INSTALL_TOOL,
  PLAN_ENGINE_REMOVE_TOOL,
  PLAN_EXTERNAL_AGENT_CONFIGURATION_TOOL,
  PLAN_EXTERNAL_AGENT_DISCONNECTION_TOOL,
  PLAN_EXTERNAL_AGENT_INSTALLATION_TOOL,
  PLAN_MANAGED_EXTERNAL_AGENT_REMOVAL_TOOL,
  PLAN_MODEL_DOWNLOAD_TOOL,
  PLAN_RUNTIME_PROFILE_ACTIVATION_TOOL,
]);

export const TOOL_PREREQUISITES = new Map<string, readonly string[]>([
  [PLAN_MODEL_START_TOOL, [RUNTIME_CATALOG_TOOL]],
  [PLAN_MODEL_STOP_TOOL, [RUNTIME_CATALOG_TOOL]],
  [PLAN_RUNTIME_PROFILE_ACTIVATION_TOOL, [RUNTIME_CATALOG_TOOL]],
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
  contextAssemblyMode?: "bounded" | "legacyFull";
}

const BASE_SYSTEM_PROMPT =
  "你是 HAL100 的本地配置 Agent，只负责 HAL100、本地模型和推理环境。" +
  "不得充当通用聊天助手；超出范围时只说明职责边界。" +
  "不得声称执行未提供的系统操作，不得生成或假装运行 shell 命令。" +
  "用户询问HAL100 Gateway时，只解释它将经过本地凭据认证的客户端请求路由到当前后端，不得声称已经修改配置。" +
  "安装、卸载、删除或改变配置只能使用 HAL100 提供的白名单计划工具；计划必须经用户原生确认后才能执行。" +
  "不得为任意路径写入、通用Shell、桌面控制或强制切换编造工具调用；当前没有这些能力。" +
  "只能依据工具返回的结构化结果，用简洁中文解释，不得猜测路径、凭据或系统信息。";

const TOOL_SYSTEM_INSTRUCTIONS = new Map<string, string>([
  [SYSTEM_SUMMARY_TOOL, "用户要求检测电脑配置时，必须调用 hal100.inspect_system_summary。"],
  [
    RUNTIME_CATALOG_TOOL,
    "用户要求查看模型、引擎或后端状态时，必须调用 hal100.inspect_runtime_catalog。",
  ],
  [
    PLAN_MODEL_START_TOOL,
    "启动或切换模型时，从运行目录复制精确modelId调用hal100.plan_model_start；计划尚未执行，必须提醒用户在HAL100原生窗口确认。",
  ],
  [
    PLAN_MODEL_STOP_TOOL,
    "停止当前模型时，从运行目录复制精确活动modelId调用hal100.plan_model_stop；计划尚未执行，必须提醒用户在HAL100原生窗口确认。",
  ],
  [
    PLAN_RUNTIME_PROFILE_ACTIVATION_TOOL,
    "运行或切换已保存方案时，先读取运行目录中的runtimeProfiles，再按用户名称选择唯一匹配项并复制精确profileId调用hal100.plan_runtime_profile_activation；ownership=external时contextWindowTokens可以为空，表示容量由外部引擎决定，不得猜测；reviewedPerformance只代表Rust精确匹配同一适配器、支持格、实例配置、引擎身份、模型证据和当前设备后的固定工作负载实测，只能在workloadRevision相同的方案间作为参考，字段缺失表示未知，禁止跨模型、跨设备或跨工作负载泛化；不能唯一匹配、方案需要修复或Rust报告实时身份漂移时必须如实说明。",
  ],
  [
    PLAN_MODEL_REMOVAL_TOOL,
    "移除模型时，从运行目录复制精确modelId调用hal100.plan_model_removal；托管文件移到废纸篓，外部文件只移除索引。",
  ],
  [
    ENVIRONMENT_DIAGNOSTICS_TOOL,
    "全面诊断HAL100环境时，必须调用hal100.inspect_environment_diagnostics。",
  ],
  [
    PLAN_DIAGNOSTIC_REPAIR_TOOL,
    "诊断并修复时，只能从本次报告选择一项带repairKind的发现，复制精确reportId和findingId调用hal100.plan_diagnostic_repair；无法安全修复时如实说明。",
  ],
  [PLAN_ENGINE_INSTALL_TOOL, "安装llama.cpp时，先读取运行环境，再调用hal100.plan_engine_install。"],
  [PLAN_ENGINE_REMOVE_TOOL, "卸载llama.cpp时，先读取运行环境，再调用hal100.plan_engine_remove。"],
  [
    EXTERNAL_AGENT_STATUS_TOOL,
    "检查OpenCode、Pi Coding Agent、OpenClaw或Hermes Agent时，复制用户点名软件的固定integrationId调用hal100.inspect_external_agent。",
  ],
  [
    PLAN_EXTERNAL_AGENT_CONFIGURATION_TOOL,
    "配置或重新配置外部Agent时，检查同一integrationId后调用hal100.plan_external_agent_configuration。",
  ],
  [
    PLAN_EXTERNAL_AGENT_DISCONNECTION_TOOL,
    "断开外部Agent时，检查同一integrationId后调用hal100.plan_external_agent_disconnection；只允许移除HAL100受管配置和专属凭据。",
  ],
  [
    PLAN_EXTERNAL_AGENT_INSTALLATION_TOOL,
    "安装外部Agent时，检查同一integrationId后调用hal100.plan_external_agent_installation；当前只有Pi Coding Agent具备验收过的HAL100私有配方，计划本身不安装。",
  ],
  [
    PLAN_MANAGED_EXTERNAL_AGENT_REMOVAL_TOOL,
    "移除HAL100私有Pi运行时时，只有同一integrationId状态明确返回managedInstallation=true才调用hal100.plan_managed_external_agent_removal；不得删除用户自行安装的Pi。",
  ],
  [
    MODEL_CATALOG_SEARCH_TOOL,
    "搜索公开GGUF候选时，先调用hal100.search_model_catalog，并只使用返回的候选标识。",
  ],
  [
    MODEL_REPOSITORY_INSPECTION_TOOL,
    "检查模型仓库时，从搜索结果复制精确repositoryId调用hal100.inspect_model_repository。",
  ],
  [
    PLAN_MODEL_DOWNLOAD_TOOL,
    "下载模型时，从仓库结果复制精确repositoryId和fileName调用hal100.plan_model_download；计划尚未执行。",
  ],
  [
    OPERATIONAL_HISTORY_TOOL,
    "分析近期失败或调试操作链路时，调用hal100.inspect_operational_history；只能使用脱敏事件，不能猜测路径或原始日志。",
  ],
  [
    OPERATIONAL_HEALTH_OBSERVATION_TOOL,
    "部署前检查或稳定性观察时，调用hal100.observe_operational_health；固定3次短时采样不代表长期后台监控。",
  ],
]);

export function buildAgentSystemPrompt(requiredTools: readonly string[]): string {
  return [
    BASE_SYSTEM_PROMPT,
    ...requiredTools.flatMap((toolName) => {
      const instruction = TOOL_SYSTEM_INSTRUCTIONS.get(toolName);
      return instruction ? [instruction] : [];
    }),
  ].join("");
}

class AgentContextMetricsCollector {
  private readonly seenToolResultCallIds = new Set<string>();
  private modelTurnCount = 0;
  private compactedTurnCount = 0;
  private peakEstimatedInputTokens = 0;
  private sentToolResultBytes = 0;
  private sentToolResultTokenEstimate = 0;
  private repeatedToolResultBytes = 0;
  private repeatedToolResultTokenEstimate = 0;

  observe(original: Context, assembled: Context): void {
    this.modelTurnCount += 1;
    if (assembled.messages.length < original.messages.length) this.compactedTurnCount += 1;
    this.peakEstimatedInputTokens = Math.max(
      this.peakEstimatedInputTokens,
      estimateContextInputTokens(assembled),
    );
    for (const message of assembled.messages) {
      if (message.role !== "toolResult") continue;
      const text = visibleToolResultText(message);
      const bytes = Buffer.byteLength(text, "utf8");
      const tokens = Math.ceil(text.length / 4);
      this.sentToolResultBytes += bytes;
      this.sentToolResultTokenEstimate += tokens;
      if (this.seenToolResultCallIds.has(message.toolCallId)) {
        this.repeatedToolResultBytes += bytes;
        this.repeatedToolResultTokenEstimate += tokens;
      }
      this.seenToolResultCallIds.add(message.toolCallId);
    }
  }

  complete(
    model: Model<"openai-completions" | "anthropic-messages">,
    messages: readonly { role: string; usage?: Usage }[],
    systemPrompt: string,
    continuationPromptCount: number,
  ): AgentRunEfficiency {
    const usages = messages.flatMap((message) =>
      message.role === "assistant" && message.usage ? [message.usage] : [],
    );
    const providerUsageAvailable = usages.some(
      (usage) =>
        usage.input > 0 ||
        usage.output > 0 ||
        usage.cacheRead > 0 ||
        usage.cacheWrite > 0 ||
        usage.totalTokens > 0,
    );
    const reportedInputs = usages.map((usage) => usage.input + usage.cacheRead + usage.cacheWrite);
    return {
      contextWindowTokens: model.contextWindow,
      maxOutputTokens: model.maxTokens,
      executionModelTurnCount: this.modelTurnCount,
      continuationPromptCount,
      providerUsageAvailable,
      reportedInputTokens: reportedInputs.reduce((total, value) => total + value, 0),
      reportedOutputTokens: usages.reduce((total, usage) => total + usage.output, 0),
      peakReportedInputTokens: Math.max(0, ...reportedInputs),
      peakEstimatedInputTokens: this.peakEstimatedInputTokens,
      taskSystemPromptBytes: Buffer.byteLength(systemPrompt, "utf8"),
      compactedTurnCount: this.compactedTurnCount,
      sentToolResultBytes: this.sentToolResultBytes,
      sentToolResultTokenEstimate: this.sentToolResultTokenEstimate,
      repeatedToolResultBytes: this.repeatedToolResultBytes,
      repeatedToolResultTokenEstimate: this.repeatedToolResultTokenEstimate,
    };
  }
}

export function compactAgentContext(context: Context): Context {
  if (context.messages.length <= 1) return context;
  const firstUserIndex = context.messages.findIndex((message) => message.role === "user");
  let lastToolResultIndex = -1;
  for (let index = context.messages.length - 1; index >= 0; index -= 1) {
    if (context.messages[index]?.role === "toolResult") {
      lastToolResultIndex = index;
      break;
    }
  }
  const selectedIndexes = new Set<number>();
  if (firstUserIndex >= 0) selectedIndexes.add(firstUserIndex);
  if (lastToolResultIndex >= 0) {
    const toolResult = context.messages[lastToolResultIndex];
    if (toolResult?.role !== "toolResult") return context;
    let matchingAssistantIndex = -1;
    for (let index = lastToolResultIndex - 1; index >= 0; index -= 1) {
      const candidate = context.messages[index];
      if (
        candidate?.role === "assistant" &&
        candidate.content.some(
          (block) => block.type === "toolCall" && block.id === toolResult.toolCallId,
        )
      ) {
        matchingAssistantIndex = index;
        break;
      }
    }
    if (matchingAssistantIndex < 0) return context;
    selectedIndexes.add(matchingAssistantIndex);
    selectedIndexes.add(lastToolResultIndex);
  }
  for (let index = context.messages.length - 1; index >= 0; index -= 1) {
    if (context.messages[index]?.role === "user") {
      selectedIndexes.add(index);
      break;
    }
  }
  return {
    ...context,
    messages: context.messages.filter((_message, index) => selectedIndexes.has(index)),
  };
}

function visibleToolResultText(message: Extract<Message, { role: "toolResult" }>): string {
  return message.content
    .map((block) => (block.type === "text" ? block.text : `[image:${block.mimeType}]`))
    .join("");
}

function estimateContextInputTokens(context: Context): number {
  let characters = context.systemPrompt?.length ?? 0;
  for (const message of context.messages) {
    if (message.role === "user") {
      characters +=
        typeof message.content === "string"
          ? message.content.length
          : message.content.reduce(
              (total, block) => total + (block.type === "text" ? block.text.length : 4_800),
              0,
            );
    } else if (message.role === "toolResult") {
      characters += visibleToolResultText(message).length;
    } else {
      for (const block of message.content) {
        if (block.type === "text") characters += block.text.length;
        else if (block.type === "thinking") characters += block.thinking.length;
        else characters += block.name.length + JSON.stringify(block.arguments).length;
      }
    }
  }
  if (context.tools?.length) characters += JSON.stringify(context.tools).length;
  return Math.ceil(characters / 4);
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
  let endedAfterActionPlan = false;
  const contextMetrics = new AgentContextMetricsCollector();
  const systemPrompt = buildAgentSystemPrompt(validated.requiredTools);
  const isAnthropic = validated.providerProtocol === "cloudAnthropic";
  const streamFn: StreamFn = (selectedModel, context, options) => {
    const nextRequiredTool = nextRequiredAgentTool(
      validated,
      completedToolNames,
      bridge.diagnosticRepairAvailable(),
    );
    const assembledContext =
      runtime.contextAssemblyMode === "legacyFull" ? context : compactAgentContext(context);
    const constrainedContext = {
      ...assembledContext,
      tools: nextRequiredTool
        ? assembledContext.tools?.filter((tool) => tool.name === nextRequiredTool)
        : [],
    };
    contextMetrics.observe(context, constrainedContext);
    return runtime.streamFn(selectedModel, constrainedContext, {
      ...options,
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
  const agent = new Agent({
    streamFn,
    toolExecution: "sequential",
    afterToolCall: async ({ toolCall, isError }) => {
      if (
        !isError &&
        runtime.contextAssemblyMode !== "legacyFull" &&
        ACTION_PLAN_TOOLS.has(toolCall.name)
      ) {
        endedAfterActionPlan = true;
        return { terminate: true };
      }
      return undefined;
    },
    maxRetryDelayMs: 0,
    initialState: {
      systemPrompt,
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
    let continuationPromptCount = 0;
    for (let promptAttempt = 0; promptAttempt < MAX_REQUIRED_TOOL_PROMPTS; promptAttempt += 1) {
      if (promptAttempt > 0) continuationPromptCount += 1;
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
    const providerFailure = runtime.failureCode?.();
    if (providerFailure) throw new AgentRunFailure(providerFailure);

    if (agent.state.errorMessage) {
      throw new AgentRunFailure(
        runtime.failureCode?.() ?? classifyModelFailure(agent.state.errorMessage),
      );
    }
    const answer = [...agent.state.messages]
      .reverse()
      .find((message) => message.role === "assistant");
    const answerText = endedAfterActionPlan
      ? "已生成一次性受控计划，尚未执行。请在 HAL100 原生窗口确认后再执行。"
      : answer
        ? contentText(answer.content).trim()
        : "";
    if (!answerText) {
      throw new AgentRunFailure("empty_agent_answer");
    }

    return {
      runId,
      answer: answerText,
      registeredToolCount: tools.length,
      completedToolCalls: completedToolNames.length,
      toolNames: completedToolNames,
      efficiency: contextMetrics.complete(
        runtime.model,
        agent.state.messages,
        systemPrompt,
        continuationPromptCount,
      ),
    };
  } catch (error) {
    if (error instanceof AgentRunFailure) throw error;
    throw new AgentRunFailure(runtime.failureCode?.() ?? classifyModelFailure(error));
  }
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
  const localCapacityIsValid =
    (request.contextWindowTokens === LOCAL_AGENT_BASELINE_CONTEXT_WINDOW_TOKENS ||
      request.contextWindowTokens === LOCAL_AGENT_STANDARD_CONTEXT_WINDOW_TOKENS) &&
    request.maxOutputTokens === LOCAL_AGENT_MAX_OUTPUT_TOKENS;
  const cloudCapacityIsValid =
    request.contextWindowTokens === CLOUD_AGENT_CONTEXT_WINDOW_TOKENS &&
    request.maxOutputTokens === CLOUD_AGENT_MAX_OUTPUT_TOKENS;
  if (
    (request.providerProtocol === "localOpenAi" && !localCapacityIsValid) ||
    (request.providerProtocol !== "localOpenAi" && !cloudCapacityIsValid)
  ) {
    throw new TypeError("agent capacity profile is invalid");
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
    contextWindowTokens: request.contextWindowTokens,
    maxOutputTokens: request.maxOutputTokens,
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
          contextWindow: CLOUD_AGENT_CONTEXT_WINDOW_TOKENS,
          maxTokens: CLOUD_AGENT_MAX_OUTPUT_TOKENS,
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
          // Rust selects one contract-listed device tier before process startup. Pi can consume
          // the selected capacity but cannot request or infer a larger context window.
          contextWindow: request.contextWindowTokens,
          maxTokens: request.maxOutputTokens,
          samplingParams:
            request.providerProtocol === "localOpenAi"
              ? {
                  temperature: 0,
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
    return models.streamSimple(selectedModel, context, {
      ...options,
      fetch: monitoredFetch,
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
