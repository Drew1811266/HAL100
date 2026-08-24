import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  Check,
  ChevronRight,
  Play,
  RefreshCw,
  ShieldCheck,
  Square,
} from "lucide-react";
import { type FormEvent, useEffect, useState } from "react";
import { NavLink } from "react-router-dom";
import {
  type AgentActionPlan,
  type AgentCloudRunPreview,
  type AgentCloudSessionPreview,
  type AgentComponentState,
  type AgentRunResult,
  applyAgentActionPlan,
  type BackendSummary,
  cancelAgentRun,
  getAgentCloudSession,
  getAgentStatus,
  getBackendCatalog,
  getEnvironmentDiagnostics,
  isTauriRuntime,
  previewAgentCloudRun,
  previewAgentCloudSession,
  runAgentPrompt,
  startAgentCloudSession,
  stopAgentCloudSession,
  stopAgentRuntime,
} from "../../lib/desktop-api";
import { EnvironmentDiagnosticsPanel } from "./EnvironmentDiagnosticsPanel";

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

const agentStateCopy: Record<AgentComponentState, { label: string; tone: string }> = {
  unavailable: { label: "不可用", tone: "warning" },
  stopped: { label: "按需待机", tone: "neutral" },
  starting: { label: "正在启动", tone: "warning" },
  running: { label: "运行中", tone: "ok" },
  error: { label: "运行异常", tone: "warning" },
};

const defaultAgentPrompt = "检测这台 Mac，并根据真实硬件给出适合的本地模型参数范围和量化建议。";

type AgentTaskCategory = "diagnostics" | "models" | "integrations";

interface AgentTaskTemplate {
  category: AgentTaskCategory;
  id: string;
  label: string;
  prompt: string;
}

const agentTaskCategories: Record<AgentTaskCategory, string> = {
  diagnostics: "诊断与修复",
  models: "模型与运行",
  integrations: "软件接入",
};

const agentTaskLibrary: AgentTaskTemplate[] = [
  {
    id: "diagnose-environment",
    category: "diagnostics",
    label: "全面诊断环境",
    prompt: "全面诊断 HAL100 当前运行环境，只依据 Rust 诊断结果说明问题，不要执行修复。",
  },
  {
    id: "plan-single-fix",
    category: "diagnostics",
    label: "生成单项修复计划",
    prompt: "诊断并为 HAL100 当前最高优先级且可自动修复的问题生成修复计划；每次只处理一项。",
  },
  {
    id: "analyze-failures",
    category: "diagnostics",
    label: "分析近期失败",
    prompt:
      "调试 HAL100 最近失败原因，读取近期脱敏运维记录并结合当前环境诊断说明最可能的问题；不要执行修复。",
  },
  {
    id: "deployment-check",
    category: "diagnostics",
    label: "部署与运行检查",
    prompt:
      "执行 HAL100 部署前检查并观察运行稳定性；使用固定短时采样说明引擎、路由、后端和外部 Agent 的就绪状态，不要执行修复。",
  },
  {
    id: "hardware-guidance",
    category: "models",
    label: "硬件与模型建议",
    prompt: defaultAgentPrompt,
  },
  {
    id: "search-model",
    category: "models",
    label: "搜索并规划模型下载",
    prompt:
      "在 HAL100 当前默认模型来源搜索 Qwen GGUF，检查合适的公开仓库，并为一个带可信 SHA-256 的 Q4_K_M 文件生成下载计划；不要直接下载。",
  },
  {
    id: "model-status",
    category: "models",
    label: "模型与引擎状态",
    prompt: "列出 HAL100 当前可用模型和引擎状态，并说明当前活动模型。",
  },
  {
    id: "gateway-guidance",
    category: "models",
    label: "Gateway 配置说明",
    prompt: "说明 HAL100 的本地 Gateway 和推理后端应该怎样配置。",
  },
  {
    id: "switch-model",
    category: "models",
    label: "生成模型切换计划",
    prompt: "读取可用模型，并为 Qwen3.5-2B Q4_K_M 生成启动或安全切换计划；不要直接执行。",
  },
  {
    id: "install-engine",
    category: "models",
    label: "生成引擎安装计划",
    prompt: "检查当前引擎状态，并生成安装 llama.cpp 的计划。",
  },
  {
    id: "remove-model",
    category: "models",
    label: "生成模型移除计划",
    prompt: "检查本地模型所有权，并为指定模型生成安全移除计划；不要直接删除任何文件。",
  },
  {
    id: "remove-engine",
    category: "models",
    label: "生成引擎卸载计划",
    prompt: "检查当前引擎和活动请求，并生成安全卸载 llama.cpp 的计划；不要直接执行。",
  },
  {
    id: "install-pi",
    category: "integrations",
    label: "生成 Pi 私有安装计划",
    prompt:
      "检查官方 Pi Coding Agent 是否已安装；如果没有，为固定版本生成 HAL100 私有安装计划，不要修改 PATH、HOME 或用户配置。",
  },
  {
    id: "remove-pi",
    category: "integrations",
    label: "生成 Pi 私有卸载计划",
    prompt:
      "检查 HAL100 私有 Pi Coding Agent 是否存在；如存在，仅为私有运行时生成移入系统废纸篓的卸载计划，保留用户安装、配置和会话。",
  },
  {
    id: "configure-opencode",
    category: "integrations",
    label: "生成 OpenCode 配置计划",
    prompt: "检查 OpenCode 状态，并生成接入 HAL100 Gateway 的配置计划。",
  },
  {
    id: "configure-openclaw",
    category: "integrations",
    label: "生成 OpenClaw 配置计划",
    prompt: "检查 OpenClaw 状态和可用协议，并生成接入 HAL100 Gateway 的配置计划。",
  },
  {
    id: "configure-hermes",
    category: "integrations",
    label: "生成 Hermes 配置计划",
    prompt: "检查 Hermes Agent 状态和运行前置条件，并生成接入 HAL100 Gateway 的配置计划。",
  },
  {
    id: "disconnect-agent",
    category: "integrations",
    label: "生成外部 Agent 断开计划",
    prompt: "检查已接入的外部 Agent，并为指定软件生成只移除 HAL100 受管配置的断开计划。",
  },
];

