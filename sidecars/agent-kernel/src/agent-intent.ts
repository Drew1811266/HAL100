import { Agent, type StreamFn } from "@earendil-works/pi-agent-core";
import { contentText, type Model } from "@earendil-works/pi-ai";
import {
  type AgentRunFailureCode,
  type AgentRunRequest,
  createGatewayRuntime,
  validateAgentRunRequest,
} from "./agent-run.js";

const MAX_INTENT_OUTPUT_BYTES = 2 * 1024;
export const AGENT_TASK_INTENT_SCHEMA_VERSION = 1;

export const AGENT_TASK_KIND_KEYS = [
  "inspect_system",
  "inspect_runtime",
  "diagnose_environment",
  "repair_environment",
  "analyze_operational_history",
  "observe_deployment_health",
  "start_model",
  "activate_runtime_profile",
  "stop_model",
  "remove_model",
  "search_model_catalog",
  "inspect_model_repository",
  "download_model",
  "install_engine",
  "remove_engine",
  "inspect_external_agent",
  "configure_external_agent",
  "disconnect_external_agent",
  "install_managed_external_agent",
  "remove_managed_external_agent",
] as const;
export const AGENT_TASK_CLARIFICATION_KEYS = [
  "external_agent_target",
  "managed_ownership",
  "single_mutation_target",
] as const;
export const AGENT_TASK_REJECTION_KEYS = [
  "invalid_prompt",
  "outside_capability_boundary",
  "outside_ownership_boundary",
] as const;

const TASK_KINDS = new Set<string>(AGENT_TASK_KIND_KEYS);
const CLARIFICATION_KINDS = new Set<string>(AGENT_TASK_CLARIFICATION_KEYS);
const REJECTION_REASONS = new Set<string>(AGENT_TASK_REJECTION_KEYS);

export type AgentIntentRequest = Omit<AgentRunRequest, "requiredTools">;

export type AgentIntentProposal =
  | {
      schemaVersion: 1;
      disposition: "task";
      taskKind: string;
      targetId?: string | null;
    }
  | {
      schemaVersion: 1;
      disposition: "clarify";
      clarificationKind: string;
    }
  | {
      schemaVersion: 1;
      disposition: "reject";
      rejectionReason: string;
    }
  | {
      schemaVersion: 1;
      disposition: "unresolved";
    };

export type AgentIntentCompletion =
  | { status: "proposed"; proposal: AgentIntentProposal }
  | { status: "invalid"; errorCode: "invalid_intent_output" }
  | { status: "failed"; errorCode: AgentRunFailureCode };

interface AgentIntentRuntime {
  streamFn: StreamFn;
  model: Model<"openai-completions" | "anthropic-messages">;
  failureCode?: () => AgentRunFailureCode | undefined;
}

