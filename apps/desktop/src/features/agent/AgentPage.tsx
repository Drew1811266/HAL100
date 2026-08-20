import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  Bot,
  Check,
  ChevronRight,
  Cpu,
  Moon,
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
  configureOpenCode: {
    title: "配置 OpenCode",
    targetLabel: "目标软件",
    pendingSummary: "Agent 尚未写入任何配置",
  },
};

export function AgentPage() {
  const queryClient = useQueryClient();
  const runtime = isTauriRuntime();
  const [prompt, setPrompt] = useState(defaultAgentPrompt);
  const [result, setResult] = useState<AgentRunResult | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
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
          Agent
          只能使用固定工具并为明确操作生成计划；模型搜索只返回有界公开元数据，下载计划会绑定精确仓库、修订、文件与
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

      <section className="agent-status-strip" aria-label="Agent 运行状态">
        <article>
          <span className="agent-status-icon">
            <Bot size={18} />
          </span>
          <div>
            <small>Pi Core</small>
            <strong>v{data.piVersion}</strong>
          </div>
          <span className={`status-pill ${agentStateCopy[kernelState].tone}`}>
            {agentStateCopy[kernelState].label}
          </span>
        </article>
        <article>
          <span className="agent-status-icon">
            <Cpu size={18} />
          </span>
          <div>
            <small>本地模型</small>
            <strong>{data.modelName}</strong>
          </div>
          <span className={`status-pill ${agentStateCopy[modelState].tone}`}>
            {data.modelPrepared ? agentStateCopy[modelState].label : "尚未准备"}
          </span>
        </article>
        <article>
          <span className="agent-status-icon">
            <Moon size={18} />
          </span>
          <div>
            <small>后台策略</small>
            <strong>{data.idleTimeoutSeconds / 60} 分钟空闲退出</strong>
          </div>
        </article>
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

      <EnvironmentDiagnosticsPanel
        report={diagnostics.data}
        error={diagnostics.isError ? diagnostics.error : null}
        isFetching={diagnostics.isFetching}
        disabled={runMutation.isPending || actionMutation.isPending}
        onRefresh={() => void diagnostics.refetch()}
      />
      {providerMode === "local" && !data.modelPrepared && (
        <section className="agent-missing-model">
          <AlertTriangle size={17} />
          <div>
            <strong>Agent 模型尚未准备好</strong>
            <span>需要已校验的 Qwen3.5-2B Q4_K_M 与 HAL100 托管 llama.cpp。</span>
          </div>
          <NavLink className="secondary-button" to="/models">
            前往模型库
          </NavLink>
        </section>
      )}

      <section className="agent-workspace">
        <form className="agent-composer" onSubmit={submitPrompt}>
          <div className="agent-section-heading">
            <div>
              <p className="eyebrow">单任务会话</p>
              <h2>诊断环境或生成受控计划</h2>
            </div>
            <span className="agent-composer-context">
              <ShieldCheck size={12} />
              不保留历史
            </span>
          </div>
          <fieldset className="agent-provider-picker">
            <legend>推理范围</legend>
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
                <strong>本地 Qwen</strong>
              </label>
              <label className={providerMode === "cloud-single" ? "selected" : ""}>
                <input
                  checked={providerMode === "cloud-single"}
                  disabled={agentTransitionPending || sessionActive}
                  name="agent-provider"
                  onChange={() => {
                    setProviderMode("cloud-single");
                    setCloudRunPreview(null);
                    setCloudSessionPreview(null);
                  }}
                  type="radio"
                />
                <strong>云端单次增强</strong>
              </label>
              <label className={providerMode === "cloud-session" ? "selected" : ""}>
                <input
                  checked={providerMode === "cloud-session"}
                  disabled={agentTransitionPending || sessionActive}
                  name="agent-provider"
                  onChange={() => {
                    setProviderMode("cloud-session");
                    setCloudRunPreview(null);
                    setCloudSessionPreview(null);
                  }}
                  type="radio"
                />
                <strong>当前会话使用云端</strong>
              </label>
            </div>
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
                  <NavLink to="/backends">前往推理后端配置</NavLink>
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
              <strong>快捷任务</strong>
              <span>选择后仍可编辑</span>
            </div>
            <div className="agent-prompt-shortcuts">
              <button
                className="agent-prompt-chip"
                disabled={agentTransitionPending}
                onClick={() =>
                  updatePrompt(
                    "全面诊断 HAL100 当前运行环境，只依据 Rust 诊断结果说明问题，不要执行修复。",
                  )
                }
                type="button"
              >
                <span>全面诊断环境</span>
                <ChevronRight size={12} />
              </button>
              <button
                className="agent-prompt-chip"
                disabled={agentTransitionPending}
                onClick={() =>
                  updatePrompt(
                    "诊断并为 HAL100 当前最高优先级且可自动修复的问题生成修复计划；每次只处理一项。",
                  )
                }
                type="button"
              >
                <span>生成单项修复计划</span>
                <ChevronRight size={12} />
              </button>
              <button
                className="agent-prompt-chip"
                disabled={agentTransitionPending}
                onClick={() =>
                  updatePrompt(
                    "在 HAL100 当前默认模型来源搜索 Qwen GGUF，检查合适的公开仓库，并为一个带可信 SHA-256 的 Q4_K_M 文件生成下载计划；不要直接下载。",
                  )
                }
                type="button"
              >
                <span>搜索并规划模型下载</span>
                <ChevronRight size={12} />
              </button>
              <button
                className="agent-prompt-chip"
                disabled={agentTransitionPending}
                onClick={() => updatePrompt(defaultAgentPrompt)}
                type="button"
              >
                <span>硬件与模型建议</span>
                <ChevronRight size={12} />
              </button>
              <button
                className="agent-prompt-chip"
                disabled={agentTransitionPending}
                onClick={() =>
                  updatePrompt("列出 HAL100 当前可用模型和引擎状态，并说明当前活动模型。")
                }
                type="button"
              >
                <span>模型与引擎状态</span>
                <ChevronRight size={12} />
              </button>
            </div>
            <details className="agent-template-more">
              <summary>
                更多计划模板
                <ChevronRight size={12} />
              </summary>
              <div className="agent-prompt-shortcuts">
                <button
                  className="agent-prompt-chip"
                  disabled={agentTransitionPending}
                  onClick={() =>
                    updatePrompt("说明 HAL100 的本地 Gateway 和推理后端应该怎样配置。")
                  }
                  type="button"
                >
                  <span>Gateway 配置说明</span>
                  <ChevronRight size={12} />
                </button>
                <button
                  className="agent-prompt-chip"
                  disabled={agentTransitionPending}
                  onClick={() =>
                    updatePrompt(
                      "读取可用模型，并为 Qwen3.5-2B Q4_K_M 生成启动或安全切换计划；不要直接执行。",
                    )
                  }
                  type="button"
                >
                  <span>生成模型切换计划</span>
                  <ChevronRight size={12} />
                </button>
                <button
                  className="agent-prompt-chip"
                  disabled={agentTransitionPending}
                  onClick={() => updatePrompt("检查当前引擎状态，并生成安装 llama.cpp 的计划。")}
                  type="button"
                >
                  <span>生成引擎安装计划</span>
                  <ChevronRight size={12} />
                </button>
                <button
                  className="agent-prompt-chip"
                  disabled={agentTransitionPending}
                  onClick={() =>
                    updatePrompt("检查 OpenCode 状态，并生成接入 HAL100 Gateway 的配置计划。")
                  }
                  type="button"
                >
                  <span>生成 OpenCode 配置计划</span>
                  <ChevronRight size={12} />
                </button>
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

        <article className="agent-result" aria-live="polite">
          <div className="agent-section-heading">
            <div>
              <p className="eyebrow">结果</p>
              <h2>工具轨迹与回答</h2>
            </div>
            {elapsedSeconds && <span>{elapsedSeconds} 秒</span>}
          </div>
          <ol className="agent-result-flow" aria-label="任务处理流程">
            <li className={runMutation.isPending || result ? "active" : ""}>
              <span>1</span>
              理解任务
            </li>
            <li className={runMutation.isPending || result ? "active" : ""}>
              <span>2</span>
              调用工具
            </li>
            <li className={result ? "active" : ""}>
              <span>3</span>
              生成回答
            </li>
          </ol>
          {runMutation.isPending ? (
            <div className="agent-empty-result">
              <span className="agent-empty-icon">
                <RefreshCw className="spinning" size={20} />
              </span>
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
              <small className="agent-result-meta">
                {result.modelName} · 会话 {result.runId.slice(-8)}
              </small>
            </div>
          ) : (
            <div className="agent-empty-result">
              <span className="agent-empty-icon">
                <Bot size={21} />
              </span>
              <strong>等待一项 HAL100 管理任务</strong>
              <span>选择左侧快捷任务或输入目标，执行轨迹与受控计划会在这里逐项展开。</span>
            </div>
          )}
        </article>
      </section>
    </div>
  );
}
