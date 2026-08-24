import type { AppOverview } from "../lib/desktop-api";

export type UiStatus = "ready" | "attention" | "warning" | "error";

export interface OverviewStatusView {
  status: UiStatus;
  label: string;
  title: string;
  description: string;
  recommendationTitle: string;
  recommendationDescription: string;
  actionLabel: string;
  actionPath: string;
  details: Array<{ label: string; value: string }>;
}

export interface OverviewReadiness {
  engineInstalled: boolean | null;
  readyModelCount: number | null;
}

export function buildOverviewStatus(
  overview: AppOverview,
  setupRequired: boolean,
  readiness: OverviewReadiness = { engineInstalled: null, readyModelCount: null },
): OverviewStatusView {
  const details = [
    { label: "Gateway", value: overview.gatewayState },
    { label: "本机数据", value: overview.databaseState },
    { label: "版本", value: overview.version },
  ];

  if (setupRequired) {
    return {
      status: "attention",
      label: "需要设置",
      title: "基础设置尚未完成",
      description: "HAL100 Core 已连接，完成两项基础偏好后即可开始使用。",
      recommendationTitle: "完成基础设置",
      recommendationDescription: "选择默认模型下载源，并确认是否随系统登录启动。",
      actionLabel: "前往设置",
      actionPath: "/settings?setup=1",
      details,
    };
  }

  if (overview.gatewayState === "异常" || overview.databaseState === "异常") {
    return {
      status: "error",
      label: "需要处理",
      title: "HAL100 运行异常",
      description: "核心状态中存在异常项，建议先进行环境诊断。",
      recommendationTitle: "诊断当前环境",
      recommendationDescription: "让 Agent 检查运行环境并生成可确认的修复计划。",
      actionLabel: "开始诊断",
      actionPath: "/agent",
      details,
    };
  }

  if (overview.gatewayState !== "运行中" || overview.databaseState !== "已就绪") {
    return {
      status: "warning",
      label: "等待就绪",
      title: "部分服务尚未就绪",
      description: "HAL100 Core 已连接，但 Gateway 或本机数据仍在等待。",
      recommendationTitle: "检查运行状态",
      recommendationDescription: "确认本地运行时与 Gateway 当前配置。",
      actionLabel: "查看运行",
      actionPath: "/workspace/runtime",
      details,
    };
  }

  if (readiness.readyModelCount === 0) {
    return {
      status: "attention",
      label: "需要模型",
      title: "核心已就绪，尚未添加模型",
      description: "本地核心、Gateway 与数据已经就绪；添加模型后才能开始本地推理。",
      recommendationTitle: "添加第一个模型",
      recommendationDescription: "从远程目录下载 GGUF，或索引电脑中已有的模型文件。",
      actionLabel: "添加模型",
      actionPath: "/workspace/models",
      details,
    };
  }

  if (readiness.readyModelCount !== null && readiness.engineInstalled === false) {
    return {
      status: "attention",
      label: "需要引擎",
      title: "模型已就绪，推理引擎尚未安装",
      description: "模型文件可以使用；安装 HAL100 托管的 llama.cpp 后即可启动。",
      recommendationTitle: "准备本地推理引擎",
      recommendationDescription: "检查固定版本和来源，并在原生确认后安装 llama.cpp。",
      actionLabel: "前往运行",
      actionPath: "/workspace/runtime",
      details,
    };
  }

  return {
    status: "ready",
    label: "运行正常",
    title: "HAL100 已准备就绪",
    description: "本地核心、Gateway 与本机数据均已就绪。",
    recommendationTitle: "连接常用软件",
    recommendationDescription: "让 OpenCode、Pi Coding Agent 或其他客户端通过 HAL100 使用模型。",
    actionLabel: "前往软件接入",
    actionPath: "/integrations",
    details,
  };
}
