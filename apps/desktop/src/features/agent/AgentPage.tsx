import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  Bot,
  Check,
  ChevronRight,
  Play,
  RefreshCw,
  ShieldCheck,
  Square,
} from "lucide-react";
import { type FormEvent, useEffect, useRef, useState } from "react";
import { NavLink } from "react-router-dom";
import { PageHeader } from "../../components/ui/PageHeader";
import {
  type AgentActionPlan,
  type AgentClarificationKind,
  type AgentClarificationOption,
  type AgentCloudRunPreview,
  type AgentCloudSessionPreview,
  type AgentComponentState,
  type AgentExternalAgentChoice,
  type AgentRunResult,
  type AgentTaskCheckpointPhase,
  type AgentTaskEvidenceSource,
  type AgentTaskGraphNodeCheckpointState,
  type AgentTaskVerificationState,
  applyAgentActionPlan,
  type BackendSummary,
  cancelAgentRun,
  cancelAgentTaskGraph,
  continueAgentClarification,
  getAgentCloudSession,
  getAgentStatus,
  getBackendCatalog,
  getEnvironmentDiagnostics,
  getModelLibrary,
  isTauriRuntime,
  previewAgentCloudRun,
  previewAgentCloudSession,
  restoreAgentTaskGraph,
  runAgentPrompt,
  runNextAgentTaskGraphCompensation,
  runNextAgentTaskGraphNode,
  startAgentCloudSession,
  startAgentTaskGraph,
  stopAgentCloudSession,
  stopAgentRuntime,
} from "../../lib/desktop-api";
import { getAgentRunProgress } from "./agent-run-progress";
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

const agentTaskPhaseCopy: Record<AgentTaskCheckpointPhase, string> = {
  draft: "草拟",
  clarifying: "等待澄清",
  inspecting: "检查中",
  planning: "规划中",
  awaitingConfirmation: "等待原生确认",
  executing: "执行中",
  verifying: "复验中",
  completed: "已完成",
  blocked: "已阻塞",
  failed: "失败",
  cancelled: "已取消",
};

const agentTaskVerificationCopy: Record<AgentTaskVerificationState, string> = {
  notStarted: "尚未开始",
  pending: "等待确定性证据",
  satisfied: "目标已满足",
  unsatisfied: "目标尚未满足",
  evidenceUnavailable: "证据不可用",
  failed: "复验失败",
};

const agentTaskEvidenceSourceCopy: Record<AgentTaskEvidenceSource, string> = {
  systemProbe: "系统探针",
  runtimeCatalog: "运行目录",
  environmentDiagnostics: "环境诊断",
  operationalHistory: "脱敏运维历史",
  operationalHealth: "短时健康观测",
  modelCatalog: "公开模型目录",
  modelRepository: "模型仓库快照",
  externalIntegrationStatus: "软件接入状态",
  actionPlan: "Rust 一次性计划",
  runtimeRecheck: "模型运行态复验",
  runtimeProfileRecheck: "个人运行方案复验",
  modelLibraryRecheck: "模型库复验",
  engineRecheck: "引擎复验",
  integrationRecheck: "软件接入复验",
  managedInstallationRecheck: "私有安装复验",
  repairDiagnosticRecheck: "修复后诊断",
};

const agentTaskGraphNodeStateCopy: Record<AgentTaskGraphNodeCheckpointState, string> = {
  blocked: "等待前置节点",
  ready: "可以继续",
  running: "正在处理",
  awaitingConfirmation: "等待原生确认",
  succeeded: "已由现实证据完成",
  failed: "失败",
  compensating: "补偿中",
  compensated: "已补偿",
  cancelled: "已取消",
};

const externalAgentChoiceCopy = {
  openCode: "OpenCode",
  piCodingAgent: "Pi Coding Agent",
  openClaw: "OpenClaw",
  hermesAgent: "Hermes Agent",
} as const;

const clarificationQuestionCopy: Record<AgentClarificationKind, string> = {
  externalAgentTarget: "本次要处理哪个外部 Agent？",
  managedOwnership: "你希望处理 HAL100 私有运行时，还是只断开接入？",
  singleMutationTarget: "本次只继续处理哪个外部 Agent？",
};

