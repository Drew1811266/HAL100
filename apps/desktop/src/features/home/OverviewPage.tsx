import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  CheckCircle2,
  ChevronRight,
  Cloud,
  Cpu,
  PackageOpen,
  Server,
  ShieldCheck,
} from "lucide-react";
import { useState } from "react";
import { Link, useNavigate, useSearchParams } from "react-router-dom";
import { Drawer } from "../../components/ui/Drawer";
import { PageHeader } from "../../components/ui/PageHeader";
import {
  activateExternalBackend,
  completeOnboarding,
  discoverLocalBackends,
  getAppOverview,
  getBackendCatalog,
  getLlamaCppStatus,
  getModelLibrary,
  getOpenCodeDetection,
  isTauriRuntime,
  type LocalBackendCandidate,
  probeExternalBackend,
  saveExternalBackend,
} from "../../lib/desktop-api";
import { buildOverviewStatus } from "../../presentation/status";

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

function FirstRunHome({
  candidate,
  discoveryError,
  discoveryPending,
  onOpenCandidate,
  showReadyLink,
}: {
  candidate: LocalBackendCandidate | null;
  discoveryError: boolean;
  discoveryPending: boolean;
  onOpenCandidate: () => void;
  showReadyLink: boolean;
}) {
  const navigate = useNavigate();
  const discoveryLabel = discoveryPending
    ? "正在检测"
    : candidate
      ? "已发现 · 推荐"
      : discoveryError
        ? "暂时无法检测"
        : "未发现服务";

  return (
    <div className="page-content first-run-page">
      <header className="first-run-header">
        <div>
          <h1>从你已有的 AI 环境开始</h1>
          <p>不必先理解模型格式或运行引擎。选择最符合你现状的方式，HAL100 会给出下一步。</p>
        </div>
        <div className="first-run-safety">
          <ShieldCheck size={16} />
          所有系统更改都会先向你确认
        </div>
      </header>

      <p className="first-run-question">你准备怎样使用 HAL100？</p>
      <section aria-label="首次使用方式" className="first-run-options">
        <button
          className={`first-run-option${candidate ? " recommended" : ""}`}
          onClick={() => (candidate ? onOpenCandidate() : navigate("/workspace/services?setup=1"))}
          type="button"
        >
          <span className={`status-pill${candidate ? " ready" : ""}`}>{discoveryLabel}</span>
          <span className="first-run-option-icon">
            <Server size={21} />
          </span>
          <strong>使用已有本地服务</strong>
          <p>
            {candidate
              ? `检测到 ${candidate.displayName}。连接后可以使用现有模型，不会修改服务配置。`
              : "连接 Ollama、MLX-LM 或其他已在本机运行的服务。"}
          </p>
          <span className="first-run-option-action">
            {candidate ? "查看并连接" : "查看连接方式"}
            <ChevronRight size={15} />
          </span>
        </button>

        <Link className="first-run-option" to="/workspace/models?setup=1">
          <span className="first-run-option-icon">
            <PackageOpen size={21} />
          </span>
          <strong>添加本地模型</strong>
          <p>在线获取适合当前设备的模型，或者导入已有 GGUF 文件，由 HAL100 在本机运行。</p>
          <span className="first-run-option-action">
            下载或导入
            <ChevronRight size={15} />
          </span>
        </Link>

        <Link className="first-run-option" to="/workspace/services?setup=cloud">
          <span className="first-run-option-icon">
            <Cloud size={21} />
          </span>
          <strong>连接云端服务</strong>
          <p>连接 OpenAI、Anthropic 或兼容服务；发送内容会在每次任务前清楚说明。</p>
          <span className="first-run-option-action">
            添加服务
            <ChevronRight size={15} />
          </span>
        </Link>
      </section>
      <div className="first-run-footnote">
        <span>这一步只选择连接方式；开机启动、下载来源等偏好稍后再设置。</span>
        {showReadyLink && <Link to="/">查看已配置后的首页</Link>}
      </div>
    </div>
  );
}