const INTENT_SYSTEM_PROMPT =
  "你是HAL100任务意图分类器。只返回一行JSON对象，不得返回Markdown、思考、解释、工具名、参数、权限或Provider。" +
  'schemaVersion固定为1。disposition只能是"task"、"clarify"、"reject"或"unresolved"。' +
  "task时只允许taskKind和可选targetId。taskKind只能从以下值选择：" +
  [...TASK_KINDS].join(",") +
  "。含义依次覆盖：检查硬件、检查运行时、环境诊断、诊断修复、失败历史、部署观测、启动模型、启用已保存运行方案、停止当前模型、移除模型、搜索模型、检查模型仓库、下载模型、安装引擎、移除引擎、检查外部Agent、配置外部Agent、断开外部Agent、安装HAL100私有Agent、移除HAL100私有Agent。" +
  "。外部Agent targetId只能是opencode、pi-coding-agent、openclaw或hermes-agent。" +
  "只有外部Agent任务需要上述targetId；检查硬件、运行时、环境诊断/修复、失败历史、部署观测、运行方案、模型目录/仓库、下载和引擎任务必须完全省略targetId，不能写system、runtime、environment、model、null或自然语言。" +
  "受管安装或移除只支持pi-coding-agent，其他外部Agent不得使用这两类taskKind。" +
  "名称映射必须严格使用：OpenCode=opencode，Pi Coding Agent或Pi=pi-coding-agent，OpenClaw=openclaw，Hermes Agent或Hermes=hermes-agent。出现其中一个名称时目标已经明确，禁止返回external_agent_target。" +
  "外部Agent动作规则：询问当前如何连接、状态或安装情况=inspect_external_agent；迁移、改指向、接到或改用HAL100=configure_external_agent；HAL100通道不要了、停止使用HAL100但保留程序=disconnect_external_agent；程序未安装且要求HAL100维护自己的副本=install_managed_external_agent。" +
  "环境诊断与修复必须互斥：只要求查清问题、报告健康或说明原因=diagnose_environment；只要还要求给修复方案、处理一个可安全修复项或诊断后修复=repair_environment，即使句子先说检查或查毛病也不能降级为纯诊断。" +
  "仅要求说明Gateway、路由、配置机制或工作原理，且没有要求读取当前状态或执行变更时，必须返回unresolved，不得返回task或clarify。" +
  "clarify只用于真正缺信息：完全没有外部Agent名称才用external_agent_target；只说卸载Pi但无法区分HAL100副本和用户安装才用managed_ownership；一个请求修改多个Agent才用single_mutation_target。不得因为措辞陌生而clarify已明确的目标，动作不确定应返回unresolved。clarificationKind只能是" +
  [...CLARIFICATION_KINDS].join(",") +
  "。要求跳过计划或确认直接改文件、运行Shell、任意文件操作时必须reject为outside_capability_boundary；要求删除用户设置、配置或密钥时必须reject为outside_ownership_boundary。rejectionReason只能是" +
  [...REJECTION_REASONS].join(",") +
  '。非外部任务例：机器能否跑模型=>{"schemaVersion":1,"disposition":"task","taskKind":"inspect_system"}；现在跑哪套模型和服务=>{"schemaVersion":1,"disposition":"task","taskKind":"inspect_runtime"}；运行我保存的代码助手方案=>{"schemaVersion":1,"disposition":"task","taskKind":"activate_runtime_profile"}；把当前推理模型停下来=>{"schemaVersion":1,"disposition":"task","taskKind":"stop_model"}；环境哪里不对=>{"schemaVersion":1,"disposition":"task","taskKind":"diagnose_environment"}；查毛病并给一个安全修复方案=>{"schemaVersion":1,"disposition":"task","taskKind":"repair_environment"}；最近为何出错=>{"schemaVersion":1,"disposition":"task","taskKind":"analyze_operational_history"}；上线前短时观察稳不稳=>{"schemaVersion":1,"disposition":"task","taskKind":"observe_deployment_health"}。外部任务例：让OpenCode改走HAL100=>{"schemaVersion":1,"disposition":"task","taskKind":"configure_external_agent","targetId":"opencode"}；Pi不再走HAL100但保留程序=>{"schemaVersion":1,"disposition":"task","taskKind":"disconnect_external_agent","targetId":"pi-coding-agent"}；Hermes现在如何连接=>{"schemaVersion":1,"disposition":"task","taskKind":"inspect_external_agent","targetId":"hermes-agent"}；给HAL100准备私有Pi副本=>{"schemaVersion":1,"disposition":"task","taskKind":"install_managed_external_agent","targetId":"pi-coding-agent"}；跳过计划直接改配置文件=>{"schemaVersion":1,"disposition":"reject","rejectionReason":"outside_capability_boundary"}；清除用户设置和密钥=>{"schemaVersion":1,"disposition":"reject","rejectionReason":"outside_ownership_boundary"}；只说卸载Pi=>{"schemaVersion":1,"disposition":"clarify","clarificationKind":"managed_ownership"}。不能安全判断时返回{"schemaVersion":1,"disposition":"unresolved"}。';

export function validateAgentIntentRequest(request: AgentIntentRequest): AgentIntentRequest {
  if (
    !isRecord(request) ||
    !exactKeys(request, 7, [
      "prompt",
      "gatewayBaseUrl",
      "apiKey",
      "modelId",
      "providerProtocol",
      "contextWindowTokens",
      "maxOutputTokens",
    ])
  ) {
    throw new TypeError("agent intent request shape is invalid");
  }
  const validated = validateAgentRunRequest({ ...request, requiredTools: [] });
  return {
    prompt: validated.prompt,
    gatewayBaseUrl: validated.gatewayBaseUrl,
    apiKey: validated.apiKey,
    modelId: validated.modelId,
    providerProtocol: validated.providerProtocol,
    contextWindowTokens: validated.contextWindowTokens,
    maxOutputTokens: validated.maxOutputTokens,
  };
}