const agentActionCopy: Record<
  AgentActionPlan["actionKind"],
  { title: string; targetLabel: string; pendingSummary: string }
> = {
  startOrSwitchModel: {
    title: "启动或切换本地模型",
    targetLabel: "目标模型",
    pendingSummary: "Agent 尚未执行任何模型切换",
  },
  downloadModel: {
    title: "下载公开 GGUF 模型",
    targetLabel: "目标文件",
    pendingSummary: "Agent 尚未启动下载或写入模型库",
  },
  removeModel: {
    title: "移除本地模型",
    targetLabel: "目标模型",
    pendingSummary: "Agent 尚未移动模型文件或移除索引",
  },
  installLlamaCpp: {
    title: "安装 llama.cpp",
    targetLabel: "目标引擎",
    pendingSummary: "Agent 尚未下载或安装引擎",
  },
  removeLlamaCpp: {
    title: "卸载 llama.cpp",
    targetLabel: "目标引擎",
    pendingSummary: "Agent 尚未停止或删除引擎",
  },
  installExternalAgent: {
    title: "私有安装外部 Agent",
    targetLabel: "目标软件",
    pendingSummary: "Agent 尚未下载或安装任何软件",
  },
  removeExternalAgent: {
    title: "卸载 HAL100 私有外部 Agent",
    targetLabel: "目标软件",
    pendingSummary: "Agent 尚未移动私有运行时或修改任何用户文件",
  },
  configureExternalAgent: {
    title: "配置外部 Agent",
    targetLabel: "目标软件",
    pendingSummary: "Agent 尚未写入任何配置",
  },
  disconnectExternalAgent: {
    title: "断开外部 Agent",
    targetLabel: "目标软件",
    pendingSummary: "Agent 尚未修改配置或撤销专属凭据",
  },
};

