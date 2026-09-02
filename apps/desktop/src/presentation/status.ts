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
  managedModelRunning: boolean;
  configuredServiceCount: number;
  activeInferenceName: string | null;
  activeInferenceReady: boolean;
}

export function buildOverviewStatus(
  overview: AppOverview,
  readiness: OverviewReadiness,
): OverviewStatusView {
  const details = [
    {
      label: "HAL100",
      value: overview.gatewayState === "运行中" ? "运行正常" : overview.gatewayState,
    },
    { label: "本地服务", value: overview.gatewayState },
    { label: "本机数据", value: overview.databaseState },
    { label: "版本", value: overview.version },
  ];

  if (overview.gatewayState === "异常" || overview.databaseState === "异常") {
    return {
      status: "error",
      label: "需要处理",
      title: "HAL100 运行异常",
      description: "当前存在影响使用的问题，建议先进行环境诊断。",
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
      description: "HAL100 已启动，但本地服务或本机数据仍在等待。",
      recommendationTitle: "检查运行状态",
      recommendationDescription: "确认本地运行时与 Gateway 当前配置。",
      actionLabel: "查看运行",
      actionPath: "/workspace/runtime",
      details,
    };
  }

  if (readiness.activeInferenceReady) {
    return {
      status: "ready",
      label: "运行正常",
      title: "HAL100 已准备就绪",
      description: readiness.activeInferenceName
        ? `当前可以使用 ${readiness.activeInferenceName}。`
        : "当前推理服务可以正常使用。",
      recommendationTitle: "连接常用软件",
      recommendationDescription: "让常用 AI 软件通过独立身份使用当前模型。",
      actionLabel: "前往软件接入",
      actionPath: "/integrations",
      details,
    };
  }

  if (readiness.configuredServiceCount > 0) {
    return {
      status: "attention",
      label: "需要选择",
      title: "推理服务已添加，尚未启用",
      description: `已经添加 ${readiness.configuredServiceCount} 个服务，但当前没有可用的活动服务。`,
      recommendationTitle: "选择一个推理服务",
      recommendationDescription: "检查连接状态，然后将可用服务设为当前使用。",
      actionLabel: "查看连接服务",
      actionPath: "/workspace/services",
      details,
    };
  }

  if (readiness.readyModelCount === 0) {
    return {
      status: "attention",
      label: "需要推理方式",
      title: "还没有可用的模型或服务",
      description: "HAL100 已正常启动，现在可以添加本地模型或连接已有服务。",
      recommendationTitle: "选择一种推理方式",
      recommendationDescription: "可以使用本地模型、本机已有服务或云端服务。",
      actionLabel: "连接服务",
      actionPath: "/workspace/services",
      details,
    };
  }

  if (readiness.readyModelCount !== null && readiness.engineInstalled === false) {
    return {
      status: "attention",
      label: "需要引擎",
      title: "模型已就绪，运行环境尚未准备",
      description: "本地模型可以使用；准备 HAL100 本地运行环境后即可启动。",
      recommendationTitle: "准备本地运行",
      recommendationDescription: "检查安装内容，并在确认后准备运行环境。",
      actionLabel: "查看本地运行",
      actionPath: "/workspace/runtime",
      details,
    };
  }

  return {
    status: "attention",
    label: "可以启动",
    title: "本地模型已经准备好",
    description: `已有 ${readiness.readyModelCount ?? 0} 个本地模型，当前尚未运行。`,
    recommendationTitle: "启动一个本地模型",
    recommendationDescription: "选择模型并启动后，软件和 Agent 才能开始使用。",
    actionLabel: "前往本地运行",
    actionPath: "/workspace/runtime",
    details,
  };
}
