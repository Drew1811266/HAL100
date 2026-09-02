import type { AgentComponentState } from "../../lib/desktop-api";

export const AGENT_MODEL_START_TIMEOUT_SECONDS = 90;
export const AGENT_RESPONSE_TIMEOUT_SECONDS = 180;
export const AGENT_SLOW_RUN_SECONDS = 30;

export interface AgentRunProgressInput {
  activeRunId: string | null;
  elapsedSeconds: number;
  kernelState: AgentComponentState;
  modelRuntimeState: AgentComponentState;
  providerMode: "local" | "cloud-single" | "cloud-session";
}

export interface AgentRunProgressCopy {
  description: string;
  slow: boolean;
  title: string;
}

function elapsedCopy(seconds: number) {
  return `已等待 ${Math.max(0, Math.floor(seconds))} 秒`;
}

export function getAgentRunProgress({
  activeRunId,
  elapsedSeconds,
  kernelState,
  modelRuntimeState,
  providerMode,
}: AgentRunProgressInput): AgentRunProgressCopy {
  const local = providerMode === "local";

  if (!activeRunId) {
    return local
      ? {
          title: "正在创建受控任务",
          description: "HAL100 正在校验任务边界，随后会按需启动本地模型。",
          slow: false,
        }
      : {
          title: "等待原生窗口确认",
          description: "确认本次云端发送后才会创建运行；拒绝时不会发送任务内容。",
          slow: false,
        };
  }

  if (kernelState === "error" || modelRuntimeState === "error") {
    return {
      title: "Agent 运行出现错误",
      description: "正在收集确定性错误信息，界面随后会显示可操作的失败原因。",
      slow: true,
    };
  }

  if (kernelState === "starting") {
    if (!local) {
      return {
        title: "正在建立受控云端运行",
        description: "HAL100 正在创建临时凭据和本机 Gateway 路由，不会静默改用本地模型。",
        slow: elapsedSeconds >= AGENT_SLOW_RUN_SECONDS,
      };
    }
    if (modelRuntimeState === "running") {
      return {
        title: "本地模型已就绪，正在启动 Agent 内核",
        description: "模型冷启动已经完成，HAL100 正在建立临时会话和只读工具边界。",
        slow: false,
      };
    }
    if (elapsedSeconds >= 60) {
      return {
        title: "本地模型冷启动时间较长",
        description: `${elapsedCopy(elapsedSeconds)}；启动最长等待 ${AGENT_MODEL_START_TIMEOUT_SECONDS} 秒，达到边界会自动报告错误。`,
        slow: true,
      };
    }
    return {
      title: "正在校验并启动本地模型",
      description: `首次冷启动会校验模型文件和运行参数；启动超时边界为 ${AGENT_MODEL_START_TIMEOUT_SECONDS} 秒。`,
      slow: elapsedSeconds >= AGENT_SLOW_RUN_SECONDS,
    };
  }

  if (kernelState === "running") {
    const provider = local ? "本地模型" : "云端模型";
    if (elapsedSeconds >= AGENT_SLOW_RUN_SECONDS) {
      return {
        title: `${provider}已就绪，Agent 正在处理复杂任务`,
        description: `${elapsedCopy(elapsedSeconds)}；单次模型响应最长等待 ${AGENT_RESPONSE_TIMEOUT_SECONDS / 60} 分钟，达到边界会报告超时，你也可以随时取消。`,
        slow: true,
      };
    }
    return {
      title: `${provider}已就绪，Agent 正在理解并执行任务`,
      description: "HAL100 正在受控工具边界内处理；如需系统变更，完成后仍会单独请求原生确认。",
      slow: false,
    };
  }

  return {
    title: "正在等待运行状态更新",
    description: `${elapsedCopy(elapsedSeconds)}；HAL100 会持续刷新模型和 Agent 内核状态，你也可以随时取消。`,
    slow: elapsedSeconds >= AGENT_SLOW_RUN_SECONDS,
  };
}