export function AgentPage() {
  const queryClient = useQueryClient();
  const runtime = isTauriRuntime();
  const [prompt, setPrompt] = useState(defaultAgentPrompt);
  const [result, setResult] = useState<AgentRunResult | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [taskCategory, setTaskCategory] = useState<AgentTaskCategory>("diagnostics");
  const [providerMode, setProviderMode] = useState<"local" | "cloud-single" | "cloud-session">(
    "local",
  );
  const [cloudBackendId, setCloudBackendId] = useState("");
  const [cloudModel, setCloudModel] = useState("");
  const [cloudRunPreview, setCloudRunPreview] = useState<AgentCloudRunPreview | null>(null);
  const [cloudSessionPreview, setCloudSessionPreview] = useState<AgentCloudSessionPreview | null>(
    null,
  );
  const status = useQuery({
    queryKey: ["agent-status"],
    queryFn: getAgentStatus,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  const backends = useQuery({
    queryKey: ["backend-catalog"],
    queryFn: getBackendCatalog,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  const cloudSession = useQuery({
    queryKey: ["agent-cloud-session"],
    queryFn: getAgentCloudSession,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  const diagnostics = useQuery({
    queryKey: ["environment-diagnostics"],
    queryFn: getEnvironmentDiagnostics,
    enabled: false,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  useEffect(() => {
    if (cloudSession.data?.active) {
      setProviderMode("cloud-session");
    }
  }, [cloudSession.data?.active]);
  const runMutation = useMutation({
    mutationFn: runAgentPrompt,
    onSuccess: (nextResult) => {
      setResult(nextResult);
      setCloudRunPreview(null);
      setActionMessage(null);
      queryClient.invalidateQueries({ queryKey: ["agent-status"] });
      queryClient.invalidateQueries({ queryKey: ["agent-cloud-session"] });
      queryClient.invalidateQueries({ queryKey: ["usage-dashboard"] });
      queryClient.invalidateQueries({ queryKey: ["audit-log"] });
    },
    onError: () => {
      queryClient.invalidateQueries({ queryKey: ["agent-cloud-session"] });
      queryClient.invalidateQueries({ queryKey: ["audit-log"] });
    },
  });
  const previewRunMutation = useMutation({
    mutationFn: previewAgentCloudRun,
    onSuccess: (preview) => setCloudRunPreview(preview),
  });
  const previewSessionMutation = useMutation({
    mutationFn: previewAgentCloudSession,
    onSuccess: (preview) => setCloudSessionPreview(preview),
  });
  const startSessionMutation = useMutation({
    mutationFn: startAgentCloudSession,
    onSuccess: (nextSession) => {
      queryClient.setQueryData(["agent-cloud-session"], nextSession);
      setProviderMode("cloud-session");
      setCloudSessionPreview(null);
      setResult(null);
      queryClient.invalidateQueries({ queryKey: ["audit-log"] });
    },
  });
  const stopSessionMutation = useMutation({
    mutationFn: stopAgentCloudSession,
    onSuccess: (nextSession) => {
      queryClient.setQueryData(["agent-cloud-session"], nextSession);
      setProviderMode("local");
      setCloudRunPreview(null);
      setCloudSessionPreview(null);
      setResult(null);
      queryClient.invalidateQueries({ queryKey: ["audit-log"] });
    },
  });
  const stopMutation = useMutation({
    mutationFn: stopAgentRuntime,
    onSuccess: (nextStatus) => queryClient.setQueryData(["agent-status"], nextStatus),
  });
  const cancelMutation = useMutation({
    mutationFn: cancelAgentRun,
    onSuccess: (nextStatus) => queryClient.setQueryData(["agent-status"], nextStatus),
  });
  const actionMutation = useMutation({
    mutationFn: applyAgentActionPlan,
    onSuccess: (action, planId) => {
      setActionMessage(action.outcomeSummary);
      if (action.diagnosticReport) {
        queryClient.setQueryData(["environment-diagnostics"], action.diagnosticReport);
      }
      setResult((previous) =>
        previous
          ? {
              ...previous,
              actionPlans: previous.actionPlans.filter((plan) => plan.planId !== planId),
            }
          : previous,
      );
      queryClient.invalidateQueries({ queryKey: ["agent-status"] });
      queryClient.invalidateQueries({ queryKey: ["llama-cpp-status"] });
      queryClient.invalidateQueries({ queryKey: ["gateway-routing"] });
      queryClient.invalidateQueries({ queryKey: ["opencode-detection"] });
      queryClient.invalidateQueries({ queryKey: ["pi-coding-agent-detection"] });
      queryClient.invalidateQueries({ queryKey: ["openclaw-detection"] });
      queryClient.invalidateQueries({ queryKey: ["hermes-agent-detection"] });
      queryClient.invalidateQueries({ queryKey: ["model-downloads"] });
      queryClient.invalidateQueries({ queryKey: ["model-library"] });
      queryClient.invalidateQueries({ queryKey: ["audit-log"] });
    },
    onError: (_error, planId) =>
      setResult((previous) =>
        previous
          ? {
              ...previous,
              actionPlans: previous.actionPlans.filter((plan) => plan.planId !== planId),
            }
          : previous,
      ),
  });

  if (status.isPending || cloudSession.isPending) {
    return <div className="state-message">正在读取 HAL100 Agent 状态…</div>;
  }
  if (status.isError || cloudSession.isError) {
    return (
      <div className="state-message error">{errorMessage(status.error ?? cloudSession.error)}</div>
    );
  }

  const data = status.data;
  const sessionData = cloudSession.data;
  const cloudBackends: BackendSummary[] =
    backends.data?.backends.filter(
      (backend) =>
        backend.enabled &&
        backend.runtimeAvailable &&
        backend.credentialConfigured &&
        (backend.kind === "externalOpenAi" || backend.kind === "externalAnthropic"),
    ) ?? [];
  const effectiveCloudBackendId = cloudBackendId || cloudBackends[0]?.id || "";
  const selectedCloudBackend = cloudBackends.find(
    (backend) => backend.id === effectiveCloudBackendId,
  );
  const cloudTargetReady = Boolean(selectedCloudBackend && cloudModel.trim());
  const sessionActive = sessionData.active;
  const sessionHealthy = sessionData.available && !sessionData.lastErrorCode;
  const agentTransitionPending =
    runMutation.isPending ||
    previewRunMutation.isPending ||
    previewSessionMutation.isPending ||
    startSessionMutation.isPending ||
    stopSessionMutation.isPending;
  const kernelState = runMutation.isPending ? "running" : data.kernelState;
  const modelState =
    runMutation.isPending && providerMode === "local" ? "running" : data.modelRuntimeState;
  const canRun =
    runtime &&
    (providerMode === "local"
      ? data.modelPrepared && !sessionActive
      : providerMode === "cloud-single"
        ? cloudTargetReady && !sessionActive
        : sessionActive
          ? sessionData.available
          : cloudTargetReady) &&
    !runMutation.isPending &&
    !previewRunMutation.isPending &&
    !previewSessionMutation.isPending &&
    !startSessionMutation.isPending &&
    !stopSessionMutation.isPending &&
    !stopMutation.isPending &&
    !actionMutation.isPending;
  const operationError =
    runMutation.error ??
    previewRunMutation.error ??
    previewSessionMutation.error ??
    startSessionMutation.error ??
    stopSessionMutation.error ??
    stopMutation.error ??
    cancelMutation.error ??
    actionMutation.error ??
    diagnostics.error;
  const elapsedSeconds = result
    ? Math.max(0, (result.completedAtMs - result.startedAtMs) / 1000).toFixed(1)
    : null;
  const recommendedTaskIds = data.lastErrorCode
    ? ["diagnose-environment", "analyze-failures", "plan-single-fix"]
    : !data.modelPrepared
      ? ["hardware-guidance", "search-model", "install-engine"]
      : ["deployment-check", "configure-opencode", "model-status"];
  const recommendedTasks = recommendedTaskIds.flatMap((taskId) => {
    const task = agentTaskLibrary.find((candidate) => candidate.id === taskId);
    return task ? [task] : [];
  });
  const libraryTasks = agentTaskLibrary.filter((task) => task.category === taskCategory);
  const agentReady = data.modelPrepared && kernelState !== "error" && !data.lastErrorCode;
  const agentStatusLabel = runMutation.isPending
    ? "正在运行"
    : agentReady
      ? "可以开始任务"
      : "需要准备";

  const updatePrompt = (nextPrompt: string) => {
    setPrompt(nextPrompt);
    setCloudRunPreview(null);
  };

  const submitPrompt = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmed = prompt.trim();
    if (canRun) {
      setResult(null);
      setActionMessage(null);
      const target = { backendId: effectiveCloudBackendId, model: cloudModel.trim() };
      if (providerMode === "cloud-single") {
        if (trimmed) {
          previewRunMutation.mutate({ prompt: trimmed, cloudTarget: target });
        }
      } else if (providerMode === "cloud-session" && !sessionActive) {
        previewSessionMutation.mutate(target);
      } else if (trimmed) {
        runMutation.mutate({ prompt: trimmed, cloudTarget: null });
      }
    }
  };

  return (
    <div className="page-content agent-page">
      <header className="page-header model-page-header">
        <div>
          <p className="eyebrow">本机受控助手</p>
          <h1>HAL100 Agent</h1>
          <p>本地 Qwen 默认运行；云端可仅用一次或绑定当前内存会话，且不会保存聊天历史。</p>
        </div>
        <button
          className="secondary-button refresh-button"
          disabled={
            status.isFetching ||
            cloudSession.isFetching ||
            backends.isFetching ||
            runMutation.isPending
          }
          onClick={() => {
            void status.refetch();
            void cloudSession.refetch();
            void backends.refetch();
          }}
          type="button"
        >
          <RefreshCw
            className={
              status.isFetching || cloudSession.isFetching || backends.isFetching ? "spinning" : ""
            }
            size={14}
          />
          {status.isFetching || cloudSession.isFetching || backends.isFetching
            ? "刷新中…"
            : "刷新状态"}
        </button>
      </header>

      <details className="agent-boundary inline-disclosure">
        <summary>
          <ShieldCheck size={16} />
          <strong>受控执行：Pi 负责推理，Rust 负责授权</strong>
          <span className="disclosure-label">
            <span className="details-closed-copy">了解边界</span>
            <span className="details-open-copy">收起边界</span>
            <ChevronRight size={14} />
          </span>
        </summary>
        <p>
          Agent 只能使用固定工具并为明确操作生成计划；全面诊断覆盖四个外部
          Agent，近期运维历史只返回脱敏事件标识。模型搜索只返回有界公开元数据，下载计划会绑定精确仓库、修订、文件与
          SHA-256。计划不会自动执行，下载、安装、卸载、删除和配置写入仍需原生确认。
        </p>
      </details>

      {sessionActive && (
        <section
          className={`agent-cloud-session-banner ${sessionHealthy ? "" : "warning"}`}
          aria-label="当前云端 Agent 会话"
        >
          <div className="agent-cloud-session-icon">
            {sessionHealthy ? <ShieldCheck size={17} /> : <AlertTriangle size={17} />}
          </div>
          <div>
            <strong>
              当前会话：{sessionData.backendName ?? sessionData.backendId} · {sessionData.model}
            </strong>
            <span>
              {sessionHealthy
                ? "后续任务使用云端；每项任务仍创建独立临时路由与 Key，退出或重启后恢复本地默认。"
                : sessionData.available
                  ? `上次云端任务失败，可重试或退出；绝不会回退本地。错误：${sessionData.lastErrorCode}`
                  : `后端当前不可用，任务会故障关闭且不会回退本地。错误：${sessionData.lastErrorCode ?? "unknown"}`}
            </span>
          </div>
          <button
            className="secondary-button"
            disabled={!runtime || runMutation.isPending || stopSessionMutation.isPending}
            onClick={() => stopSessionMutation.mutate()}
            type="button"
          >
            {stopSessionMutation.isPending ? "正在退出…" : "退出云端会话"}
          </button>
        </section>
      )}

      <section className={`agent-status-summary ${agentReady ? "ready" : "attention"}`}>
        <span className="agent-status-icon">
          {agentReady ? <ShieldCheck size={19} /> : <AlertTriangle size={19} />}
        </span>
        <div>
          <span
            className={`status-pill ${runMutation.isPending ? "warning" : agentReady ? "ok" : "neutral"}`}
          >
            {agentStatusLabel}
          </span>
          <strong>
            {agentReady
              ? "本地 Agent 已准备好"
              : data.modelPrepared
                ? "Agent 正在等待运行时"
                : "需要先准备本地模型"}
          </strong>
          <p>
            {agentReady ? "可直接描述任务；系统变更仍需单独确认。" : "HAL100 会引导你完成缺失项。"}
          </p>
        </div>
        <button
          className="secondary-button"
          disabled={diagnostics.isFetching || runMutation.isPending || actionMutation.isPending}
          onClick={() => void diagnostics.refetch()}
          type="button"
        >
          <RefreshCw className={diagnostics.isFetching ? "spinning" : ""} size={13} />
          {diagnostics.isFetching ? "诊断中…" : "环境诊断"}
        </button>
      </section>

      <details className="agent-technical-details inline-disclosure">
        <summary>
          <span>Agent 技术详情</span>
          <ChevronRight size={14} />
        </summary>
        <dl>
          <div>
            <dt>Pi Core</dt>
            <dd>
              v{data.piVersion} · {agentStateCopy[kernelState].label}
            </dd>
          </div>
          <div>
            <dt>固定模型</dt>
            <dd>
              {data.modelName} · {agentStateCopy[modelState].label}
            </dd>
          </div>
          <div>
            <dt>运行策略</dt>
            <dd>{data.idleTimeoutSeconds / 60} 分钟空闲退出 · Rust 授权</dd>
          </div>
        </dl>
      </details>

      {(diagnostics.data || diagnostics.isFetching || diagnostics.isError) && (
        <EnvironmentDiagnosticsPanel
          report={diagnostics.data}
          error={diagnostics.isError ? diagnostics.error : null}
          isFetching={diagnostics.isFetching}
          disabled={runMutation.isPending || actionMutation.isPending}
          onRefresh={() => void diagnostics.refetch()}
        />
      )}
      {providerMode === "local" && !data.modelPrepared && (
        <section className="agent-missing-model">
          <AlertTriangle size={17} />
          <div>
            <strong>Agent 模型尚未准备好</strong>
            <span>需要已校验的 Qwen3.5-2B Q4_K_M 与 HAL100 托管 llama.cpp。</span>
          </div>
          <NavLink className="secondary-button" to="/workspace/models">
            前往模型库
          </NavLink>
        </section>
      )}

      <section className="agent-workspace">
        <form className="agent-composer" onSubmit={submitPrompt}>
          <div className="agent-section-heading">
            <div>
              <p className="eyebrow">单任务会话</p>
              <h2>诊断、部署检查或生成受控计划</h2>
            </div>
            <span className="agent-composer-context">
              <ShieldCheck size={12} />
              不保留历史
            </span>
          </div>
          <fieldset className="agent-provider-picker">
            <legend>任务处理位置</legend>
            <div className="agent-provider-options">
              <label className={providerMode === "local" ? "selected" : ""}>
                <input
                  checked={providerMode === "local"}
                  disabled={agentTransitionPending || sessionActive}
                  name="agent-provider"
                  onChange={() => {
                    setProviderMode("local");
                    setCloudRunPreview(null);
                    setCloudSessionPreview(null);
                  }}
                  type="radio"
                />
                <strong>本地</strong>
              </label>
              <label className={providerMode !== "local" ? "selected" : ""}>
                <input
                  checked={providerMode !== "local"}
                  disabled={agentTransitionPending || sessionActive}
                  name="agent-provider"
                  onChange={() => {
                    setProviderMode("cloud-single");
                    setCloudRunPreview(null);
                    setCloudSessionPreview(null);
                  }}
                  type="radio"
                />
                <strong>云端</strong>
              </label>
            </div>
            {providerMode !== "local" && (
              <fieldset className="agent-cloud-scope-picker">
                <legend>云端使用范围</legend>
                <label className={providerMode === "cloud-single" ? "selected" : ""}>
                  <input
                    checked={providerMode === "cloud-single"}
                    disabled={agentTransitionPending || sessionActive}
                    name="agent-cloud-scope"
                    onChange={() => {
                      setProviderMode("cloud-single");
                      setCloudRunPreview(null);
                      setCloudSessionPreview(null);
                    }}
                    type="radio"
                  />
                  <strong>仅本次任务</strong>
                </label>
                <label className={providerMode === "cloud-session" ? "selected" : ""}>
                  <input
                    checked={providerMode === "cloud-session"}
                    disabled={agentTransitionPending || sessionActive}
                    name="agent-cloud-scope"
                    onChange={() => {
                      setProviderMode("cloud-session");
                      setCloudRunPreview(null);
                      setCloudSessionPreview(null);
                    }}
                    type="radio"
                  />
                  <strong>当前会话</strong>
                </label>
              </fieldset>
            )}
            <p className="agent-provider-note">
              <ShieldCheck size={13} />
              {providerMode === "local"
                ? "默认在本机处理，任务数据不会离开这台 Mac。"
                : providerMode === "cloud-single"
                  ? "仅发送当前任务；发送范围会先预览，并由原生窗口确认。"
                  : "授权仅存在于当前内存会话，退出或重启后自动恢复本地模式。"}
            </p>
          </fieldset>
          {providerMode !== "local" && !sessionActive && (
            <section className="agent-cloud-target" aria-label="云端 Agent 目标">
              {cloudBackends.length > 0 ? (
                <>
                  <label htmlFor="agent-cloud-backend">已配置后端</label>
                  <select
                    disabled={agentTransitionPending}
                    id="agent-cloud-backend"
                    onChange={(event) => {
                      setCloudBackendId(event.target.value);
                      setCloudRunPreview(null);
                      setCloudSessionPreview(null);
                    }}
                    value={effectiveCloudBackendId}
                  >
                    {cloudBackends.map((backend) => (
                      <option key={backend.id} value={backend.id}>
                        {backend.displayName} ·{" "}
                        {backend.kind === "externalAnthropic" ? "Anthropic" : "OpenAI"}
                      </option>
                    ))}
                  </select>
                  <label htmlFor="agent-cloud-model">模型 ID</label>
                  <input
                    autoComplete="off"
                    disabled={agentTransitionPending}
                    id="agent-cloud-model"
                    maxLength={256}
                    onChange={(event) => {
                      setCloudModel(event.target.value);
                      setCloudRunPreview(null);
                      setCloudSessionPreview(null);
                    }}
                    placeholder={
                      selectedCloudBackend?.kind === "externalAnthropic"
                        ? "例如 claude-sonnet-4-6"
                        : "例如 gpt-4.1-mini"
                    }
                    value={cloudModel}
                  />
                  <small>
                    {selectedCloudBackend?.apiRoot} · API Key 只由 Gateway 从 macOS Keychain 读取
                  </small>
                </>
              ) : (
                <div className="agent-cloud-empty">
                  <span>暂无可用且已配置凭据的 OpenAI/Anthropic 兼容后端。</span>
                  <NavLink to="/workspace/services">前往推理服务配置</NavLink>
                </div>
              )}
            </section>
          )}
          <div className="agent-task-field">
            <div className="agent-task-label">
              <label htmlFor="agent-prompt">任务</label>
              <span>{prompt.length} / 4096</span>
            </div>
            <textarea
              disabled={
                (providerMode === "local" && !data.modelPrepared) ||
                (sessionActive && !sessionData.available) ||
                agentTransitionPending
              }
              id="agent-prompt"
              maxLength={4096}
              onChange={(event) => updatePrompt(event.target.value)}
              value={prompt}
            />
          </div>
          <section className="agent-templates" aria-label="快捷任务模板">
            <div className="agent-templates-heading">
              <strong>推荐任务</strong>
              <span>根据当前状态推荐，选择后仍可编辑</span>
            </div>
            <div className="agent-prompt-shortcuts agent-context-recommendations">
              {recommendedTasks.map((task) => (
                <button
                  className="agent-prompt-chip"
                  disabled={agentTransitionPending}
                  key={task.id}
                  onClick={() => updatePrompt(task.prompt)}
                  type="button"
                >
                  <span>{task.label}</span>
                  <ChevronRight size={12} />
                </button>
              ))}
            </div>
            <details className="agent-task-library">
              <summary>
                打开任务库
                <span>{agentTaskLibrary.length} 项能力</span>
                <ChevronRight size={12} />
              </summary>
              <label>
                <span>任务分类</span>
                <select
                  aria-label="任务分类"
                  onChange={(event) => setTaskCategory(event.target.value as AgentTaskCategory)}
                  value={taskCategory}
                >
                  {(Object.keys(agentTaskCategories) as AgentTaskCategory[]).map((category) => (
                    <option key={category} value={category}>
                      {agentTaskCategories[category]}
                    </option>
                  ))}
                </select>
              </label>
              <div className="agent-prompt-shortcuts">
                {libraryTasks.map((task) => (
                  <button
                    className="agent-prompt-chip"
                    disabled={agentTransitionPending}
                    key={task.id}
                    onClick={() => updatePrompt(task.prompt)}
                    type="button"
                  >
                    <span>{task.label}</span>
                    <ChevronRight size={12} />
                  </button>
                ))}
              </div>
            </details>
          </section>
          {!runtime && (
            <p className="inline-notice">
              浏览器预览不会运行 Agent、启动下载或执行任何写操作；请在 Tauri 开发版中运行。
            </p>
          )}
          {data.lastErrorCode && <p className="inline-error">上次错误代码：{data.lastErrorCode}</p>}
          {operationError && <p className="inline-error">{errorMessage(operationError)}</p>}
          {cloudRunPreview && (
            <section className="agent-cloud-preview" aria-label="云端发送预览">
              <div>
                <ShieldCheck size={15} />
                <strong>脱敏发送预览</strong>
              </div>
              <dl>
                <div>
                  <dt>目标</dt>
                  <dd>{cloudRunPreview.backendName}</dd>
                </div>
                <div>
                  <dt>模型</dt>
                  <dd>{cloudRunPreview.model}</dd>
                </div>
                <div>
                  <dt>任务文字</dt>
                  <dd>{cloudRunPreview.promptBytes} 字节</dd>
                </div>
                <div>
                  <dt>凭据 / 路径</dt>
                  <dd>
                    {cloudRunPreview.sendsCredentialsToSidecar || cloudRunPreview.sendsLocalPaths
                      ? "包含"
                      : "不发送"}
                  </dd>
                </div>
              </dl>
              <p>{cloudRunPreview.confirmationSummary}</p>
              <button
                className="primary-button"
                disabled={!canRun || !prompt.trim()}
                onClick={() =>
                  runMutation.mutate({
                    prompt: prompt.trim(),
                    cloudTarget: {
                      backendId: effectiveCloudBackendId,
                      model: cloudModel.trim(),
                    },
                  })
                }
                type="button"
              >
                <ShieldCheck size={13} />
                在原生窗口确认本次发送
              </button>
            </section>
          )}
          {cloudSessionPreview && !sessionActive && (
            <section className="agent-cloud-preview" aria-label="云端会话授权预览">
              <div>
                <ShieldCheck size={15} />
                <strong>当前内存会话授权预览</strong>
              </div>
              <dl>
                <div>
                  <dt>目标</dt>
                  <dd>{cloudSessionPreview.backendName}</dd>
                </div>
                <div>
                  <dt>模型</dt>
                  <dd>{cloudSessionPreview.model}</dd>
                </div>
                <div>
                  <dt>后续任务</dt>
                  <dd>{cloudSessionPreview.sendsFuturePrompts ? "发送到云端" : "不发送"}</dd>
                </div>
                <div>
                  <dt>聊天历史</dt>
                  <dd>{cloudSessionPreview.storesConversationHistory ? "保存" : "不保存"}</dd>
                </div>
                <div>
                  <dt>工具结果</dt>
                  <dd>{cloudSessionPreview.maySendToolResults ? "可能发送" : "不发送"}</dd>
                </div>
                <div>
                  <dt>凭据 / 路径</dt>
                  <dd>
                    {cloudSessionPreview.sendsCredentialsToSidecar ||
                    cloudSessionPreview.sendsLocalPaths
                      ? "包含"
                      : "不发送"}
                  </dd>
                </div>
              </dl>
              <p>{cloudSessionPreview.confirmationSummary}</p>
              <button
                className="primary-button"
                disabled={!canRun}
                onClick={() =>
                  startSessionMutation.mutate({
                    backendId: effectiveCloudBackendId,
                    model: cloudModel.trim(),
                  })
                }
                type="button"
              >
                <ShieldCheck size={13} />
                在原生窗口确认并启用当前会话
              </button>
            </section>
          )}
          <div className="agent-composer-actions">
            <span className="agent-execution-note">
              <ShieldCheck size={13} />
              系统变更仍需原生确认
            </span>
            <div>
              {runMutation.isPending && (
                <button
                  className="secondary-button"
                  disabled={!runtime || cancelMutation.isPending || data.cancellationRequested}
                  onClick={() => cancelMutation.mutate()}
                  type="button"
                >
                  <Square size={11} />
                  {cancelMutation.isPending || data.cancellationRequested
                    ? "正在取消…"
                    : "取消当前任务"}
                </button>
              )}
              <button
                className="secondary-button"
                disabled={
                  !runtime ||
                  modelState !== "running" ||
                  runMutation.isPending ||
                  stopMutation.isPending
                }
                onClick={() => stopMutation.mutate()}
                type="button"
              >
                <Square size={11} />
                {stopMutation.isPending ? "正在释放…" : "释放模型"}
              </button>
              <button
                className="primary-button"
                disabled={
                  !canRun ||
                  (!prompt.trim() && !(providerMode === "cloud-session" && !sessionActive))
                }
                type="submit"
              >
                <Play size={13} />
                {runMutation.isPending
                  ? "正在执行受控任务…"
                  : previewRunMutation.isPending || previewSessionMutation.isPending
                    ? "正在生成预览…"
                    : providerMode === "cloud-single"
                      ? "预览单次云端发送"
                      : providerMode === "cloud-session"
                        ? sessionActive
                          ? "运行云端会话任务"
                          : "预览会话授权"
                        : "运行本地任务"}
              </button>
            </div>
          </div>
        </form>

        <article
          className={`agent-result${!runMutation.isPending && !result ? " is-empty" : ""}`}
          aria-live="polite"
        >
          <div className="agent-section-heading">
            <div>
              <p className="eyebrow">结果</p>
              <h2>回答与计划</h2>
            </div>
            {elapsedSeconds && <span>{elapsedSeconds} 秒</span>}
          </div>
          {runMutation.isPending && (
            <div className="agent-running-stage" role="status">
              <RefreshCw className="spinning" size={14} />
              <span>正在处理当前任务，可随时取消</span>
            </div>
          )}
          {runMutation.isPending ? (
            <div className="agent-running-detail">
              <strong>
                {providerMode !== "local"
                  ? "正在通过本机 Gateway 请求云端模型"
                  : "正在启动独立模型运行时"}
              </strong>
              <span>
                {providerMode !== "local"
                  ? "失败时会直接报告错误，不会静默改用本地模型。"
                  : "首次运行包含完整模型校验与冷启动，完成后将自动进入空闲倒计时。"}
              </span>
            </div>
          ) : result ? (
            <div className="agent-completed-result">
              {result.actionPlans.map((plan) => {
                const copy = agentActionCopy[plan.actionKind];
                return (
                  <section className="agent-action-plan" key={plan.planId}>
                    <div className="agent-action-plan-heading">
                      <span className="agent-action-plan-icon">
                        <ShieldCheck size={15} />
                      </span>
                      <div>
                        <strong>等待原生确认：{copy.title}</strong>
                        <small>计划 {plan.planId.slice(-8)} · 5 分钟内有效</small>
                      </div>
                    </div>
                    <dl>
                      <div>
                        <dt>{copy.targetLabel}</dt>
                        <dd>{plan.targetName}</dd>
                      </div>
                      <div>
                        <dt>当前状态</dt>
                        <dd>{plan.currentState ?? "等待 Rust 复核"}</dd>
                      </div>
                    </dl>
                    <p>
                      {plan.actionSummary}。{copy.pendingSummary}。
                    </p>
                    {plan.details.length > 0 && (
                      <small className="agent-action-plan-details">
                        {plan.details.join(" · ")}
                      </small>
                    )}
                    <button
                      className="primary-button"
                      disabled={
                        !runtime || actionMutation.isPending || Date.now() > plan.expiresAtMs
                      }
                      onClick={() => actionMutation.mutate(plan.planId)}
                      type="button"
                    >
                      <ShieldCheck size={13} />
                      {actionMutation.isPending ? "等待原生确认…" : "在原生窗口确认并执行"}
                    </button>
                  </section>
                );
              })}
              {actionMessage && <p className="agent-action-success">{actionMessage}</p>}
              <div className="agent-answer">{result.answer}</div>
              <details className="agent-tool-trace">
                <summary>
                  <span>工具轨迹 · {result.toolEvents.length} 项</span>
                  <ChevronRight size={12} />
                </summary>
                <div className="agent-tool-timeline">
                  {result.toolEvents.length > 0 ? (
                    result.toolEvents.map((tool) => (
                      <div key={tool.toolCallId}>
                        <span className="agent-tool-check">
                          {tool.status === "awaiting_confirmation" ? (
                            <ShieldCheck size={11} />
                          ) : (
                            <Check size={11} />
                          )}
                        </span>
                        <div>
                          <strong>{tool.label}</strong>
                          <small>{tool.summary}</small>
                        </div>
                      </div>
                    ))
                  ) : (
                    <span className="agent-no-tool">本次说明未请求系统工具。</span>
                  )}
                </div>
              </details>
              <details className="agent-result-technical">
                <summary>
                  <span>技术详情</span>
                  <ChevronRight size={12} />
                </summary>
                <dl>
                  <div>
                    <dt>模型</dt>
                    <dd>{result.modelName}</dd>
                  </div>
                  <div>
                    <dt>Run ID</dt>
                    <dd>{result.runId}</dd>
                  </div>
                </dl>
              </details>
            </div>
          ) : (
            <div className="agent-empty-result">
              <strong>等待一项 HAL100 管理任务</strong>
              <span>选择推荐任务、打开任务库，或直接输入目标。</span>
            </div>
          )}
        </article>
      </section>
    </div>
  );
}
