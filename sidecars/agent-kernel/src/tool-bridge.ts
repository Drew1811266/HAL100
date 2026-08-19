import type { AgentTool } from "@earendil-works/pi-agent-core";
import { Type } from "typebox";
import { AGENT_RPC_VERSION, type AgentRpcEnvelope } from "./protocol.js";

export const SYSTEM_SUMMARY_TOOL = "hal100.inspect_system_summary";
export const SIMULATED_SYSTEM_SUMMARY_TOOL = SYSTEM_SUMMARY_TOOL;
export const RUNTIME_CATALOG_TOOL = "hal100.inspect_runtime_catalog";
export const PLAN_MODEL_START_TOOL = "hal100.plan_model_start";
export const PLAN_MODEL_REMOVAL_TOOL = "hal100.plan_model_removal";
export const ENVIRONMENT_DIAGNOSTICS_TOOL = "hal100.inspect_environment_diagnostics";
export const PLAN_DIAGNOSTIC_REPAIR_TOOL = "hal100.plan_diagnostic_repair";
export const PLAN_ENGINE_INSTALL_TOOL = "hal100.plan_engine_install";
export const PLAN_ENGINE_REMOVE_TOOL = "hal100.plan_engine_remove";
export const OPENCODE_STATUS_TOOL = "hal100.inspect_opencode_status";
export const PLAN_OPENCODE_CONFIGURATION_TOOL = "hal100.plan_opencode_configuration";
const TOOL_BROKER_TIMEOUT_MS = 5_000;

const systemSummaryParameters = Type.Object(
  {
    detail: Type.Literal("summary"),
  },
  { additionalProperties: false },
);

const runtimeCatalogParameters = Type.Object(
  {
    detail: Type.Literal("summary"),
  },
  { additionalProperties: false },
);

const modelStartParameters = Type.Object(
  {
    modelId: Type.String({ minLength: 1, maxLength: 128 }),
  },
  { additionalProperties: false },
);

const llamaCppTargetParameters = Type.Object(
  {
    target: Type.Literal("llama.cpp"),
  },
  { additionalProperties: false },
);

const openCodeTargetParameters = Type.Object(
  {
    target: Type.Literal("opencode"),
  },
  { additionalProperties: false },
);

const environmentDiagnosticsParameters = Type.Object(
  {
    target: Type.Literal("full"),
  },
  { additionalProperties: false },
);

const diagnosticRepairParameters = Type.Object(
  {
    reportId: Type.String({ minLength: 1, maxLength: 128 }),
    findingId: Type.String({ minLength: 1, maxLength: 128 }),
  },
  { additionalProperties: false },
);

export interface ToolCallRequestPayload {
  runId: string;
  toolCallId: string;
  toolName: string;
  arguments: unknown;
}

export type ToolCallResultPayload =
  | {
      toolCallId: string;
      status: "success";
      output: unknown;
    }
  | {
      toolCallId: string;
      status: "error";
      error: {
        code: string;
        message: string;
      };
    };

type PendingToolCall = {
  toolCallId: string;
  resolve: (output: unknown) => void;
  reject: (error: Error) => void;
  timeout: NodeJS.Timeout;
  removeAbortListener: () => void;
};

export type SendAgentRpcEnvelope = (envelope: AgentRpcEnvelope) => void;

export class ToolBrokerBridge {
  private readonly pending = new Map<string, PendingToolCall>();
  private diagnosticRepairAvailableState: boolean | undefined;
  private nextRequestId = 1;

  constructor(private readonly send: SendAgentRpcEnvelope) {}

  createSystemSummaryTool(runId: string): AgentTool<typeof systemSummaryParameters, unknown> {
    return {
      name: SYSTEM_SUMMARY_TOOL,
      label: "检测这台 Mac",
      description:
        "请求 Rust Tool Broker 按需读取 Apple Silicon 芯片、统一内存、CPU 核心和模型存储可用空间。Sidecar 不直接读取电脑信息。",
      parameters: systemSummaryParameters,
      executionMode: "sequential",
      execute: async (toolCallId, parameters, signal) => {
        const output = await this.requestTool(
          runId,
          toolCallId,
          SYSTEM_SUMMARY_TOOL,
          parameters,
          signal,
        );
        return {
          content: [{ type: "text", text: JSON.stringify(output) }],
          details: output,
        };
      },
    };
  }