function clarificationOptionCopy(option: AgentClarificationOption): string {
  if (option.choice === "selectExternalAgent" && option.externalAgent) {
    return externalAgentChoiceCopy[option.externalAgent];
  }
  if (option.choice === "removeManagedRuntime") return "移除 HAL100 私有 Pi 运行时";
  if (option.choice === "disconnectOnly") return "只断开 HAL100 接入";
  return "取消当前任务";
}

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
    id: "activate-runtime-profile",
    category: "models",
    label: "运行已保存方案",
    prompt: "读取个人运行方案，并为我指定的已保存方案生成安全启用计划；不要直接执行。",
  },
  {
    id: "stop-current-model",
    category: "models",
    label: "生成当前模型停止计划",
    prompt: "读取当前活动模型，并生成安全停止计划；保留模型文件、索引和用量记录，不要直接执行。",
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
  activateRuntimeProfile: {
    title: "启用已保存运行方案",
    targetLabel: "目标方案",
    pendingSummary: "Agent 尚未切换模型或运行参数",
  },
  stopModel: {
    title: "停止当前本地模型",
    targetLabel: "当前模型",
    pendingSummary: "Agent 尚未停止当前模型",
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
  const [prompt, setPrompt] = useState("");
  const promptRef = useRef<HTMLTextAreaElement>(null);
  const [result, setResult] = useState<AgentRunResult | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [taskCategory, setTaskCategory] = useState<AgentTaskCategory>("diagnostics");
  const [providerMode, setProviderMode] = useState<"local" | "cloud-single" | "cloud-session">(
    "local",
  );
  const [cloudBackendId, setCloudBackendId] = useState("");
  const [cloudModel, setCloudModel] = useState("");
  const [graphModelId, setGraphModelId] = useState("");
  const [graphExternalAgent, setGraphExternalAgent] =
    useState<AgentExternalAgentChoice>("openCode");
  const [includeManagedPi, setIncludeManagedPi] = useState(false);
  const [cloudRunPreview, setCloudRunPreview] = useState<AgentCloudRunPreview | null>(null);
  const [cloudSessionPreview, setCloudSessionPreview] = useState<AgentCloudSessionPreview | null>(
    null,
  );
  const [runStartedAtMs, setRunStartedAtMs] = useState<number | null>(null);
  const [runElapsedSeconds, setRunElapsedSeconds] = useState(0);
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
  const modelLibrary = useQuery({
    queryKey: ["model-library"],
    queryFn: getModelLibrary,
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
  const acceptRunResult = (nextResult: AgentRunResult) => {
    setResult(nextResult);
    setCloudRunPreview(null);
    setActionMessage(null);
    queryClient.invalidateQueries({ queryKey: ["agent-status"] });
    queryClient.invalidateQueries({ queryKey: ["agent-cloud-session"] });
    queryClient.invalidateQueries({ queryKey: ["usage-dashboard"] });
    queryClient.invalidateQueries({ queryKey: ["audit-log"] });
  };
  const runMutation = useMutation({
    mutationFn: runAgentPrompt,
    onMutate: () => {
      setRunStartedAtMs(Date.now());
      setRunElapsedSeconds(0);
    },
    onSuccess: acceptRunResult,
    onError: () => {
      queryClient.invalidateQueries({ queryKey: ["agent-status"] });
      queryClient.invalidateQueries({ queryKey: ["agent-cloud-session"] });
      queryClient.invalidateQueries({ queryKey: ["audit-log"] });
    },
    onSettled: () => setRunStartedAtMs(null),
  });
  const clarificationMutation = useMutation({
    mutationFn: continueAgentClarification,
    onSuccess: acceptRunResult,
    onError: () => {
      queryClient.invalidateQueries({ queryKey: ["agent-status"] });
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
    onError: (_error, planId) => {
      setResult((previous) =>
        previous
          ? {
              ...previous,
              actionPlans: previous.actionPlans.filter((plan) => plan.planId !== planId),
            }
          : previous,
      );
      queryClient.invalidateQueries({ queryKey: ["agent-status"] });
    },
  });
  const startGraphMutation = useMutation({
    mutationFn: startAgentTaskGraph,
    onSuccess: () => {
      setResult(null);
      setActionMessage(null);
      queryClient.invalidateQueries({ queryKey: ["agent-status"] });
      queryClient.invalidateQueries({ queryKey: ["audit-log"] });
    },
  });
  const restoreGraphMutation = useMutation({
    mutationFn: restoreAgentTaskGraph,
    onSuccess: () => {
      setResult(null);
      setActionMessage(null);
      queryClient.invalidateQueries({ queryKey: ["agent-status"] });
      queryClient.invalidateQueries({ queryKey: ["audit-log"] });
    },
  });
  const runGraphNodeMutation = useMutation({
    mutationFn: runNextAgentTaskGraphNode,
    onSuccess: acceptRunResult,
    onError: () => {
      queryClient.invalidateQueries({ queryKey: ["agent-status"] });
      queryClient.invalidateQueries({ queryKey: ["audit-log"] });
    },
  });
  const runGraphCompensationMutation = useMutation({
    mutationFn: runNextAgentTaskGraphCompensation,
    onSuccess: acceptRunResult,
    onError: () => {
      queryClient.invalidateQueries({ queryKey: ["agent-status"] });
      queryClient.invalidateQueries({ queryKey: ["audit-log"] });
    },
  });
  const cancelGraphMutation = useMutation({
    mutationFn: cancelAgentTaskGraph,
    onSuccess: (nextStatus) => {
      setResult(null);
      setActionMessage(null);
      queryClient.setQueryData(["agent-status"], nextStatus);
      queryClient.invalidateQueries({ queryKey: ["audit-log"] });
    },
  });

  const refetchAgentStatus = status.refetch;
  useEffect(() => {
    if (!runMutation.isPending || runStartedAtMs === null) return;

    const refreshRunStatus = () => {
      setRunElapsedSeconds(Math.max(0, Math.floor((Date.now() - runStartedAtMs) / 1000)));
      void refetchAgentStatus();
    };
    refreshRunStatus();
    const refreshInterval = window.setInterval(refreshRunStatus, 1_000);
    return () => window.clearInterval(refreshInterval);
  }, [refetchAgentStatus, runMutation.isPending, runStartedAtMs]);

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
    clarificationMutation.isPending ||
    previewRunMutation.isPending ||
    previewSessionMutation.isPending ||
    startSessionMutation.isPending ||
    stopSessionMutation.isPending ||
    startGraphMutation.isPending ||
    restoreGraphMutation.isPending ||
    runGraphNodeMutation.isPending ||
    runGraphCompensationMutation.isPending ||
    cancelGraphMutation.isPending;
  const agentRunPending =
    runMutation.isPending ||
    runGraphNodeMutation.isPending ||
    runGraphCompensationMutation.isPending;
  const kernelState =
    agentRunPending && data.kernelState === "stopped" ? "starting" : data.kernelState;
  const modelState =
    agentRunPending && providerMode === "local" && data.modelRuntimeState === "stopped"
      ? "starting"
      : data.modelRuntimeState;
  const runProgress = getAgentRunProgress({
    activeRunId: data.activeRunId,
    elapsedSeconds: runElapsedSeconds,
    kernelState,
    modelRuntimeState: modelState,
    providerMode,
  });
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
    !clarificationMutation.isPending &&
    !previewRunMutation.isPending &&
    !previewSessionMutation.isPending &&
    !startSessionMutation.isPending &&
    !stopSessionMutation.isPending &&
    !stopMutation.isPending &&
    !actionMutation.isPending &&
    data.taskGraphCheckpoint?.state !== "active" &&
    data.taskGraphCheckpoint?.state !== "compensating";
  const operationError =
    runMutation.error ??
    clarificationMutation.error ??
    previewRunMutation.error ??
    previewSessionMutation.error ??
    startSessionMutation.error ??
    stopSessionMutation.error ??
    stopMutation.error ??
    cancelMutation.error ??
    actionMutation.error ??
    startGraphMutation.error ??
    restoreGraphMutation.error ??
    runGraphNodeMutation.error ??
    runGraphCompensationMutation.error ??
    cancelGraphMutation.error ??
    modelLibrary.error ??
    diagnostics.error;
  const elapsedSeconds = result
    ? Math.max(0, (result.completedAtMs - result.startedAtMs) / 1000).toFixed(1)
    : null;
  const clarification = result?.clarification ?? null;
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
  const agentStatusLabel =
    runMutation.isPending ||
    runGraphNodeMutation.isPending ||
    runGraphCompensationMutation.isPending
      ? "正在运行"
      : agentReady
        ? "可以开始任务"
        : "需要准备";
  const graph = data.taskGraphCheckpoint;
  const recoverableGraph = data.recoverableTaskGraphCheckpoint;
  const readyModels = modelLibrary.data?.models.filter((model) => model.state === "ready") ?? [];
  const effectiveGraphModelId = graphModelId || readyModels[0]?.id || "";
  const graphAwaitingConfirmation = graph?.nodes.some(
    (node) => node.state === "awaitingConfirmation",
  );
  const graphHasRunningNode = graph?.nodes.some((node) => node.state === "running");
  const graphHasCompensatingNode = graph?.nodes.some((node) => node.state === "compensating");
  const graphHasCompensationCandidate = graph?.nodes.some(
    (node) => node.state === "succeeded" && node.changedOwnedState,
  );
  const graphCompensationAwaitingConfirmation = Boolean(
    graph?.state === "compensating" && data.taskCheckpoint?.phase === "awaitingConfirmation",
  );
  const graphRequiresAttention = Boolean(
    graph?.state === "active" ||
      graph?.state === "compensating" ||
      (graph?.state === "failed" && graphHasCompensationCandidate) ||
      (recoverableGraph && !graph),
  );
  const canContinueGraph = Boolean(
    runtime &&
      graph?.state === "active" &&
      !graphAwaitingConfirmation &&
      (graph.readyNodeCount > 0 || graphHasRunningNode) &&
      data.modelPrepared &&
      !agentTransitionPending &&
      !actionMutation.isPending,
  );
  const canContinueGraphCompensation = Boolean(
    runtime &&
      graph?.state === "compensating" &&
      !graphCompensationAwaitingConfirmation &&
      graphHasCompensatingNode &&
      data.modelPrepared &&
      !agentTransitionPending &&
      !actionMutation.isPending,
  );

  const updatePrompt = (nextPrompt: string) => {
    setPrompt(nextPrompt);
    setCloudRunPreview(null);
  };

  const chooseTask = (task: AgentTaskTemplate) => {
    updatePrompt(task.prompt);
    window.requestAnimationFrame(() => promptRef.current?.focus());
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
      <PageHeader
        action={
          <div className="agent-header-actions">
            <span className={`agent-ready-badge ${agentReady ? "ready" : "attention"}`}>
              <i aria-hidden="true" />
              {agentStatusLabel}
            </span>
            <button
              className="secondary-button"
              disabled={diagnostics.isFetching || runMutation.isPending || actionMutation.isPending}
              onClick={() => void diagnostics.refetch()}
              type="button"
            >
              <RefreshCw className={diagnostics.isFetching ? "spinning" : ""} size={13} />
              {diagnostics.isFetching ? "检查中…" : "检查环境"}
            </button>
          </div>
        }
        description="描述目标，Agent 会先分析并给出可检查的结果或操作计划。"
        title="HAL100 Agent"
      />

      <details className="agent-boundary inline-disclosure">
        <summary>
          <ShieldCheck size={16} />
          <strong>所有系统更改都由你确认</strong>
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

      <details className="agent-technical-details inline-disclosure">
        <summary>
          <span>运行与安全详情</span>
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
          <div>
            <dt>上下文容量</dt>
            <dd>
              {data.contextWindowTokens.toLocaleString()} Token · {data.capacityTier} · Rust
              设备策略
            </dd>
          </div>
          {data.taskCheckpoint && (
            <>
              <div>
                <dt>任务检查点</dt>
                <dd>
                  {data.taskCheckpoint.taskKind} · {agentTaskPhaseCopy[data.taskCheckpoint.phase]} ·
                  序列
                  {data.taskCheckpoint.checkpointSequence}
                </dd>
              </div>
              <div>
                <dt>成功复验</dt>
                <dd>
                  {agentTaskVerificationCopy[data.taskCheckpoint.verificationState]}
                  {data.taskCheckpoint.evidenceSource
                    ? ` · ${agentTaskEvidenceSourceCopy[data.taskCheckpoint.evidenceSource]}`
                    : ""}
                  {data.taskCheckpoint.evidenceObservationCount > 0
                    ? ` · ${data.taskCheckpoint.evidenceObservationCount} 项有界观察`
                    : ""}
                  {data.taskCheckpoint.maxReplanAttempts > 0
                    ? ` · 重规划 ${data.taskCheckpoint.replanAttemptCount}/${data.taskCheckpoint.maxReplanAttempts}`
                    : ""}
                  {data.taskCheckpoint.maxClarificationAttempts > 0
                    ? ` · 澄清 ${data.taskCheckpoint.clarificationAttemptCount}/${data.taskCheckpoint.maxClarificationAttempts}`
                    : ""}
                </dd>
              </div>
            </>
          )}
        </dl>
        <div className="agent-runtime-actions">
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
            {stopMutation.isPending ? "正在释放…" : "释放 Agent 模型"}
          </button>
        </div>
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

      <details
        className={`agent-task-graph${graphRequiresAttention ? " is-active" : ""}`}
        aria-label="多步骤任务"
        open={graphRequiresAttention || undefined}
      >
        <summary className="agent-section-heading">
          <div>
            <p className="eyebrow">多步骤任务</p>
            <h2>分步准备模型与常用软件</h2>
          </div>
          <span className="agent-composer-context">
            <ShieldCheck size={12} />
            逐项确认
            <ChevronRight size={12} />
          </span>
        </summary>
        <p className="agent-task-graph-note">
          每次只运行一个节点；安装、启动和配置仍分别生成一次性计划，并逐项弹出原生确认。
        </p>
        {graph?.state === "active" || graph?.state === "compensating" ? (
          <>
            <ol className="agent-task-graph-nodes">
              {graph.nodes.map((node) => (
                <li className={node.state} key={node.nodeIndex}>
                  <span>{node.nodeIndex + 1}</span>
                  <div>
                    <strong>{node.taskKind}</strong>
                    <small>
                      {agentTaskGraphNodeStateCopy[node.state]}
                      {node.evidenceSource
                        ? ` · ${agentTaskEvidenceSourceCopy[node.evidenceSource]}`
                        : ""}
                      {node.changedOwnedState ? " · 已改变 HAL100 所有状态" : ""}
                    </small>
                  </div>
                </li>
              ))}
            </ol>
            {graphAwaitingConfirmation && (
              <p className="inline-notice">
                当前步骤已停在确认前。请检查下方一次性计划；确认执行并通过系统复验后，下一步才会解锁。
              </p>
            )}
            {graphCompensationAwaitingConfirmation && (
              <p className="inline-notice">
                补偿计划已停在确认前。补偿不会自动执行；请核对下方逆操作并再次完成原生确认。
              </p>
            )}
            {graph.state === "active" ? (
              <div className="agent-task-graph-actions">
                <button
                  className="primary-button"
                  disabled={!canContinueGraph}
                  onClick={() => runGraphNodeMutation.mutate()}
                  type="button"
                >
                  <Play size={12} />
                  {runGraphNodeMutation.isPending
                    ? "正在处理节点…"
                    : graphHasRunningNode
                      ? "重新规划当前节点"
                      : "继续下一节点"}
                </button>
                {runGraphNodeMutation.isPending ? (
                  <button
                    className="secondary-button"
                    disabled={!runtime || cancelMutation.isPending || data.cancellationRequested}
                    onClick={() => cancelMutation.mutate()}
                    type="button"
                  >
                    <Square size={11} />
                    取消当前节点
                  </button>
                ) : (
                  <button
                    className="secondary-button"
                    disabled={!runtime || agentTransitionPending || actionMutation.isPending}
                    onClick={() => cancelGraphMutation.mutate()}
                    type="button"
                  >
                    <Square size={11} />
                    取消复合任务
                  </button>
                )}
              </div>
            ) : (
              <div className="agent-task-graph-actions">
                <button
                  className="primary-button"
                  disabled={!canContinueGraphCompensation}
                  onClick={() => runGraphCompensationMutation.mutate()}
                  type="button"
                >
                  <Play size={12} />
                  {runGraphCompensationMutation.isPending
                    ? "正在生成补偿计划…"
                    : "重新规划当前补偿"}
                </button>
                {runGraphCompensationMutation.isPending ? (
                  <button
                    className="secondary-button"
                    disabled={!runtime || cancelMutation.isPending || data.cancellationRequested}
                    onClick={() => cancelMutation.mutate()}
                    type="button"
                  >
                    <Square size={11} />
                    取消当前补偿
                  </button>
                ) : (
                  <button
                    className="secondary-button"
                    disabled={!runtime || agentTransitionPending || actionMutation.isPending}
                    onClick={() => cancelGraphMutation.mutate()}
                    type="button"
                  >
                    <Square size={11} />
                    停止补偿
                  </button>
                )}
              </div>
            )}
          </>
        ) : (
          <>
            {graph && (
              <>
                <p className={`agent-task-graph-terminal ${graph.state}`}>
                  上一复合任务：{graph.state === "succeeded" ? "全部完成" : graph.state}
                </p>
                {(graph.state === "failed" || graph.state === "cancelled") &&
                  graphHasCompensationCandidate && (
                    <button
                      className="secondary-button"
                      disabled={!runtime || agentTransitionPending || actionMutation.isPending}
                      onClick={() => runGraphCompensationMutation.mutate()}
                      type="button"
                    >
                      <ShieldCheck size={12} />
                      {runGraphCompensationMutation.isPending
                        ? "正在准备补偿…"
                        : "显式准备下一项安全补偿"}
                    </button>
                  )}
              </>
            )}
            {recoverableGraph && !graph && (
              <div className="inline-notice">
                <strong>检测到重启前的脱敏任务图</strong>
                <span>
                  仅保留{recoverableGraph.nodes.length}个节点的语义形状；请在下方重新选择精确模型和
                  软件。恢复后所有节点都会重新读取现实状态，旧成功、计划与确认均不会恢复。
                </span>
                <button
                  className="secondary-button"
                  disabled={
                    !runtime ||
                    !data.modelPrepared ||
                    !effectiveGraphModelId ||
                    agentTransitionPending ||
                    actionMutation.isPending
                  }
                  onClick={() =>
                    restoreGraphMutation.mutate({
                      kind:
                        recoverableGraph.nodes.length === 4
                          ? "prepareManagedPi"
                          : "prepareExternalAgent",
                      modelId: effectiveGraphModelId,
                      externalAgent:
                        recoverableGraph.nodes.length === 4 ? "piCodingAgent" : graphExternalAgent,
                    })
                  }
                  type="button"
                >
                  <RefreshCw size={12} />
                  {restoreGraphMutation.isPending ? "正在重绑定…" : "按当前选择恢复并全量复验"}
                </button>
              </div>
            )}
            <div className="agent-task-graph-form">
              <label>
                <span>要启动的本地模型</span>
                <select
                  disabled={agentTransitionPending || readyModels.length === 0}
                  onChange={(event) => setGraphModelId(event.target.value)}
                  value={effectiveGraphModelId}
                >
                  {readyModels.length === 0 ? (
                    <option value="">模型库中没有已就绪模型</option>
                  ) : (
                    readyModels.map((model) => (
                      <option key={model.id} value={model.id}>
                        {model.displayName}
                      </option>
                    ))
                  )}
                </select>
              </label>
              <label>
                <span>要接入的软件</span>
                <select
                  disabled={agentTransitionPending}
                  onChange={(event) => {
                    const next = event.target.value as AgentExternalAgentChoice;
                    setGraphExternalAgent(next);
                    if (next !== "piCodingAgent") setIncludeManagedPi(false);
                  }}
                  value={graphExternalAgent}
                >
                  <option value="openCode">OpenCode</option>
                  <option value="piCodingAgent">Pi Coding Agent</option>
                  <option value="openClaw">OpenClaw</option>
                </select>
              </label>
              <label className="agent-task-graph-check">
                <input
                  checked={includeManagedPi}
                  disabled={graphExternalAgent !== "piCodingAgent" || agentTransitionPending}
                  onChange={(event) => setIncludeManagedPi(event.target.checked)}
                  type="checkbox"
                />
                <span>同时准备 HAL100 私有 Pi 安装</span>
              </label>
              <button
                className="primary-button"
                disabled={
                  !runtime ||
                  !data.modelPrepared ||
                  !effectiveGraphModelId ||
                  agentTransitionPending ||
                  actionMutation.isPending
                }
                onClick={() =>
                  startGraphMutation.mutate({
                    kind: includeManagedPi ? "prepareManagedPi" : "prepareExternalAgent",
                    modelId: effectiveGraphModelId,
                    externalAgent: graphExternalAgent,
                  })
                }
                type="button"
              >
                <Play size={12} />
                {startGraphMutation.isPending ? "正在准备…" : "开始分步准备"}
              </button>
            </div>
          </>
        )}
      </details>

      <section className="agent-workspace">
        <form className="agent-composer" onSubmit={submitPrompt}>
          <div className="agent-section-heading">
            <div>
              <p className="eyebrow">新任务</p>
              <h2>你想让 Agent 完成什么？</h2>
            </div>
            <span className="agent-composer-context">
              <ShieldCheck size={12} />
              不保留历史
            </span>
          </div>
          <div className="agent-task-field">
            <label htmlFor="agent-prompt">用自然语言描述目标</label>
            <textarea
              disabled={
                (providerMode === "local" && !data.modelPrepared) ||
                (sessionActive && !sessionData.available) ||
                agentTransitionPending
              }
              id="agent-prompt"
              maxLength={4096}
              onChange={(event) => updatePrompt(event.target.value)}
              placeholder="例如：检查这台 Mac 的本地 AI 环境，并告诉我最需要先处理的问题"
              ref={promptRef}
              value={prompt}
            />
            <div className="agent-task-field-meta">
              <span>
                <ShieldCheck size={12} />
                {providerMode === "local"
                  ? "默认在本机处理，内容不会离开这台 Mac"
                  : "发送到云端前会先展示范围并要求确认"}
              </span>
              <span>{prompt.length} / 4096</span>
            </div>
          </div>
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
                        : "开始分析"}
              </button>
            </div>
          </div>
          <details className="agent-provider-disclosure">
            <summary>
              <div>
                <strong>处理位置</strong>
                <span>
                  {providerMode === "local"
                    ? "内容留在本机"
                    : providerMode === "cloud-single"
                      ? "仅发送当前任务"
                      : "当前会话使用云端"}
                </span>
              </div>
              <span>
                {providerMode === "local" ? "本地" : "云端"}
                <ChevronRight size={13} />
              </span>
            </summary>
            <div className="agent-provider-disclosure-body">
              <fieldset className="agent-provider-picker">
                <legend>选择处理位置</legend>
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
                      <label htmlFor="agent-cloud-backend">已连接服务</label>
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
                        {selectedCloudBackend?.apiRoot} · API Key 只由 Gateway 从 macOS Keychain
                        读取
                      </small>
                    </>
                  ) : (
                    <div className="agent-cloud-empty">
                      <span>暂无可用且已配置凭据的 OpenAI/Anthropic 兼容后端。</span>
                      <NavLink to="/workspace/services">前往连接服务</NavLink>
                    </div>
                  )}
                </section>
              )}
            </div>
          </details>
          <section className="agent-templates" aria-label="快捷任务模板">
            <div className="agent-templates-heading">
              <strong>常用任务</strong>
              <span>选择后会填入上方，确认内容再开始</span>
            </div>
            <div className="agent-prompt-shortcuts agent-context-recommendations">
              {recommendedTasks.map((task) => (
                <button
                  className="agent-prompt-chip"
                  disabled={agentTransitionPending}
                  key={task.id}
                  onClick={() => chooseTask(task)}
                  type="button"
                >
                  <span>{task.label}</span>
                  <ChevronRight size={12} />
                </button>
              ))}
            </div>
            <details className="agent-task-library">
              <summary>
                浏览全部任务
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
                    onClick={() => chooseTask(task)}
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
              浏览器预览不会运行 Agent、启动下载或执行任何写操作；请在桌面开发版中运行。
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
        </form>

        <article
          className={`agent-result${!runMutation.isPending && !result ? " is-empty" : ""}`}
          aria-live="polite"
        >
          <div className="agent-section-heading">
            <div>
              <p className="eyebrow">结果</p>
              <h2>结果与待确认操作</h2>
            </div>
            {elapsedSeconds && <span>{elapsedSeconds} 秒</span>}
          </div>
          <ol className="agent-result-flow" aria-label="任务处理流程">
            <li className={runMutation.isPending || result ? "active" : ""}>
              <span>1</span>
              分析目标
            </li>
            <li className={result ? "active" : ""}>
              <span>2</span>
              展示结果或计划
            </li>
            <li className={result?.actionPlans.length ? "active" : ""}>
              <span>3</span>
              确认后执行
            </li>
          </ol>
          {runMutation.isPending && (
            <div className="agent-running-stage" role="status">
              <RefreshCw className="spinning" size={14} />
              <span>正在处理当前任务 · {runElapsedSeconds} 秒，可随时取消</span>
            </div>
          )}
          {runMutation.isPending ? (
            <div className={`agent-running-detail${runProgress.slow ? " is-slow" : ""}`}>
              <strong>{runProgress.title}</strong>
              <span>{runProgress.description}</span>
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
              {clarification && (
                <section className="agent-clarification" aria-label="继续当前澄清任务">
                  <div>
                    <ShieldCheck size={15} />
                    <div>
                      <strong>{clarificationQuestionCopy[clarification.kind]}</strong>
                      <span>固定选项只补全当前任务，不恢复旧提示词，也不等同于执行确认。</span>
                    </div>
                  </div>
                  <div className="agent-clarification-options">
                    {clarification.options.map((option) => (
                      <button
                        className={
                          option.choice === "cancel" ? "secondary-button" : "primary-button"
                        }
                        disabled={
                          clarificationMutation.isPending || Date.now() > clarification.expiresAtMs
                        }
                        key={`${option.choice}-${option.externalAgent ?? "none"}`}
                        onClick={() =>
                          clarificationMutation.mutate({
                            kind: clarification.kind,
                            choice: option.choice,
                            externalAgent: option.externalAgent,
                            cloudTarget:
                              providerMode === "cloud-single"
                                ? {
                                    backendId: effectiveCloudBackendId,
                                    model: cloudModel.trim(),
                                  }
                                : null,
                          })
                        }
                        type="button"
                      >
                        {clarificationMutation.isPending
                          ? "正在继续…"
                          : clarificationOptionCopy(option)}
                      </button>
                    ))}
                  </div>
                  <small>
                    澄清次数 {clarification.attemptCount}/{clarification.maxAttempts} ·
                    仅当前进程内有效
                  </small>
                </section>
              )}
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
                    <dt>上下文窗口</dt>
                    <dd>{result.efficiency.contextWindowTokens.toLocaleString()} Token</dd>
                  </div>
                  <div>
                    <dt>模型回合</dt>
                    <dd>
                      {result.efficiency.totalModelTurnCount}（意图
                      {result.efficiency.intentModelTurnCount} / 执行
                      {result.efficiency.executionModelTurnCount}）
                    </dd>
                  </div>
                  <div>
                    <dt>峰值输入</dt>
                    <dd>
                      {result.efficiency.providerUsageAvailable
                        ? `${result.efficiency.peakReportedInputTokens.toLocaleString()} Token`
                        : `约 ${result.efficiency.peakEstimatedInputTokens.toLocaleString()} Token`}
                    </dd>
                  </div>
                  <div>
                    <dt>重复工具结果</dt>
                    <dd>
                      约 {result.efficiency.repeatedToolResultTokenEstimate.toLocaleString()} Token
                    </dd>
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
              <span className="agent-empty-icon">
                <Bot size={20} />
              </span>
              <strong>等待你的任务</strong>
              <span>提交后，分析结果和需要确认的操作会按顺序显示在这里。</span>
            </div>
          )}
        </article>
      </section>
    </div>
  );
}
