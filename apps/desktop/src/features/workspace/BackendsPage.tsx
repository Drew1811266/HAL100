import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  Cable,
  ChevronRight,
  Download,
  FlaskConical,
  Play,
  RefreshCw,
  Search,
  ServerCog,
  ShieldCheck,
  Square,
  Trash2,
  X,
} from "lucide-react";
import { type FormEvent, useState } from "react";
import { NavLink } from "react-router-dom";
import { Drawer } from "../../components/ui/Drawer";
import { PageHeader } from "../../components/ui/PageHeader";
import {
  activateExternalBackend,
  applyLlamaCppInstall,
  applyLlamaCppRemove,
  type BackendAuthMethod,
  type BackendDraft,
  type BackendKind,
  type BackendProbeStatus,
  deleteExternalBackend,
  deleteModelRoute,
  discoverLocalBackends,
  type EngineInstallPlan,
  type EngineRemovePlan,
  forceActivateExternalBackend,
  forceStartLlamaCppModel,
  forceStopLlamaCpp,
  getBackendCatalog,
  getLlamaCppStatus,
  getModelLibrary,
  type InferenceEngineKind,
  isTauriRuntime,
  planLlamaCppInstall,
  planLlamaCppRemove,
  probeExternalBackend,
  saveExternalBackend,
  saveModelRoute,
  startLlamaCppModel,
  stopLlamaCpp,
  testActiveModel,
} from "../../lib/desktop-api";

interface SectionTab {
  label: string;
  path: string;
}

const workspaceTabs: SectionTab[] = [
  { label: "模型", path: "/workspace/models" },
  { label: "运行", path: "/workspace/runtime" },
  { label: "推理服务", path: "/workspace/services" },
];

function SectionTabs({ label, tabs }: { label: string; tabs: SectionTab[] }) {
  return (
    <nav aria-label={label} className="section-tabs">
      {tabs.map((tab) => (
        <NavLink
          className={({ isActive }) => (isActive ? "active" : undefined)}
          key={tab.path}
          to={tab.path}
        >
          {tab.label}
        </NavLink>
      ))}
    </nav>
  );
}