  createRuntimeCatalogTool(runId: string): AgentTool<typeof runtimeCatalogParameters, unknown> {
    return {
      name: RUNTIME_CATALOG_TOOL,
      label: "读取 HAL100 运行环境",
      description:
        "请求 Rust Tool Broker 返回 llama.cpp 安装/运行状态、当前活动模型、后端数量与可用本地模型摘要。不返回文件路径或凭据。",
      parameters: runtimeCatalogParameters,
      executionMode: "sequential",
      execute: async (toolCallId, parameters, signal) => {
        const output = await this.requestTool(
          runId,
          toolCallId,
          RUNTIME_CATALOG_TOOL,
          parameters,
          signal,
        );
        return {
          content: [{ type: "text", text: JSON.stringify(output) }],
          details: output,
        };
      },
    };
  }

  createModelStartPlanTool(runId: string): AgentTool<typeof modelStartParameters, unknown> {
    return {
      name: PLAN_MODEL_START_TOOL,
      label: "生成模型启动或切换计划",
      description:
        "使用运行环境工具返回的精确 modelId，请求 Rust 生成一次性计划。此工具只生成计划，不启动模型；用户仍需在 HAL100 原生确认窗口批准。",
      parameters: modelStartParameters,
      executionMode: "sequential",
      execute: async (toolCallId, parameters, signal) => {
        const output = await this.requestTool(
          runId,
          toolCallId,
          PLAN_MODEL_START_TOOL,
          parameters,
          signal,
        );
        return {
          content: [{ type: "text", text: JSON.stringify(output) }],
          details: output,
        };
      },
    };
  }

  createModelRemovalPlanTool(runId: string): AgentTool<typeof modelStartParameters, unknown> {
    return {
      name: PLAN_MODEL_REMOVAL_TOOL,
      label: "生成模型移除计划",
      description:
        "使用运行环境工具返回的精确 modelId，请求 Rust 生成一次性移除计划。托管文件只会移到系统废纸篓，外部模型只移除索引；此工具本身不执行，用户仍需原生确认。",
      parameters: modelStartParameters,
      executionMode: "sequential",
      execute: async (toolCallId, parameters, signal) => {
        const output = await this.requestTool(
          runId,
          toolCallId,
          PLAN_MODEL_REMOVAL_TOOL,
          parameters,
          signal,
        );
        return {
          content: [{ type: "text", text: JSON.stringify(output) }],
          details: output,
        };
      },
    };
  }

  createEnvironmentDiagnosticsTool(
    runId: string,
  ): AgentTool<typeof environmentDiagnosticsParameters, unknown> {
    return {
      name: ENVIRONMENT_DIAGNOSTICS_TOOL,
      label: "诊断 HAL100 运行环境",
      description:
        "请求 Rust 按需检查 Gateway、llama.cpp、模型索引和 OpenCode，返回无本地路径、无凭据且最多 64 项的结构化诊断快照。",
      parameters: environmentDiagnosticsParameters,
      executionMode: "sequential",
      execute: async (toolCallId, parameters, signal) => {
        const output = await this.requestTool(
          runId,
          toolCallId,
          ENVIRONMENT_DIAGNOSTICS_TOOL,
          parameters,
          signal,
        );
        this.diagnosticRepairAvailableState = diagnosticRepairAvailability(output);
        return {
          content: [{ type: "text", text: JSON.stringify(output) }],
          details: output,
        };
      },
    };
  }

  diagnosticRepairAvailable(): boolean | undefined {
    return this.diagnosticRepairAvailableState;
  }