export function OverviewPage({ setupRequired }: { setupRequired: boolean }) {
  const queryClient = useQueryClient();
  const [searchParams] = useSearchParams();
  const [candidateOpen, setCandidateOpen] = useState(false);
  const showFirstRun = setupRequired || searchParams.get("guide") === "1";
  const overview = useQuery({ queryKey: ["app-overview"], queryFn: getAppOverview });
  const models = useQuery({ queryKey: ["model-library"], queryFn: getModelLibrary });
  const runtime = useQuery({ queryKey: ["llama-cpp-status"], queryFn: getLlamaCppStatus });
  const backends = useQuery({ queryKey: ["backend-catalog"], queryFn: getBackendCatalog });
  const openCode = useQuery({ queryKey: ["opencode-detection"], queryFn: getOpenCodeDetection });
  const discovery = useQuery({
    queryKey: ["local-backend-discovery", "first-run"],
    queryFn: discoverLocalBackends,
    enabled: showFirstRun,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  const candidate = discovery.data?.candidates[0] ?? null;

  const connectMutation = useMutation({
    mutationFn: async (localCandidate: LocalBackendCandidate) => {
      let catalog = backends.data;
      let backend = catalog?.backends.find(
        (item) => item.kind === localCandidate.kind && item.apiRoot === localCandidate.apiRoot,
      );
      if (!catalog || !backend) {
        catalog = await saveExternalBackend({
          id: null,
          displayName: localCandidate.displayName,
          kind: localCandidate.kind,
          engine: localCandidate.engine,
          adapterVariant: localCandidate.adapterVariant,
          apiRoot: localCandidate.apiRoot,
          authMethod: "none",
          apiKey: null,
        });
        backend = catalog.backends.find(
          (item) => item.kind === localCandidate.kind && item.apiRoot === localCandidate.apiRoot,
        );
      }
      if (!backend) throw new Error("服务已保存，但无法读取新连接，请重新检测。");
      const probe = await probeExternalBackend(backend.id);
      if (probe.status !== "healthy") {
        throw new Error("已发现服务，但连接验证没有通过。请前往连接服务查看详情。");
      }
      const activeCatalog = backend.isActive ? catalog : await activateExternalBackend(backend.id);
      const settings = await completeOnboarding();
      return { catalog: activeCatalog, settings };
    },
    onSuccess: ({ catalog, settings }) => {
      queryClient.setQueryData(["backend-catalog"], catalog);
      queryClient.setQueryData(["desktop-settings"], settings);
      queryClient.invalidateQueries({ queryKey: ["app-overview"] });
      queryClient.invalidateQueries({ queryKey: ["audit-log"] });
      setCandidateOpen(false);
    },
  });

  if (
    overview.isPending ||
    models.isPending ||
    runtime.isPending ||
    backends.isPending ||
    openCode.isPending
  ) {
    return <div className="state-message">正在读取当前环境…</div>;
  }

  if (overview.isError) {
    return <div className="state-message error">无法读取 HAL100 状态。</div>;
  }

  if (showFirstRun) {
    return (
      <>
        <FirstRunHome
          candidate={candidate}
          discoveryError={discovery.isError}
          discoveryPending={discovery.isPending}
          onOpenCandidate={() => setCandidateOpen(true)}
          showReadyLink={!setupRequired}
        />
        {candidateOpen && candidate && (
          <Drawer
            description="先验证服务是否真实可用，通过后再设为当前服务。"
            eyebrow="首次使用"
            onClose={() => setCandidateOpen(false)}
            title={`连接 ${candidate.displayName}`}
          >
            <section className="first-run-drawer-summary">
              <span className="first-run-service-mark">AI</span>
              <div>
                <strong>{candidate.displayName}</strong>
                <p>
                  {candidate.version
                    ? `检测到版本 ${candidate.version}`
                    : "已在固定本机地址发现服务"}
                </p>
              </div>
              <span className="status-pill ready">已发现</span>
            </section>
            <dl className="first-run-impact-list">
              <div>
                <dt>连接后读取</dt>
                <dd>模型名称与可用状态</dd>
              </div>
              <div>
                <dt>修改外部服务配置</dt>
                <dd>不会</dd>
              </div>
              <div>
                <dt>请求处理位置</dt>
                <dd>这台电脑</dd>
              </div>
            </dl>
            <p className="first-run-drawer-note">
              <ShieldCheck size={14} />
              HAL100 只保存连接身份，不移动模型，也不改变外部服务的启动方式。
            </p>
            {connectMutation.isError && (
              <p className="inline-error">{errorMessage(connectMutation.error)}</p>
            )}
            {!isTauriRuntime() && (
              <p className="browser-mode-note">浏览器预览不会保存或启用真实服务连接。</p>
            )}
            <div className="drawer-actions">
              <Link className="secondary-button" to="/workspace/services?setup=1">
                查看连接服务
              </Link>
              <button
                className="primary-button"
                disabled={connectMutation.isPending || !isTauriRuntime()}
                onClick={() => connectMutation.mutate(candidate)}
                type="button"
              >
                {connectMutation.isPending ? "正在验证…" : "连接并继续"}
              </button>
            </div>
          </Drawer>
        )}
      </>
    );
  }

  const modelLibrary = models.data;
  const runtimeState = runtime.data;
  const backendCatalog = backends.data;
  const readyModelCount =
    modelLibrary?.models.filter((model) => model.state === "ready").length ?? 0;
  const activeBackend = backendCatalog?.backends.find((backend) => backend.isActive) ?? null;
  const activeBackendReady = Boolean(
    activeBackend?.enabled &&
      activeBackend.runtimeAvailable &&
      !activeBackend.circuitOpen &&
      (activeBackend.authMethod === "none" || activeBackend.credentialConfigured),
  );
  const managedModelRunning = runtimeState?.runtimeState === "running";
  const activeInferenceName = activeBackendReady
    ? (activeBackend?.displayName ?? null)
    : managedModelRunning
      ? (runtimeState?.activeModelName ?? "本地模型")
      : null;
  const status = buildOverviewStatus(overview.data, {
    engineInstalled: runtimeState ? runtimeState.installState === "installed" : null,
    readyModelCount,
    managedModelRunning,
    configuredServiceCount: backendCatalog?.backends.length ?? 0,
    activeInferenceName,
    activeInferenceReady: activeBackendReady || managedModelRunning,
  });
  const openCodeConnected = openCode.data?.integrationState === "configured";

  return (
    <div className="page-content overview-page overview-ready-v2">
      <PageHeader
        description={`${status.description} 当前没有被隐藏的自动操作。`}
        title="你好，今天可以从这里开始"
      />

      <section className={`overview-ready-status ${status.status}`} aria-label="当前状态">
        <span className="overview-ready-orb">
          {status.status === "ready" ? <CheckCircle2 size={19} /> : <Server size={19} />}
        </span>
        <div>
          <h2>{status.title}</h2>
          <p>{status.description}</p>
        </div>
        <details>
          <summary>查看详情</summary>
          <dl>
            {status.details.map((detail) => (
              <div key={detail.label}>
                <dt>{detail.label}</dt>
                <dd>{detail.value}</dd>
              </div>
            ))}
          </dl>
        </details>
      </section>

      <div className="overview-ready-grid">
        <section className="overview-next-action">
          <div>
            <p className="eyebrow">推荐下一步</p>
            <h2>{status.recommendationTitle}</h2>
            <p>{status.recommendationDescription}</p>
          </div>
          <Link className="primary-button" to={status.actionPath}>
            {status.actionLabel}
            <ChevronRight size={14} />
          </Link>
        </section>

        <section className="overview-state-list" aria-label="当前工作状态">
          <article>
            <span className="overview-current-icon">AI</span>
            <div>
              <strong>{activeInferenceName ?? "尚未选择推理服务"}</strong>
              <small>当前推理</small>
            </div>
            <Link to={activeInferenceName ? "/workspace/runtime" : "/workspace/services"}>
              管理
            </Link>
          </article>
          <article>
            <span className="overview-current-icon">
              <Cpu size={16} />
            </span>
            <div>
              <strong>
                {readyModelCount > 0 ? `${readyModelCount} 个本地模型可用` : "尚未添加本地模型"}
              </strong>
              <small>模型库</small>
            </div>
            <Link to="/workspace/models">查看</Link>
          </article>
          <article>
            <span className="overview-current-icon">↗</span>
            <div>
              <strong>{openCodeConnected ? "OpenCode 已接入" : "尚未接入常用软件"}</strong>
              <small>软件接入</small>
            </div>
            <Link to="/integrations">配置</Link>
          </article>
        </section>
      </div>

      <section className="overview-recent-strip">
        <strong>最近活动</strong>
        <span>
          {activeInferenceName ? `${activeInferenceName} 当前可用` : "当前没有推理服务在运行"}
        </span>
        <Link to="/activity/usage">查看活动</Link>
      </section>
    </div>
  );
}