export async function proposePiIntent(
  request: AgentIntentRequest,
  runtimeOverride?: AgentIntentRuntime,
): Promise<AgentIntentCompletion> {
  const validated = validateAgentIntentRequest(request);
  const runtime = runtimeOverride ?? createGatewayRuntime({ ...validated, requiredTools: [] });
  const model = {
    ...runtime.model,
    maxTokens: 128,
    samplingParams: {
      ...runtime.model.samplingParams,
      temperature: 0,
    },
  } as typeof runtime.model;
  const agent = new Agent({
    streamFn: runtime.streamFn,
    toolExecution: "sequential",
    maxRetryDelayMs: 0,
    initialState: {
      systemPrompt: INTENT_SYSTEM_PROMPT,
      model,
      thinkingLevel: "off",
      tools: [],
      messages: [],
    },
  });

  try {
    await agent.prompt(validated.prompt);
  } catch {
    return {
      status: "failed",
      errorCode: runtime.failureCode?.() ?? "model_request_failed",
    };
  }
  if (agent.state.errorMessage) {
    return {
      status: "failed",
      errorCode: runtime.failureCode?.() ?? "model_request_failed",
    };
  }
  const answer = [...agent.state.messages]
    .reverse()
    .find((message) => message.role === "assistant");
  const answerText = answer ? contentText(answer.content).trim() : "";
  const proposal = parseAgentIntentProposal(answerText);
  return proposal
    ? { status: "proposed", proposal }
    : { status: "invalid", errorCode: "invalid_intent_output" };
}

export function parseAgentIntentProposal(output: string): AgentIntentProposal | undefined {
  if (
    !output.startsWith("{") ||
    !output.endsWith("}") ||
    Buffer.byteLength(output, "utf8") > MAX_INTENT_OUTPUT_BYTES
  ) {
    return undefined;
  }
  let value: unknown;
  try {
    value = JSON.parse(output);
  } catch {
    return undefined;
  }
  if (!isRecord(value) || value.schemaVersion !== AGENT_TASK_INTENT_SCHEMA_VERSION) {
    return undefined;
  }

  if (
    value.disposition === "task" &&
    typeof value.taskKind === "string" &&
    TASK_KINDS.has(value.taskKind) &&
    exactKeys(value, value.targetId === undefined ? 3 : 4, [
      "schemaVersion",
      "disposition",
      "taskKind",
      "targetId",
    ]) &&
    (value.targetId === undefined ||
      value.targetId === null ||
      (typeof value.targetId === "string" &&
        value.targetId.length > 0 &&
        value.targetId.length <= 128 &&
        value.targetId.trim() === value.targetId &&
        !/[\p{Cc}]/u.test(value.targetId)))
  ) {
    return value.targetId === undefined
      ? { schemaVersion: 1, disposition: "task", taskKind: value.taskKind }
      : {
          schemaVersion: 1,
          disposition: "task",
          taskKind: value.taskKind,
          targetId: value.targetId,
        };
  }
  if (
    value.disposition === "clarify" &&
    typeof value.clarificationKind === "string" &&
    CLARIFICATION_KINDS.has(value.clarificationKind) &&
    exactKeys(value, 3, ["schemaVersion", "disposition", "clarificationKind"])
  ) {
    return {
      schemaVersion: 1,
      disposition: "clarify",
      clarificationKind: value.clarificationKind,
    };
  }
  if (
    value.disposition === "reject" &&
    typeof value.rejectionReason === "string" &&
    REJECTION_REASONS.has(value.rejectionReason) &&
    exactKeys(value, 3, ["schemaVersion", "disposition", "rejectionReason"])
  ) {
    return {
      schemaVersion: 1,
      disposition: "reject",
      rejectionReason: value.rejectionReason,
    };
  }
  if (value.disposition === "unresolved" && exactKeys(value, 2, ["schemaVersion", "disposition"])) {
    return { schemaVersion: 1, disposition: "unresolved" };
  }
  return undefined;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function exactKeys(value: Record<string, unknown>, count: number, allowed: string[]): boolean {
  const keys = Object.keys(value);
  return keys.length === count && keys.every((key) => allowed.includes(key));
}