  createDiagnosticRepairPlanTool(
    runId: string,
  ): AgentTool<typeof diagnosticRepairParameters, unknown> {
    return {
      name: PLAN_DIAGNOSTIC_REPAIR_TOOL,
      label: "生成诊断修复计划",
      description:
        "只能复制当前诊断报告返回的 reportId 和 repairable findingId，请求 Rust 生成一次性修复计划。此工具不执行修复，仍需用户原生确认。",
      parameters: diagnosticRepairParameters,
      executionMode: "sequential",
      execute: async (toolCallId, parameters, signal) => {
        const output = await this.requestTool(
          runId,
          toolCallId,
          PLAN_DIAGNOSTIC_REPAIR_TOOL,
          parameters,
          signal,
        );
        return {
          content: [{ type: "text", text: JSON.stringify(output) }],
          details: output,
        };
      },
    };
  }

  createEngineInstallPlanTool(runId: string): AgentTool<typeof llamaCppTargetParameters, unknown> {
    return this.createExactTargetTool(
      runId,
      PLAN_ENGINE_INSTALL_TOOL,
      "生成 llama.cpp 安装计划",
      "请求 Rust 为固定、可校验的 Apple Silicon llama.cpp 构建生成一次性安装计划。只生成计划；用户仍需在 HAL100 原生确认窗口批准。",
      llamaCppTargetParameters,
    );
  }

  createEngineRemovePlanTool(runId: string): AgentTool<typeof llamaCppTargetParameters, unknown> {
    return this.createExactTargetTool(
      runId,
      PLAN_ENGINE_REMOVE_TOOL,
      "生成 llama.cpp 卸载计划",
      "请求 Rust 为 HAL100 托管的 llama.cpp 生成一次性卸载计划。不会删除模型，只生成计划；用户仍需原生确认。",
      llamaCppTargetParameters,
    );
  }

  createOpenCodeStatusTool(runId: string): AgentTool<typeof openCodeTargetParameters, unknown> {
    return this.createExactTargetTool(
      runId,
      OPENCODE_STATUS_TOOL,
      "检查 OpenCode 接入状态",
      "请求 Rust 检查 OpenCode 是否安装、全局配置是否存在以及 HAL100 Provider 是否冲突。Sidecar 不直接读取用户配置文件。",
      openCodeTargetParameters,
    );
  }

  createOpenCodeConfigurationPlanTool(
    runId: string,
  ): AgentTool<typeof openCodeTargetParameters, unknown> {
    return this.createExactTargetTool(
      runId,
      PLAN_OPENCODE_CONFIGURATION_TOOL,
      "生成 OpenCode 配置计划",
      "请求 Rust 生成一次性 OpenCode 配置计划；不覆盖冲突配置，不更改默认模型，且必须由用户在 HAL100 原生窗口确认。",
      openCodeTargetParameters,
    );
  }

  acceptResult(envelope: AgentRpcEnvelope): boolean {
    if (envelope.kind !== "tool.call.result") {
      return false;
    }

    const pending = this.pending.get(envelope.id);
    if (!pending) {
      return false;
    }

    try {
      const payload = assertToolCallResultPayload(envelope.payload);
      if (payload.toolCallId !== pending.toolCallId) {
        throw new TypeError("tool result correlation does not match the pending request");
      }

      this.finishPending(envelope.id, pending);
      if (payload.status === "success") {
        pending.resolve(payload.output);
      } else {
        pending.reject(new Error(`Rust Tool Broker rejected the request: ${payload.error.code}`));
      }
    } catch (error) {
      this.finishPending(envelope.id, pending);
      pending.reject(error instanceof Error ? error : new Error("invalid tool result"));
    }

    return true;
  }

  cancelAll(reason = "Tool Broker connection closed"): void {
    for (const [requestId, pending] of this.pending) {
      this.finishPending(requestId, pending);
      pending.reject(new Error(reason));
    }
  }