function formatBytes(bytes: number): string {
  if (bytes >= 1024 ** 3) {
    return `${(bytes / 1024 ** 3).toFixed(bytes >= 10 * 1024 ** 3 ? 0 : 1)} GB`;
  }
  if (bytes >= 1024 ** 2) {
    return `${(bytes / 1024 ** 2).toFixed(bytes >= 10 * 1024 ** 2 ? 0 : 1)} MB`;
  }
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

type EnginePlan =
  | { kind: "install"; plan: EngineInstallPlan }
  | { kind: "remove"; plan: EngineRemovePlan };

function EngineConfirmationDialog({
  operation,
  applying,
  error,
  onCancel,
  onApply,
}: {
  operation: EnginePlan;
  applying: boolean;
  error: string | null;
  onCancel: () => void;
  onApply: () => void;
}) {
  const runtime = isTauriRuntime();
  const installing = operation.kind === "install";
  const plan = operation.plan;
  return (
    <div className="dialog-backdrop" role="presentation">
      <section
        aria-labelledby="engine-confirmation-title"
        aria-modal="true"
        className="dialog"
        role="dialog"
      >
        <div className="dialog-heading">
          <div>
            <p className="eyebrow">需要确认</p>
            <h2 id="engine-confirmation-title">
              {installing ? "安装 llama.cpp" : "卸载 llama.cpp"}
            </h2>
          </div>
          <button
            aria-label="关闭"
            className="icon-button"
            disabled={applying}
            onClick={onCancel}
            type="button"
          >
            <X size={17} />
          </button>
        </div>
        <p className="dialog-intro">
          {installing
            ? "HAL100 将获取固定且可校验的 Apple Silicon 官方构建。"
            : "卸载会先停止 HAL100 托管的推理服务，再删除引擎文件。"}
        </p>
        <div className="engine-plan-preview">
          <ShieldCheck className="engine-plan-icon" size={20} />
          <div>
            <strong>
              {plan.engine} {plan.version}
            </strong>
            <span>
              {installing
                ? `${operation.plan.publisher} · ${formatBytes(operation.plan.archiveSizeBytes)}`
                : operation.plan.installPath}
            </span>
          </div>
        </div>
        <div className="safety-summary">
          {installing ? <Download size={17} /> : <Trash2 size={17} />}
          <p>{plan.actionSummary}。</p>
        </div>
        {!runtime && <p className="inline-notice">浏览器预览模式只能查看计划，不能执行操作。</p>}
        {error && <p className="inline-error">{error}</p>}
        <div className="dialog-actions">
          <button className="secondary-button" disabled={applying} onClick={onCancel} type="button">
            取消
          </button>
          <button
            className={installing ? "primary-button" : "danger-button"}
            disabled={applying || !runtime}
            onClick={onApply}
            type="button"
          >
            {applying ? "正在执行…" : installing ? "确认安装" : "确认卸载"}
          </button>
        </div>
      </section>
    </div>
  );
}

const backendKindLabels: Record<BackendKind, string> = {
  managedLlamaCpp: "HAL100 托管 llama.cpp",
  externalOpenAi: "OpenAI 兼容",
  externalAnthropic: "Anthropic 兼容",
  externalOllama: "Ollama",
  externalVllm: "vLLM",
  externalLlamaCpp: "llama.cpp Server",
};

const backendAuthLabels: Record<BackendAuthMethod, string> = {
  none: "无需认证",
  bearer: "Bearer API Key",
  anthropicApiKey: "Anthropic x-api-key",
};

const explicitEngineBindings: Array<{
  key: string;
  engine: InferenceEngineKind;
  label: string;
  adapterVariant: string;
}> = [
  {
    key: "mlxLm",
    engine: "mlxLm",
    label: "MLX-LM 官方 HTTP Server",
    adapterVariant: "official-http-server",
  },
  {
    key: "vllm",
    engine: "vllm",
    label: "vLLM 官方 OpenAI Server",
    adapterVariant: "official-openai-server",
  },
  {
    key: "mlcLlm",
    engine: "mlcLlm",
    label: "MLC LLM 官方 OpenAI Server",
    adapterVariant: "official-openai-server",
  },
  {
    key: "openVino:cpu",
    engine: "openVino",
    label: "OpenVINO Model Server · CPU",
    adapterVariant: "ovms-openai-cpu",
  },
  {
    key: "openVino:intelGpu",
    engine: "openVino",
    label: "OpenVINO Model Server · Intel GPU",
    adapterVariant: "ovms-openai-intel-gpu",
  },
  {
    key: "openVino:intelNpu",
    engine: "openVino",
    label: "OpenVINO Model Server · Intel NPU",
    adapterVariant: "ovms-openai-intel-npu",
  },
  {
    key: "sglang",
    engine: "sglang",
    label: "SGLang 官方 OpenAI Server",
    adapterVariant: "official-openai-server",
  },
  {
    key: "lmDeploy",
    engine: "lmDeploy",
    label: "LMDeploy 官方 OpenAI Server",
    adapterVariant: "official-openai-server",
  },
  {
    key: "tensorRtLlm",
    engine: "tensorRtLlm",
    label: "TensorRT-LLM trtllm-serve",
    adapterVariant: "trtllm-serve-openai-server",
  },
];

function backendIdentityLabel(backend: {
  kind: BackendKind;
  engine: InferenceEngineKind | null;
  adapterVariant: string | null;
}) {
  if (backend.kind === "externalOpenAi" && backend.engine) {
    return (
      explicitEngineBindings.find(
        (binding) =>
          binding.engine === backend.engine && binding.adapterVariant === backend.adapterVariant,
      )?.label ?? backendKindLabels[backend.kind]
    );
  }
  return backendKindLabels[backend.kind];
}

const backendProbeLabels: Record<BackendProbeStatus, string> = {
  healthy: "连接正常",
  authenticationFailed: "认证失败",
  upstreamError: "后端返回错误",
  invalidResponse: "响应格式无效",
  unreachable: "无法连接",
};

const emptyBackendDraft: BackendDraft = {
  id: null,
  displayName: "",
  kind: "externalOpenAi",
  engine: null,
  adapterVariant: null,
  apiRoot: "http://127.0.0.1:8000/v1/",
  authMethod: "none",
  apiKey: null,
};

export function BackendsPage({
  initialTestOpen = false,
  view,
}: {
  initialTestOpen?: boolean;
  view: "runtime" | "services";
}) {
  const queryClient = useQueryClient();
  const status = useQuery({
    queryKey: ["llama-cpp-status"],
    queryFn: getLlamaCppStatus,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  const library = useQuery({
    queryKey: ["model-library"],
    queryFn: getModelLibrary,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  const backendCatalog = useQuery({
    queryKey: ["backend-catalog"],
    queryFn: getBackendCatalog,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  const [enginePlan, setEnginePlan] = useState<EnginePlan | null>(null);
  const [modelTestOpen, setModelTestOpen] = useState(initialTestOpen);
  const [selectedModelId, setSelectedModelId] = useState("");
  const [serviceSetupOpen, setServiceSetupOpen] = useState(false);
  const [editingBackend, setEditingBackend] = useState<BackendDraft | null>(null);
  const [routeAlias, setRouteAlias] = useState("");
  const [routeBackendId, setRouteBackendId] = useState("");
  const [routeResolvedModel, setRouteResolvedModel] = useState("");
  const installPlanMutation = useMutation({
    mutationFn: planLlamaCppInstall,
    onSuccess: (plan) => setEnginePlan({ kind: "install", plan }),
  });
  const removePlanMutation = useMutation({
    mutationFn: planLlamaCppRemove,
    onSuccess: (plan) => setEnginePlan({ kind: "remove", plan }),
  });
  const applyPlanMutation = useMutation({
    mutationFn: (operation: EnginePlan) =>
      operation.kind === "install"
        ? applyLlamaCppInstall(operation.plan.planId)
        : applyLlamaCppRemove(operation.plan.planId),
    onSuccess: (nextStatus) => {
      setEnginePlan(null);
      queryClient.setQueryData(["llama-cpp-status"], nextStatus);
      queryClient.invalidateQueries({ queryKey: ["backend-catalog"] });
    },
  });
  const startMutation = useMutation({
    mutationFn: startLlamaCppModel,
    onSuccess: (nextStatus) => {
      queryClient.setQueryData(["llama-cpp-status"], nextStatus);
      queryClient.invalidateQueries({ queryKey: ["backend-catalog"] });
    },
  });
  const stopMutation = useMutation({
    mutationFn: stopLlamaCpp,
    onSuccess: (nextStatus) => {
      queryClient.setQueryData(["llama-cpp-status"], nextStatus);
      queryClient.invalidateQueries({ queryKey: ["backend-catalog"] });
    },
  });
  const forceStartMutation = useMutation({
    mutationFn: forceStartLlamaCppModel,
    onSuccess: (nextStatus) => {
      queryClient.setQueryData(["llama-cpp-status"], nextStatus);
      queryClient.invalidateQueries({ queryKey: ["backend-catalog"] });
    },
  });
  const forceStopMutation = useMutation({
    mutationFn: forceStopLlamaCpp,
    onSuccess: (nextStatus) => {
      queryClient.setQueryData(["llama-cpp-status"], nextStatus);
      queryClient.invalidateQueries({ queryKey: ["backend-catalog"] });
    },
  });
  const saveBackendMutation = useMutation({
    mutationFn: saveExternalBackend,
    onSuccess: (catalog) => {
      queryClient.setQueryData(["backend-catalog"], catalog);
      queryClient.invalidateQueries({ queryKey: ["agent-cloud-session"] });
      setEditingBackend(null);
    },
  });
  const activateBackendMutation = useMutation({
    mutationFn: activateExternalBackend,
    onSuccess: (catalog) => queryClient.setQueryData(["backend-catalog"], catalog),
  });
  const forceActivateBackendMutation = useMutation({
    mutationFn: forceActivateExternalBackend,
    onSuccess: (catalog) => queryClient.setQueryData(["backend-catalog"], catalog),
  });
  const deleteBackendMutation = useMutation({
    mutationFn: deleteExternalBackend,
    onSuccess: (catalog) => {
      queryClient.setQueryData(["backend-catalog"], catalog);
      queryClient.invalidateQueries({ queryKey: ["agent-cloud-session"] });
    },
  });
  const saveRouteMutation = useMutation({
    mutationFn: saveModelRoute,
    onSuccess: (catalog) => {
      queryClient.setQueryData(["backend-catalog"], catalog);
      setRouteAlias("");
      setRouteResolvedModel("");
    },
  });
  const deleteRouteMutation = useMutation({
    mutationFn: deleteModelRoute,
    onSuccess: (catalog) => queryClient.setQueryData(["backend-catalog"], catalog),
  });
  const discoverBackendsMutation = useMutation({ mutationFn: discoverLocalBackends });
  const probeBackendMutation = useMutation({
    mutationFn: probeExternalBackend,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["backend-catalog"] }),
  });

  if (status.isPending || library.isPending || backendCatalog.isPending) {
    return <div className="state-message">正在读取推理引擎与模型状态…</div>;
  }
  if (status.isError || library.isError || backendCatalog.isError) {
    return (
      <div className="state-message error">
        {errorMessage(status.error ?? library.error ?? backendCatalog.error)}
      </div>
    );
  }

  const data = status.data;
  const readyModels = library.data.models.filter((model) => model.state === "ready");
  const availableExternalBackends = backendCatalog.data.backends.filter(
    (backend) => backend.runtimeAvailable,
  );
  const editingBackendCredentialConfigured = editingBackend?.id
    ? (backendCatalog.data.backends.find((backend) => backend.id === editingBackend.id)
        ?.credentialConfigured ?? false)
    : false;
  const activeModelId = selectedModelId || data.activeModelId || readyModels[0]?.id || "";
  const installCopy = {
    notInstalled: { label: "未安装", tone: "neutral" },
    installed: { label: "校验通过", tone: "ok" },
    verificationFailed: { label: "安装损坏", tone: "warning" },
  } as const;
  const runtimeCopy = {
    stopped: "未运行",
    starting: "正在启动",
    running: "运行中",
    error: "运行异常",
  } as const;
  const operationError =
    installPlanMutation.error ??
    removePlanMutation.error ??
    startMutation.error ??
    stopMutation.error ??
    forceStartMutation.error ??
    forceStopMutation.error ??
    saveBackendMutation.error ??
    activateBackendMutation.error ??
    forceActivateBackendMutation.error ??
    deleteBackendMutation.error ??
    saveRouteMutation.error ??
    deleteRouteMutation.error ??
    discoverBackendsMutation.error ??
    probeBackendMutation.error;

  return (
    <div className="page-content backends-page">
      <PageHeader
        action={
          view === "services" ? (
            <button
              className="primary-button"
              onClick={() => setServiceSetupOpen(true)}
              type="button"
            >
              <Cable size={14} />
              添加推理服务
            </button>
          ) : (
            <button
              className="secondary-button refresh-button"
              disabled={status.isFetching}
              onClick={() => {
                status.refetch();
                backendCatalog.refetch();
              }}
              type="button"
            >
              <RefreshCw className={status.isFetching ? "spinning" : ""} size={14} />
              {status.isFetching ? "检测中…" : "重新检测"}
            </button>
          )
        }
        className="model-page-header"
        description={
          view === "runtime"
            ? "管理 HAL100 托管的本地模型运行时。"
            : "连接本机或远程推理服务；路由和模型别名按需展开。"
        }
        eyebrow={view === "runtime" ? "本地推理" : "推理连接"}
        title={view === "runtime" ? "运行" : "推理服务"}
      />
      <SectionTabs label="模型与运行" tabs={workspaceTabs} />

      {operationError && <p className="inline-error">{errorMessage(operationError)}</p>}

      {view === "runtime" && (
        <div className="backend-primary-grid">
          <section className="engine-card">
            <div className="engine-heading">
              <div className="engine-icon">
                <ServerCog size={21} />
              </div>
              <div>
                <p className="eyebrow">HAL100 托管引擎</p>
                <h2>
                  llama.cpp <span>{data.version}</span>
                </h2>
              </div>
              <span className={`status-pill ${installCopy[data.installState].tone}`}>
                {installCopy[data.installState].label}
              </span>
            </div>
            <div className="runtime-current-state">
              <span>当前状态</span>
              <strong>
                {runtimeCopy[data.runtimeState]}
                {data.activeModelName ? ` · ${data.activeModelName}` : ""}
              </strong>
            </div>
            {data.installState === "notInstalled" && (
              <div className="engine-actions">
                <button
                  className="primary-button"
                  disabled={installPlanMutation.isPending}
                  onClick={() => installPlanMutation.mutate()}
                  type="button"
                >
                  <Download size={14} />
                  {installPlanMutation.isPending ? "正在生成计划…" : "安装 llama.cpp"}
                </button>
              </div>
            )}
            <details className="runtime-technical-details">
              <summary>
                <span>技术详情</span>
                <ChevronRight size={14} />
              </summary>
              <div className="engine-details runtime-technical-grid">
                <div>
                  <span>引擎版本</span>
                  <strong>{data.version}</strong>
                </div>
                <div>
                  <span>本地端口</span>
                  <strong>{data.port ? `127.0.0.1:${data.port}` : "未监听"}</strong>
                </div>
                <div>
                  <span>当前模型</span>
                  <strong>{data.activeModelName ?? "未选择"}</strong>
                </div>
                <div>
                  <span>Gateway</span>
                  <strong>{data.runtimeState === "running" ? "已接管" : "等待引擎"}</strong>
                </div>
              </div>
              {data.installState !== "notInstalled" && (
                <button
                  className="secondary-button compact-button"
                  disabled={removePlanMutation.isPending}
                  onClick={() => removePlanMutation.mutate()}
                  type="button"
                >
                  <Trash2 size={14} />
                  {removePlanMutation.isPending ? "正在检查…" : "卸载引擎"}
                </button>
              )}
            </details>
          </section>

          <section
            className={`runtime-card${
              data.installState !== "installed"
                ? " is-blocked"
                : readyModels.length === 0
                  ? " needs-model"
                  : ""
            }`}
          >
            <div>
              <p className="eyebrow">模型运行时</p>
              <h2>启动或切换模型</h2>
              <p>只列出已通过本地状态检查的 GGUF；切换时会停止当前 llama-server 再加载新模型。</p>
            </div>
            {data.installState !== "installed" && (
              <div className="runtime-prerequisite">
                <strong>先准备推理引擎</strong>
                <span>完成左侧 llama.cpp 安装后，模型选择和启动操作将在这里启用。</span>
              </div>
            )}
            {data.installState === "installed" && readyModels.length === 0 && (
              <div className="runtime-prerequisite">
                <strong>还没有可运行的模型</strong>
                <span>添加或导入一个通过本地检查的 GGUF 模型后即可启动。</span>
                <NavLink className="secondary-button" to="/workspace/models">
                  前往模型库
                </NavLink>
              </div>
            )}
            <label>
              <span>本地模型</span>
              <select
                disabled={readyModels.length === 0}
                onChange={(event) => setSelectedModelId(event.target.value)}
                value={activeModelId}
              >
                {readyModels.length === 0 && <option value="">没有可用模型</option>}
                {readyModels.map((model) => (
                  <option key={model.id} value={model.id}>
                    {model.displayName} · {model.quantization ?? "GGUF"}
                  </option>
                ))}
              </select>
            </label>
            <div className="runtime-actions">
              {data.runtimeState === "running" && activeModelId === data.activeModelId ? (
                <button
                  className="primary-button"
                  onClick={() => setModelTestOpen(true)}
                  type="button"
                >
                  <FlaskConical size={14} />
                  测试当前模型
                </button>
              ) : (
                <button
                  className="primary-button"
                  disabled={
                    data.installState !== "installed" ||
                    !activeModelId ||
                    startMutation.isPending ||
                    stopMutation.isPending ||
                    forceStartMutation.isPending ||
                    forceStopMutation.isPending
                  }
                  onClick={() => startMutation.mutate(activeModelId)}
                  type="button"
                >
                  <Play size={14} />
                  {startMutation.isPending
                    ? "正在等待模型加载…"
                    : data.runtimeState === "running"
                      ? "切换到所选模型"
                      : "启动模型"}
                </button>
              )}
              {data.runtimeState !== "stopped" && (
                <button
                  className="secondary-button"
                  disabled={
                    stopMutation.isPending ||
                    forceStartMutation.isPending ||
                    forceStopMutation.isPending
                  }
                  onClick={() => stopMutation.mutate()}
                  type="button"
                >
                  <Square size={12} />
                  {stopMutation.isPending ? "正在停止…" : "停止"}
                </button>
              )}
            </div>
            {data.runtimeState !== "stopped" && (
              <details className="runtime-danger-zone">
                <summary>
                  <span>
                    <AlertTriangle size={13} />
                    强制操作
                  </span>
                  <ChevronRight size={14} />
                </summary>
                <p>仅在请求无法正常排空时使用；执行前仍会进入原生确认。</p>
                <div>
                  <button
                    className="danger-button compact-button"
                    disabled={
                      data.installState !== "installed" ||
                      !activeModelId ||
                      startMutation.isPending ||
                      stopMutation.isPending ||
                      forceStartMutation.isPending ||
                      forceStopMutation.isPending ||
                      !isTauriRuntime()
                    }
                    onClick={() => forceStartMutation.mutate(activeModelId)}
                    title="取消未完成请求后立即切换"
                    type="button"
                  >
                    强制切换
                  </button>
                  <button
                    className="danger-button compact-button"
                    disabled={
                      startMutation.isPending ||
                      stopMutation.isPending ||
                      forceStartMutation.isPending ||
                      forceStopMutation.isPending ||
                      !isTauriRuntime()
                    }
                    onClick={() => forceStopMutation.mutate()}
                    type="button"
                  >
                    {forceStopMutation.isPending ? "正在强制停止…" : "强制停止"}
                  </button>
                </div>
              </details>
            )}
          </section>
        </div>
      )}

      {view === "services" && (
        <>
          <section className="external-backends-card" aria-labelledby="external-backends-title">
            <div className="routing-heading">
              <div>
                <p className="eyebrow">外部推理服务</p>
                <h2 id="external-backends-title">已配置后端</h2>
              </div>
            </div>
            {backendCatalog.data.backends.length > 0 ? (
              <div className="backend-list">
                {backendCatalog.data.backends.map((backend) => {
                  const isReferenced = backendCatalog.data.modelRoutes.some(
                    (route) => route.backendId === backend.id,
                  );
                  return (
                    <article className="backend-row" key={backend.id}>
                      <div className="backend-row-main">
                        <div>
                          <strong>{backend.displayName}</strong>
                          <span>{backendIdentityLabel(backend)}</span>
                        </div>
                        <code>{backend.apiRoot}</code>
                        <small>
                          {backendAuthLabels[backend.authMethod]}
                          {backend.credentialConfigured ? " · Keychain 已配置" : ""}
                          {backend.consecutiveFailures > 0
                            ? ` · 连续故障 ${backend.consecutiveFailures} 次`
                            : ""}
                        </small>
                        {probeBackendMutation.data?.backendId === backend.id && (
                          <small
                            className={
                              probeBackendMutation.data.status === "healthy"
                                ? "probe-result-ok"
                                : "probe-result-error"
                            }
                          >
                            {backendProbeLabels[probeBackendMutation.data.status]} ·{" "}
                            {probeBackendMutation.data.httpStatus
                              ? `HTTP ${probeBackendMutation.data.httpStatus} · `
                              : ""}
                            {probeBackendMutation.data.latencyMs} ms
                            {probeBackendMutation.data.modelCount != null
                              ? ` · ${probeBackendMutation.data.modelCount} 个模型`
                              : ""}
                          </small>
                        )}
                      </div>
                      <span
                        className={`status-pill ${
                          backend.circuitOpen
                            ? "warning"
                            : backend.isActive
                              ? "ok"
                              : backend.runtimeAvailable
                                ? "neutral"
                                : "warning"
                        }`}
                      >
                        {backend.circuitOpen
                          ? "暂时熔断"
                          : backend.isActive
                            ? "当前活动"
                            : backend.runtimeAvailable
                              ? "已加载"
                              : "凭据不可用"}
                      </span>
                      <div className="backend-row-actions">
                        <button
                          className="secondary-button compact-button"
                          disabled={!backend.runtimeAvailable || probeBackendMutation.isPending}
                          onClick={() => probeBackendMutation.mutate(backend.id)}
                          type="button"
                        >
                          测试连接
                        </button>
                        <button
                          className="secondary-button compact-button"
                          disabled={
                            backend.isActive ||
                            !backend.runtimeAvailable ||
                            activateBackendMutation.isPending ||
                            !isTauriRuntime()
                          }
                          onClick={() => activateBackendMutation.mutate(backend.id)}
                          type="button"
                        >
                          设为活动
                        </button>
                        <button
                          className="danger-button compact-button"
                          disabled={
                            backend.isActive ||
                            !backend.runtimeAvailable ||
                            forceActivateBackendMutation.isPending ||
                            !isTauriRuntime()
                          }
                          onClick={() => forceActivateBackendMutation.mutate(backend.id)}
                          title="取消旧后端的未完成请求后立即切换"
                          type="button"
                        >
                          强制切换
                        </button>
                        <button
                          className="secondary-button compact-button"
                          onClick={() =>
                            setEditingBackend({
                              id: backend.id,
                              displayName: backend.displayName,
                              kind: backend.kind,
                              engine: backend.engine,
                              adapterVariant: backend.adapterVariant,
                              apiRoot: backend.apiRoot,
                              authMethod: backend.authMethod,
                              apiKey: null,
                            })
                          }
                          type="button"
                        >
                          编辑
                        </button>
                        <button
                          aria-label={`删除后端 ${backend.displayName}`}
                          className="icon-button danger-icon-button"
                          disabled={
                            backend.isActive ||
                            isReferenced ||
                            deleteBackendMutation.isPending ||
                            !isTauriRuntime()
                          }
                          onClick={() => deleteBackendMutation.mutate(backend.id)}
                          title={isReferenced ? "请先删除引用此后端的模型别名" : "删除后端"}
                          type="button"
                        >
                          <Trash2 size={13} />
                        </button>
                      </div>
                    </article>
                  );
                })}
              </div>
            ) : (
              <div className="routing-empty empty-routing-state">
                <div>
                  <strong>还没有推理服务</strong>
                  <p>接入 OpenAI/Anthropic 兼容服务、Ollama、vLLM 或 llama.cpp Server。</p>
                </div>
                <button
                  className="primary-button"
                  onClick={() => setServiceSetupOpen(true)}
                  type="button"
                >
                  <Cable size={14} />
                  立即添加服务
                </button>
              </div>
            )}
          </section>

          <details className="routing-card disclosure-card">
            <summary className="routing-heading">
              <div>
                <p className="eyebrow">高级配置</p>
                <h2>默认服务与模型名称映射</h2>
                <small>
                  {backendCatalog.data.activeBackendId ?? "尚未配置活动后端"} ·{" "}
                  {backendCatalog.data.modelRoutes.length} 个模型别名
                </small>
              </div>
              <span className="disclosure-label">
                <span className="details-closed-copy">展开管理</span>
                <span className="details-open-copy">收起管理</span>
                <ChevronRight size={14} />
              </span>
            </summary>
            <div className="routing-metrics">
              <div>
                <span>活动后端</span>
                <strong>{backendCatalog.data.activeBackendId ?? "尚未配置"}</strong>
              </div>
              <div>
                <span>已加载后端</span>
                <strong>
                  {
                    backendCatalog.data.backends.filter((backend) => backend.runtimeAvailable)
                      .length
                  }
                </strong>
              </div>
              <div>
                <span>显式模型别名</span>
                <strong>{backendCatalog.data.modelRoutes.length}</strong>
              </div>
            </div>
            <form
              className="route-editor"
              onSubmit={(event) => {
                event.preventDefault();
                const backendId = routeBackendId || availableExternalBackends[0]?.id;
                if (!backendId) return;
                saveRouteMutation.mutate({
                  alias: routeAlias,
                  backendId,
                  resolvedModel: routeResolvedModel,
                });
              }}
            >
              <label>
                <span>对外模型别名</span>
                <input
                  aria-label="对外模型别名"
                  onChange={(event) => setRouteAlias(event.target.value)}
                  placeholder="例如 qwen-local"
                  value={routeAlias}
                />
              </label>
              <label>
                <span>目标后端</span>
                <select
                  aria-label="目标后端"
                  onChange={(event) => setRouteBackendId(event.target.value)}
                  value={routeBackendId || availableExternalBackends[0]?.id || ""}
                >
                  {backendCatalog.data.backends.map((backend) => (
                    <option
                      disabled={!backend.runtimeAvailable}
                      key={backend.id}
                      value={backend.id}
                    >
                      {backend.displayName}
                      {!backend.runtimeAvailable ? "（凭据不可用）" : ""}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                <span>后端实际模型 ID</span>
                <input
                  aria-label="后端实际模型 ID"
                  onChange={(event) => setRouteResolvedModel(event.target.value)}
                  placeholder="例如 Qwen/Qwen3.5-2B"
                  value={routeResolvedModel}
                />
              </label>
              <button
                className="secondary-button"
                disabled={
                  !isTauriRuntime() ||
                  !routeAlias.trim() ||
                  !routeResolvedModel.trim() ||
                  availableExternalBackends.length === 0 ||
                  saveRouteMutation.isPending
                }
                type="submit"
              >
                {saveRouteMutation.isPending ? "正在保存…" : "保存别名"}
              </button>
            </form>
            {backendCatalog.data.modelRoutes.length > 0 ? (
              <div className="route-list">
                {backendCatalog.data.modelRoutes.map((route) => (
                  <div key={route.alias}>
                    <code>{route.alias}</code>
                    <span>→</span>
                    <strong>{route.resolvedModel}</strong>
                    <small>{route.backendId}</small>
                    <button
                      aria-label={`删除模型别名 ${route.alias}`}
                      className="icon-button danger-icon-button"
                      disabled={deleteRouteMutation.isPending || !isTauriRuntime()}
                      onClick={() => deleteRouteMutation.mutate(route.alias)}
                      title="删除模型别名"
                      type="button"
                    >
                      <Trash2 size={13} />
                    </button>
                  </div>
                ))}
              </div>
            ) : (
              <p className="routing-empty">
                `hal100-active` 始终指向当前活动后端；尚未添加指向其他后端的显式模型别名。
              </p>
            )}
            <p className="routing-footnote">
              路由切换默认等待活动请求排空；API Key 只保存在 macOS Keychain，不写入 SQLite。
            </p>
          </details>
        </>
      )}

      {view === "runtime" && enginePlan && (
        <EngineConfirmationDialog
          applying={applyPlanMutation.isPending}
          error={applyPlanMutation.isError ? errorMessage(applyPlanMutation.error) : null}
          onApply={() => applyPlanMutation.mutate(enginePlan)}
          onCancel={() => {
            if (!applyPlanMutation.isPending) setEnginePlan(null);
          }}
          operation={enginePlan}
        />
      )}
      {view === "runtime" && modelTestOpen && (
        <Drawer
          description="发送一次非流式请求，验证本地运行时、Gateway、鉴权和 Token 计量闭环。"
          eyebrow="单轮验证"
          onClose={() => setModelTestOpen(false)}
          title="测试当前模型"
        >
          <ModelTestPanel />
        </Drawer>
      )}
      {view === "services" && serviceSetupOpen && (
        <Drawer
          description="可按需检查固定的本机回环端口，或手动输入远程服务地址。HAL100 不扫描局域网。"
          eyebrow="推理服务"
          onClose={() => setServiceSetupOpen(false)}
          title="添加推理服务"
        >
          <section className="drawer-section service-discovery-section">
            <div>
              <h3>发现本机服务</h3>
              <p>
                只检查 127.0.0.1 上 Ollama、vLLM、MLX-LM、MLC LLM、OpenVINO Model
                Server、SGLang、LMDeploy 和 llama.cpp 的常用端口。
              </p>
            </div>
            <button
              className="secondary-button"
              disabled={discoverBackendsMutation.isPending}
              onClick={() => discoverBackendsMutation.mutate()}
              type="button"
            >
              <Search size={13} />
              {discoverBackendsMutation.isPending ? "正在探测…" : "开始发现"}
            </button>
          </section>
          {discoverBackendsMutation.data && (
            <section className="discovery-results" aria-label="本机后端发现结果">
              <div className="discovery-summary">
                <strong>
                  已检查 {discoverBackendsMutation.data.checkedTargets} 个固定回环地址，发现{" "}
                  {discoverBackendsMutation.data.candidates.length} 个候选
                </strong>
                <span>探测仅在你点击后运行，不会常驻监测。</span>
              </div>
              {discoverBackendsMutation.data.candidates.map((candidate) => {
                const external = discoverBackendsMutation.data.externalEngines.find(
                  (engine) => engine.apiRoot === candidate.apiRoot,
                );
                return (
                  <div
                    className="discovery-candidate"
                    key={`${candidate.kind}-${candidate.apiRoot}`}
                  >
                    <div>
                      <strong>{candidate.displayName}</strong>
                      <code>{candidate.apiRoot}</code>
                      <small>
                        {candidate.evidence}
                        {candidate.version ? ` · ${candidate.version}` : ""}
                      </small>
                      {external?.modelCatalogComplete && (
                        <div className="discovery-models">
                          <span>{external.models.length} 个可验证模型</span>
                          {external.models.slice(0, 4).map((model) => (
                            <code
                              key={`${model.evidence.algorithm}:${model.evidence.value}`}
                              title={`${model.evidence.algorithm}: ${model.evidence.value}`}
                            >
                              {model.name}
                              {model.quantization ? ` · ${model.quantization}` : ""}
                            </code>
                          ))}
                          {external.models.length > 4 && (
                            <small>另有 {external.models.length - 4} 个</small>
                          )}
                        </div>
                      )}
                    </div>
                    <button
                      className="secondary-button compact-button"
                      onClick={() => {
                        setServiceSetupOpen(false);
                        setEditingBackend({
                          id: null,
                          displayName: candidate.displayName,
                          kind: candidate.kind,
                          engine: candidate.engine,
                          adapterVariant: candidate.adapterVariant,
                          apiRoot: candidate.apiRoot,
                          authMethod: "none",
                          apiKey: null,
                        });
                      }}
                      type="button"
                    >
                      使用此候选
                    </button>
                  </div>
                );
              })}
              {discoverBackendsMutation.data.candidates.length === 0 && (
                <p className="routing-empty">没有发现本机服务，你仍可手动填写地址。</p>
              )}
            </section>
          )}
          <section className="drawer-section service-manual-section">
            <div>
              <h3>手动连接</h3>
              <p>适用于远程 OpenAI / Anthropic 兼容服务或自定义本机端口。</p>
            </div>
            <button
              className="primary-button"
              onClick={() => {
                setServiceSetupOpen(false);
                setEditingBackend({ ...emptyBackendDraft });
              }}
              type="button"
            >
              手动填写配置
            </button>
          </section>
        </Drawer>
      )}
      {view === "services" && editingBackend && (
        <div className="dialog-backdrop" role="presentation">
          <form
            aria-labelledby="backend-editor-title"
            className="dialog backend-editor-dialog"
            onSubmit={(event) => {
              event.preventDefault();
              saveBackendMutation.mutate(editingBackend);
            }}
            role="dialog"
          >
            <div className="dialog-heading">
              <div>
                <p className="eyebrow">外部推理服务</p>
                <h2 id="backend-editor-title">
                  {editingBackend.id ? "编辑外部后端" : "添加外部后端"}
                </h2>
              </div>
              <button
                aria-label="关闭后端编辑器"
                className="icon-button"
                disabled={saveBackendMutation.isPending}
                onClick={() => setEditingBackend(null)}
                type="button"
              >
                <X size={16} />
              </button>
            </div>
            <div className="backend-editor-grid">
              <label>
                <span>显示名称</span>
                <input
                  onChange={(event) =>
                    setEditingBackend({ ...editingBackend, displayName: event.target.value })
                  }
                  placeholder="例如 工作站 vLLM"
                  value={editingBackend.displayName}
                />
              </label>
              <label>
                <span>后端类型</span>
                <select
                  onChange={(event) =>
                    setEditingBackend({
                      ...editingBackend,
                      kind: event.target.value as BackendKind,
                      engine: null,
                      adapterVariant: null,
                    })
                  }
                  value={editingBackend.kind}
                >
                  {(
                    [
                      "externalOpenAi",
                      "externalAnthropic",
                      "externalOllama",
                      "externalVllm",
                      "externalLlamaCpp",
                    ] as BackendKind[]
                  ).map((kind) => (
                    <option key={kind} value={kind}>
                      {backendKindLabels[kind]}
                    </option>
                  ))}
                </select>
              </label>
              {editingBackend.kind === "externalOpenAi" && (
                <label>
                  <span>推理引擎身份</span>
                  <select
                    onChange={(event) => {
                      const binding = explicitEngineBindings.find(
                        (candidate) => candidate.key === event.target.value,
                      );
                      setEditingBackend({
                        ...editingBackend,
                        engine: binding?.engine ?? null,
                        adapterVariant: binding?.adapterVariant ?? null,
                      });
                    }}
                    value={
                      explicitEngineBindings.find(
                        (binding) =>
                          binding.engine === editingBackend.engine &&
                          binding.adapterVariant === editingBackend.adapterVariant,
                      )?.key ?? ""
                    }
                  >
                    <option value="">通用 OpenAI 兼容（不绑定引擎）</option>
                    {explicitEngineBindings.map((binding) => (
                      <option key={binding.key} value={binding.key}>
                        {binding.label}
                      </option>
                    ))}
                  </select>
                </label>
              )}
              <label className="wide-field">
                <span>API 根地址</span>
                <input
                  onChange={(event) =>
                    setEditingBackend({ ...editingBackend, apiRoot: event.target.value })
                  }
                  placeholder="http://127.0.0.1:8000/v1/"
                  spellCheck={false}
                  value={editingBackend.apiRoot}
                />
              </label>
              <label>
                <span>认证方式</span>
                <select
                  onChange={(event) =>
                    setEditingBackend({
                      ...editingBackend,
                      authMethod: event.target.value as BackendAuthMethod,
                      apiKey: event.target.value === "none" ? null : editingBackend.apiKey,
                    })
                  }
                  value={editingBackend.authMethod}
                >
                  {(Object.keys(backendAuthLabels) as BackendAuthMethod[]).map((method) => (
                    <option key={method} value={method}>
                      {backendAuthLabels[method]}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                <span>API Key</span>
                <input
                  autoComplete="off"
                  disabled={editingBackend.authMethod === "none"}
                  onChange={(event) =>
                    setEditingBackend({
                      ...editingBackend,
                      apiKey: event.target.value || null,
                    })
                  }
                  placeholder={
                    editingBackendCredentialConfigured ? "留空则保持现有 Key" : "输入 API Key"
                  }
                  type="password"
                  value={editingBackend.apiKey ?? ""}
                />
              </label>
            </div>
            <p className="dialog-note">
              地址和非敏感元数据写入 SQLite；API Key 只写入 macOS Keychain。HAL100
              不会把它返回给网页界面。
            </p>
            {!isTauriRuntime() && (
              <p className="inline-error">浏览器预览模式只能查看编辑器，不能保存配置。</p>
            )}
            <div className="dialog-actions">
              <button
                className="secondary-button"
                disabled={saveBackendMutation.isPending}
                onClick={() => setEditingBackend(null)}
                type="button"
              >
                取消
              </button>
              <button
                className="primary-button"
                disabled={
                  !isTauriRuntime() ||
                  !editingBackend.displayName.trim() ||
                  !editingBackend.apiRoot.trim() ||
                  (editingBackend.authMethod !== "none" &&
                    !editingBackendCredentialConfigured &&
                    !editingBackend.apiKey) ||
                  saveBackendMutation.isPending
                }
                type="submit"
              >
                {saveBackendMutation.isPending ? "正在保存…" : "保存后端"}
              </button>
            </div>
          </form>
        </div>
      )}
    </div>
  );
}

function formatTokens(tokens: number | null | undefined): string {
  return tokens == null ? "—" : new Intl.NumberFormat("zh-CN").format(tokens);
}

function _formatRequestTime(timestampMs: number): string {
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(new Date(timestampMs));
}

function ModelTestPanel() {
  const queryClient = useQueryClient();
  const [prompt, setPrompt] = useState("");
  const status = useQuery({
    queryKey: ["llama-cpp-status"],
    queryFn: getLlamaCppStatus,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  const testMutation = useMutation({
    mutationFn: testActiveModel,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["usage-dashboard"] }),
  });

  if (status.isPending) {
    return <div className="state-message">正在读取当前模型状态…</div>;
  }
  if (status.isError) {
    return <div className="state-message error">{errorMessage(status.error)}</div>;
  }

  const engine = status.data;
  const runtimeReady = engine.runtimeState === "running";
  const submitTest = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmedPrompt = prompt.trim();
    if (runtimeReady && trimmedPrompt && !testMutation.isPending) {
      testMutation.mutate(trimmedPrompt);
    }
  };

  return (
    <div className="model-test-page model-test-panel">
      <div className="model-test-status">
        <span className={`status-pill ${runtimeReady ? "ok" : "neutral"}`}>
          {runtimeReady ? `运行中 · ${engine.activeModelName ?? "当前模型"}` : "尚未启动模型"}
        </span>
        <p>测试只针对当前正在运行的本地模型，不会更改模型或路由配置。</p>
      </div>

      <section className="model-test-grid model-test-panel-grid">
        <form className="model-test-composer" onSubmit={submitTest}>
          <div className="usage-section-heading">
            <div>
              <p className="eyebrow">测试输入</p>
              <h2>发送给当前模型</h2>
            </div>
            <span>{prompt.length} / 8000</span>
          </div>
          <label htmlFor="model-test-prompt">内容</label>
          <textarea
            disabled={!runtimeReady || testMutation.isPending}
            id="model-test-prompt"
            maxLength={8000}
            onChange={(event) => setPrompt(event.target.value)}
            placeholder={
              runtimeReady ? "输入一段用于验证模型的内容…" : "请先在“推理后端”中启动一个模型"
            }
            rows={9}
            value={prompt}
          />
          {!isTauriRuntime() && (
            <p className="inline-notice">浏览器预览不会发送内容；请在 Tauri 开发版中测试。</p>
          )}
          {!runtimeReady && isTauriRuntime() && (
            <p className="inline-notice">当前没有运行中的模型。启动模型后再进行测试。</p>
          )}
          {testMutation.isError && (
            <p className="inline-error">{errorMessage(testMutation.error)}</p>
          )}
          <div className="model-test-actions">
            <button
              className="primary-button"
              disabled={!runtimeReady || !prompt.trim() || testMutation.isPending}
              type="submit"
            >
              <Play size={14} />
              {testMutation.isPending ? "模型正在生成…" : "发送测试"}
            </button>
          </div>
        </form>

        <article className="model-test-response" aria-live="polite">
          <div className="usage-section-heading">
            <div>
              <p className="eyebrow">模型响应</p>
              <h2>单次结果</h2>
            </div>
            {testMutation.data && <span>{testMutation.data.elapsedMs} ms</span>}
          </div>
          {testMutation.isPending ? (
            <div className="model-test-empty">
              <RefreshCw className="model-test-empty-icon spinning" size={20} />
              <span>等待本地模型完成推理…</span>
            </div>
          ) : testMutation.data ? (
            <>
              <div className="model-test-answer">{testMutation.data.content}</div>
              <dl className="model-test-usage">
                <div>
                  <dt>输入</dt>
                  <dd>{formatTokens(testMutation.data.inputTokens)}</dd>
                </div>
                <div>
                  <dt>缓存</dt>
                  <dd>{formatTokens(testMutation.data.cachedTokens)}</dd>
                </div>
                <div>
                  <dt>输出</dt>
                  <dd>{formatTokens(testMutation.data.outputTokens)}</dd>
                </div>
                <div>
                  <dt>总计</dt>
                  <dd>{formatTokens(testMutation.data.totalTokens)}</dd>
                </div>
              </dl>
              <p className="model-test-request-id">
                {testMutation.data.requestId
                  ? `请求 ${testMutation.data.requestId}`
                  : "Gateway 未返回请求标识"}
              </p>
            </>
          ) : (
            <div className="model-test-empty">
              <FlaskConical className="model-test-empty-icon" size={22} />
              <strong>等待一次真实测试</strong>
              <span>这里不会展示模拟响应。</span>
            </div>
          )}
        </article>
      </section>
      <section className="idle-cost-note">
        <ShieldCheck className="idle-cost-icon" size={16} />
        <p>输入仅发送到本机 127.0.0.1 的 HAL100 Gateway；不会写入日志，也不会发送到云端。</p>
      </section>
    </div>
  );
}

export function ModelTestPage() {
  return <BackendsPage initialTestOpen view="runtime" />;
}