  private createExactTargetTool<TParameters extends typeof llamaCppTargetParameters>(
    runId: string,
    toolName: string,
    label: string,
    description: string,
    parameters: TParameters,
  ): AgentTool<TParameters, unknown>;
  private createExactTargetTool<TParameters extends typeof openCodeTargetParameters>(
    runId: string,
    toolName: string,
    label: string,
    description: string,
    parameters: TParameters,
  ): AgentTool<TParameters, unknown>;
  private createExactTargetTool<TParameters extends typeof environmentDiagnosticsParameters>(
    runId: string,
    toolName: string,
    label: string,
    description: string,
    parameters: TParameters,
  ): AgentTool<TParameters, unknown>;
  private createExactTargetTool(
    runId: string,
    toolName: string,
    label: string,
    description: string,
    parameters:
      | typeof llamaCppTargetParameters
      | typeof openCodeTargetParameters
      | typeof environmentDiagnosticsParameters,
  ): AgentTool<
    | typeof llamaCppTargetParameters
    | typeof openCodeTargetParameters
    | typeof environmentDiagnosticsParameters,
    unknown
  > {
    return {
      name: toolName,
      label,
      description,
      parameters,
      executionMode: "sequential",
      execute: async (toolCallId, toolParameters, signal) => {
        const output = await this.requestTool(runId, toolCallId, toolName, toolParameters, signal);
        return {
          content: [{ type: "text", text: JSON.stringify(output) }],
          details: output,
        };
      },
    };
  }

  private requestTool(
    runId: string,
    toolCallId: string,
    toolName: string,
    parameters: unknown,
    signal?: AbortSignal,
  ): Promise<unknown> {
    if (signal?.aborted) {
      return Promise.reject(new Error("tool request aborted"));
    }

    const requestId = `broker-${this.nextRequestId++}`;
    return new Promise((resolve, reject) => {
      const onAbort = () => {
        const pending = this.pending.get(requestId);
        if (pending) {
          this.finishPending(requestId, pending);
          reject(new Error("tool request aborted"));
        }
      };
      signal?.addEventListener("abort", onAbort, { once: true });

      const timeout = setTimeout(() => {
        const pending = this.pending.get(requestId);
        if (pending) {
          this.finishPending(requestId, pending);
          reject(new Error("Rust Tool Broker response timed out"));
        }
      }, TOOL_BROKER_TIMEOUT_MS);

      this.pending.set(requestId, {
        toolCallId,
        resolve,
        reject,
        timeout,
        removeAbortListener: () => signal?.removeEventListener("abort", onAbort),
      });

      const payload: ToolCallRequestPayload = {
        runId,
        toolCallId,
        toolName,
        arguments: parameters,
      };
      try {
        this.send({
          protocolVersion: AGENT_RPC_VERSION,
          id: requestId,
          kind: "tool.call.request",
          payload,
        });
      } catch (error) {
        const pending = this.pending.get(requestId);
        if (pending) {
          this.finishPending(requestId, pending);
        }
        reject(error instanceof Error ? error : new Error("failed to send tool request"));
      }
    });
  }

  private finishPending(requestId: string, pending: PendingToolCall): void {
    clearTimeout(pending.timeout);
    pending.removeAbortListener();
    this.pending.delete(requestId);
  }
}

export function assertToolCallRequestPayload(value: unknown): ToolCallRequestPayload {
  if (
    !isRecord(value) ||
    !isCorrelationId(value.runId) ||
    !isCorrelationId(value.toolCallId) ||
    typeof value.toolName !== "string" ||
    !("arguments" in value)
  ) {
    throw new TypeError("invalid tool.call.request payload");
  }

  return value as unknown as ToolCallRequestPayload;
}

export function assertToolCallResultPayload(value: unknown): ToolCallResultPayload {
  if (!isRecord(value) || !isCorrelationId(value.toolCallId)) {
    throw new TypeError("invalid tool.call.result payload");
  }

  if (value.status === "success" && "output" in value) {
    return value as unknown as ToolCallResultPayload;
  }

  if (
    value.status === "error" &&
    isRecord(value.error) &&
    typeof value.error.code === "string" &&
    typeof value.error.message === "string"
  ) {
    return value as unknown as ToolCallResultPayload;
  }

  throw new TypeError("invalid tool.call.result payload");
}

function diagnosticRepairAvailability(value: unknown): boolean | undefined {
  if (!isRecord(value) || !Array.isArray(value.findings)) return undefined;
  return value.findings.some(
    (finding) =>
      isRecord(finding) &&
      (finding.repairKind === "installLlamaCpp" ||
        finding.repairKind === "configureOpenCode" ||
        finding.repairKind === "removeModelIndex"),
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isCorrelationId(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && Buffer.byteLength(value, "utf8") <= 128;
}
