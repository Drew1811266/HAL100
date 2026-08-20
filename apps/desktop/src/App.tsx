import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Activity,
  AlertTriangle,
  Bot,
  Boxes,
  Cable,
  ChartNoAxesCombined,
  ChevronRight,
  CircleGauge,
  ClipboardCopy,
  Cpu,
  Database,
  Download,
  FileCheck2,
  FlaskConical,
  FolderInput,
  FolderOpen,
  HardDrive,
  KeyRound,
  ListFilter,
  Moon,
  Play,
  RefreshCw,
  RotateCcw,
  ScrollText,
  Search,
  ServerCog,
  Settings,
  ShieldCheck,
  Square,
  Sun,
  Trash2,
  UserRoundPlus,
  X,
} from "lucide-react";
import { type ComponentType, type FormEvent, useEffect, useState } from "react";
import { Link, NavLink, Route, Routes, useNavigate } from "react-router-dom";
import { AgentPage } from "./features/agent/AgentPage";
import {
  activateExternalBackend,
  applyDataRetention,
  applyGgufImport,
  applyHermesAgentConfiguration,
  applyHermesAgentDisconnection,
  applyLlamaCppInstall,
  applyLlamaCppRemove,
  applyModelRemoval,
  applyOpenClawConfiguration,
  applyOpenClawDisconnection,
  applyOpenCodeConfiguration,
  applyOpenCodeDisconnection,
  applyPiCodingAgentConfiguration,
  applyPiCodingAgentDisconnection,
  type BackendAuthMethod,
  type BackendDraft,
  type BackendKind,
  type BackendProbeStatus,
  cancelModelDownload,
  completeOnboarding,
  createGenericClient,
  type DesktopSettings,
  type DownloadSource,
  deleteExternalBackend,
  deleteModelRoute,
  discardHermesAgentConfigurationPlan,
  discardHermesAgentDisconnectionPlan,
  discardOpenClawConfigurationPlan,
  discardOpenClawDisconnectionPlan,
  discardOpenCodeConfigurationPlan,
  discardOpenCodeDisconnectionPlan,
  discardPiCodingAgentConfigurationPlan,
  discardPiCodingAgentDisconnectionPlan,
  discoverLocalBackends,
  type EngineInstallPlan,
  type EngineRemovePlan,
  type ExternalAgentConfigurationPlan,
  type ExternalAgentDetection,
  type ExternalAgentDisconnectPlan,
  type ExternalAgentGatewayProtocol,
  type ExternalAgentIntegrationState,
  forceActivateExternalBackend,
  forceStartLlamaCppModel,
  forceStopLlamaCpp,
  type GenericClientCredential,
  type GgufImportPlan,
  getAgentEcosystemCatalog,
  getAppOverview,
  getAuditLog,
  getBackendCatalog,
  getDataCleanupPreview,
  getDesktopSettings,
  getGenericClientCatalog,
  getHardwareProfile,
  getHermesAgentDetection,
  getLlamaCppStatus,
  getModelDownloads,
  getModelLibrary,
  getOpenClawDetection,
  getOpenCodeDetection,
  getPiCodingAgentDetection,
  getRemoteModelRepository,
  getUsageDashboard,
  isTauriRuntime,
  type LocalModelSummary,
  type ModelDownloadPlan,
  type ModelDownloadSnapshot,
  type ModelRemovalPlan,
  type OpenCodeConfigPlan,
  type OpenCodeIntegrationState,
  planHermesAgentConfiguration,
  planHermesAgentDisconnection,
  planLlamaCppInstall,
  planLlamaCppRemove,
  planModelDownload,
  planModelRemoval,
  planOpenClawConfiguration,
  planOpenClawDisconnection,
  planOpenCodeConfiguration,
  planOpenCodeDisconnection,
  planPiCodingAgentConfiguration,
  planPiCodingAgentDisconnection,
  probeExternalBackend,
  type RemoteModelRepository,
  type RetentionSettingsDraft,
  resumeModelDownload,
  revokeGenericClient,
  saveExternalBackend,
  saveModelRoute,
  searchRemoteModels,
  selectAndPlanGgufImport,
  setDefaultDownloadSource,
  setLaunchAtLogin,
  startLlamaCppModel,
  startModelDownload,
  stopLlamaCpp,
  testActiveModel,
  type UsageRequestSummary,
  type UsageTotals,
  updateRetentionSettings,
} from "./lib/desktop-api";

interface NavigationItem {
  label: string;
  path: string;
  icon: ComponentType<{ size?: number; strokeWidth?: number }>;
}

const navigation: NavigationItem[] = [
  { label: "总览", path: "/", icon: CircleGauge },
  { label: "模型库", path: "/models", icon: Boxes },
  { label: "推理后端", path: "/backends", icon: ServerCog },
  { label: "软件接入", path: "/integrations", icon: Cable },
  { label: "Token 统计", path: "/usage", icon: ChartNoAxesCombined },
  { label: "HAL100 Agent", path: "/agent", icon: Bot },
  { label: "测试模型", path: "/test", icon: FlaskConical },
  { label: "审计记录", path: "/audit", icon: ScrollText },
];

function Sidebar({
  darkMode,
  onToggleTheme,
  setupRequired,
}: {
  darkMode: boolean;
  onToggleTheme: () => void;
  setupRequired: boolean;
}) {
  return (
    <aside className="sidebar">
      <div className="traffic-lights" data-tauri-drag-region aria-hidden="true">
        {!isTauriRuntime() && (
          <>
            <span />
            <span />
            <span />
          </>
        )}
      </div>
      <div className="brand">
        <span className="brand-mark" aria-hidden="true">
          <img alt="" src="/hal100-logo.png" />
        </span>
        <div>
          <strong>HAL100</strong>
          <small>本地 AI 控制台</small>
        </div>
      </div>
      <nav className="navigation" aria-label="主导航">
        {navigation.map((item) => {
          const Icon = item.icon;
          return (
            <NavLink
              className={({ isActive }) => `nav-item${isActive ? " active" : ""}`}
              end={item.path === "/"}
              key={item.path}
              to={item.path}
            >
              <Icon size={17} strokeWidth={1.8} />
              <span>{item.label}</span>
            </NavLink>
          );
        })}
      </nav>
      <div className="sidebar-footer">
        <button className="theme-button" onClick={onToggleTheme} type="button">
          {darkMode ? <Sun size={17} /> : <Moon size={17} />}
          {darkMode ? "使用浅色外观" : "使用深色外观"}
        </button>
        <NavLink className="nav-item" to="/settings">
          <Settings size={17} strokeWidth={1.8} />
          <span>设置</span>
          {setupRequired && <i aria-hidden="true" className="nav-notice-dot" />}
        </NavLink>
      </div>
    </aside>
  );
}

function OverviewPage({ setupRequired }: { setupRequired: boolean }) {
  const overview = useQuery({ queryKey: ["app-overview"], queryFn: getAppOverview });

  if (overview.isPending) {
    return <div className="state-message">正在连接 HAL100 Core…</div>;
  }

  if (overview.isError) {
    return <div className="state-message error">无法读取后台核心状态。</div>;
  }

  const data = overview.data;
  return (
    <div className="page-content overview-page">
      <header className="page-header">
        <div>
          <p className="eyebrow">总览</p>
          <h1>HAL100 已准备就绪</h1>
          <p>本地核心与统一 Gateway 正常运行。</p>
        </div>
        <span className="development-badge">开发版 {data.version}</span>
      </header>

      {setupRequired && (
        <section className="overview-setup-reminder">
          <div>
            <span className="overview-reminder-icon">
              <Settings size={16} />
            </span>
            <p>
              <strong>基础设置尚未完成</strong>
              <span>选择模型下载源并确认是否随系统登录启动即可。</span>
            </p>
          </div>
          <Link className="secondary-button" to="/settings?setup=1">
            前往设置
            <ChevronRight size={13} />
          </Link>
        </section>
      )}

      <section className="overview-health" aria-label="核心状态">
        <div>
          <span className="overview-health-icon accent">
            <Activity size={17} />
          </span>
          <p>
            <small>HAL100 Core</small>
            <strong>运行正常</strong>
          </p>
        </div>
        <div>
          <span className="overview-health-icon">
            <Cable size={17} />
          </span>
          <p>
            <small>本地 Gateway</small>
            <strong>{data.gatewayState}</strong>
          </p>
        </div>
        <div>
          <span className="overview-health-icon">
            <Database size={17} />
          </span>
          <p>
            <small>本机数据</small>
            <strong>{data.databaseState}</strong>
          </p>
        </div>
      </section>

      <section className="overview-actions" aria-labelledby="overview-actions-title">
        <div className="overview-section-heading">
          <p className="eyebrow">常用入口</p>
          <h2 id="overview-actions-title">接下来做什么</h2>
        </div>
        <div>
          <Link to="/models">
            <Boxes size={18} />
            <span>
              <strong>管理模型</strong>
              <small>下载或导入 GGUF</small>
            </span>
            <ChevronRight size={15} />
          </Link>
          <Link to="/backends">
            <ServerCog size={18} />
            <span>
              <strong>启动推理</strong>
              <small>选择模型与后端</small>
            </span>
            <ChevronRight size={15} />
          </Link>
          <Link to="/integrations">
            <Cable size={18} />
            <span>
              <strong>连接软件</strong>
              <small>OpenCode 或通用客户端</small>
            </span>
            <ChevronRight size={15} />
          </Link>
        </div>
      </section>
    </div>
  );
}

const downloadSourceCopy: Record<DownloadSource, string> = {
  huggingFace: "Hugging Face",
  modelScope: "ModelScope",
};

const modelSourceCopy = {
  huggingFace: "Hugging Face",
  modelScope: "ModelScope",
  localFile: "本地导入",
} as const;

const modelStateCopy = {
  ready: "已就绪",
  missing: "文件缺失",
  changed: "文件已变化",
  verificationFailed: "校验失败",
} as const;

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  const digits = unitIndex >= 3 && value < 10 ? 1 : 0;
  return `${value.toFixed(digits)} ${units[unitIndex]}`;
}

function isExactModelRepository(value: string): boolean {
  return /^[A-Za-z0-9._-]+\/[A-Za-z0-9._-]+$/.test(value);
}

function ModelRow({
  model,
  planning,
  onPlanRemoval,
}: {
  model: LocalModelSummary;
  planning: boolean;
  onPlanRemoval: (modelId: string) => void;
}) {
  return (
    <article className="model-row">
      <div className={`model-state-dot ${model.state}`} aria-hidden="true" />
      <div className="model-identity">
        <strong>{model.displayName}</strong>
        <span>
          {modelSourceCopy[model.source]} ·{" "}
          {model.ownership === "managed" ? "HAL100 托管" : "外部模型"}
        </span>
      </div>
      <span>{model.quantization ?? model.format.toUpperCase()}</span>
      <span>{formatBytes(model.sizeBytes)}</span>
      <span className={model.state === "ready" ? "model-ready" : "model-warning"}>
        {modelStateCopy[model.state]}
      </span>
      <button
        aria-label={`${model.ownership === "managed" ? "删除" : "移除索引"} ${model.displayName}`}
        className="model-remove-button"
        disabled={planning}
        onClick={() => onPlanRemoval(model.id)}
        title={model.ownership === "managed" ? "移到废纸篓" : "只移除 HAL100 索引"}
        type="button"
      >
        <Trash2 size={13} />
        {planning ? "检查中…" : model.ownership === "managed" ? "删除" : "移除索引"}
      </button>
    </article>
  );
}

function GgufImportConfirmationDialog({
  plan,
  applying,
  error,
  onCancel,
  onApply,
}: {
  plan: GgufImportPlan;
  applying: boolean;
  error: string | null;
  onCancel: () => void;
  onApply: () => void;
}) {
  const runtime = isTauriRuntime();
  return (
    <div className="dialog-backdrop" role="presentation">
      <section
        aria-labelledby="gguf-import-dialog-title"
        aria-modal="true"
        className="dialog"
        role="dialog"
      >
        <div className="dialog-heading">
          <div>
            <p className="eyebrow">需要确认</p>
            <h2 id="gguf-import-dialog-title">导入外部 GGUF</h2>
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
          HAL100 已完成固定文件头和文件快照检查。确认后只会把以下外部模型加入本地索引：
        </p>
        <div className="gguf-preview">
          <div className="gguf-preview-file">
            <FileCheck2 className="gguf-preview-icon" size={19} />
            <div>
              <strong>{plan.displayName}</strong>
              <code>{plan.sourcePath}</code>
            </div>
          </div>
          <dl>
            <div>
              <dt>格式</dt>
              <dd>GGUF v{plan.ggufVersion}</dd>
            </div>
            <div>
              <dt>量化</dt>
              <dd>{plan.quantization ?? "未从文件名识别"}</dd>
            </div>
            <div>
              <dt>文件大小</dt>
              <dd>{formatBytes(plan.sizeBytes)}</dd>
            </div>
            <div>
              <dt>所有权</dt>
              <dd>外部模型</dd>
            </div>
          </dl>
        </div>
        <div className="safety-summary">
          <FolderInput size={17} />
          <p>{plan.actionSummary}。确认时会再次检查路径、大小、修改时间、GGUF 头和完整 SHA-256。</p>
        </div>
        {!runtime && <p className="inline-notice">浏览器预览模式只能查看计划，不能写入索引。</p>}
        {error && <p className="inline-error">{error}</p>}
        <div className="dialog-actions">
          <button className="secondary-button" disabled={applying} onClick={onCancel} type="button">
            取消
          </button>
          <button
            className="primary-button"
            disabled={applying || !runtime}
            onClick={onApply}
            type="button"
          >
            {applying ? "正在复核并建立索引…" : "确认导入外部模型"}
          </button>
        </div>
      </section>
    </div>
  );
}

function ModelDownloadConfirmationDialog({
  plan,
  starting,
  error,
  onCancel,
  onStart,
}: {
  plan: ModelDownloadPlan;
  starting: boolean;
  error: string | null;
  onCancel: () => void;
  onStart: () => void;
}) {
  const runtime = isTauriRuntime();
  return (
    <div className="dialog-backdrop" role="presentation">
      <section
        aria-labelledby="model-download-dialog-title"
        aria-modal="true"
        className="dialog"
        role="dialog"
      >
        <div className="dialog-heading">
          <div>
            <p className="eyebrow">需要确认</p>
            <h2 id="model-download-dialog-title">下载并安装模型</h2>
          </div>
          <button
            aria-label="关闭"
            className="icon-button"
            disabled={starting}
            onClick={onCancel}
            type="button"
          >
            <X size={17} />
          </button>
        </div>
        <p className="dialog-intro">
          HAL100 已重新读取远端元数据并检查磁盘空间。确认后才会创建下载任务和托管目录。
        </p>
        <div className="model-provenance-notice">
          <AlertTriangle aria-hidden="true" size={17} />
          <p>
            GGUF
            由下方远端仓库的发布者提供。目录检索和哈希校验只验证来源与文件完整性，不代表原模型作者官方发布；请确认仓库、许可证和量化版本。
          </p>
        </div>
        <div className="gguf-preview">
          <div className="gguf-preview-file">
            <Download className="gguf-preview-icon" size={19} />
            <div>
              <strong>{plan.displayName}</strong>
              <code>{plan.file.path}</code>
            </div>
          </div>
          <dl>
            <div className="gguf-preview-wide">
              <dt>发布仓库</dt>
              <dd>
                <code>{plan.repository}</code>
              </dd>
            </div>
            <div>
              <dt>许可证</dt>
              <dd>{plan.license ?? "未声明"}</dd>
            </div>
            <div>
              <dt>来源</dt>
              <dd>{downloadSourceCopy[plan.source]}</dd>
            </div>
            <div>
              <dt>量化</dt>
              <dd>{plan.file.quantization ?? "未识别"}</dd>
            </div>
            <div>
              <dt>文件大小</dt>
              <dd>{formatBytes(plan.file.sizeBytes)}</dd>
            </div>
            <div>
              <dt>远端修订</dt>
              <dd>
                <code>{plan.file.revision}</code>
              </dd>
            </div>
            <div>
              <dt>所需空间</dt>
              <dd>{formatBytes(plan.requiredStorageBytes)}</dd>
            </div>
            <div>
              <dt>当前可用</dt>
              <dd>{formatBytes(plan.availableStorageBytes)}</dd>
            </div>
            <div className="gguf-preview-wide">
              <dt>SHA-256</dt>
              <dd>
                <code>{plan.file.sha256 ?? "不可用"}</code>
              </dd>
            </div>
          </dl>
        </div>
        <div className="safety-summary">
          <FileCheck2 size={17} />
          <p>{plan.actionSummary}。任务可取消；失败或重启后会保留分片，恢复前再次核对远端版本。</p>
        </div>
        {!runtime && <p className="inline-notice">浏览器预览模式只能查看计划，不能下载文件。</p>}
        {error && <p className="inline-error">{error}</p>}
        <div className="dialog-actions">
          <button className="secondary-button" disabled={starting} onClick={onCancel} type="button">
            取消
          </button>
          <button
            className="primary-button"
            disabled={starting || !runtime}
            onClick={onStart}
            type="button"
          >
            {starting ? "正在创建任务…" : "确认下载并安装"}
          </button>
        </div>
      </section>
    </div>
  );
}

function ModelRemovalConfirmationDialog({
  plan,
  applying,
  error,
  onCancel,
  onApply,
}: {
  plan: ModelRemovalPlan;
  applying: boolean;
  error: string | null;
  onCancel: () => void;
  onApply: () => void;
}) {
  const runtime = isTauriRuntime();
  const managedTrash = plan.removalKind === "moveManagedFileToTrash";
  const missingManaged = plan.removalKind === "removeMissingManagedIndex";
  return (
    <div className="dialog-backdrop" role="presentation">
      <section
        aria-labelledby="model-removal-dialog-title"
        aria-modal="true"
        className="dialog"
        role="dialog"
      >
        <div className="dialog-heading">
          <div>
            <p className="eyebrow">需要确认</p>
            <h2 id="model-removal-dialog-title">
              {managedTrash ? "删除托管模型" : "移除模型索引"}
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
        <p className="dialog-intro">{plan.actionSummary}。</p>
        <div className="gguf-preview">
          <div className="gguf-preview-file">
            <Trash2 className="gguf-preview-icon" size={19} />
            <div>
              <strong>{plan.displayName}</strong>
              <code>{plan.modelId}</code>
            </div>
          </div>
          <dl>
            <div>
              <dt>所有权</dt>
              <dd>{plan.ownership === "managed" ? "HAL100 托管" : "外部模型"}</dd>
            </div>
            <div>
              <dt>记录大小</dt>
              <dd>{formatBytes(plan.sizeBytes)}</dd>
            </div>
          </dl>
        </div>
        <div className="model-provenance-notice">
          <AlertTriangle aria-hidden="true" size={17} />
          <p>
            {managedTrash
              ? "确认后文件会移到 macOS 废纸篓，并从 HAL100 模型库移除；清空废纸篓前仍可恢复。"
              : missingManaged
                ? "HAL100 已确认托管文件不存在，本次只清理失效索引。"
                : "外部 GGUF 源文件不会被移动、修改或删除；本次只移除 HAL100 索引。"}
          </p>
        </div>
        {!runtime && <p className="inline-notice">浏览器预览模式不会执行模型移除。</p>}
        {error && <p className="inline-error">{error}</p>}
        <div className="dialog-actions">
          <button className="secondary-button" disabled={applying} onClick={onCancel} type="button">
            取消
          </button>
          <button
            className={managedTrash ? "danger-button" : "primary-button"}
            disabled={applying || !runtime}
            onClick={onApply}
            type="button"
          >
            {applying ? "等待原生确认…" : managedTrash ? "继续并在原生窗口确认" : "确认移除索引"}
          </button>
        </div>
      </section>
    </div>
  );
}

function readWindowActivity(): boolean {
  return document.visibilityState === "visible" && document.hasFocus();
}

function useWindowActive(): boolean {
  const [active, setActive] = useState(readWindowActivity);

  useEffect(() => {
    const update = () => setActive(readWindowActivity());
    window.addEventListener("focus", update);
    window.addEventListener("blur", update);
    document.addEventListener("visibilitychange", update);
    return () => {
      window.removeEventListener("focus", update);
      window.removeEventListener("blur", update);
      document.removeEventListener("visibilitychange", update);
    };
  }, []);

  return active;
}

function ModelsPage() {
  const queryClient = useQueryClient();
  const windowActive = useWindowActive();
  const hardware = useQuery({
    queryKey: ["hardware-profile"],
    queryFn: getHardwareProfile,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  const library = useQuery({
    queryKey: ["model-library"],
    queryFn: getModelLibrary,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  const sourceMutation = useMutation({
    mutationFn: setDefaultDownloadSource,
    onSuccess: (data) => queryClient.setQueryData(["model-library"], data),
  });
  const [importPlan, setImportPlan] = useState<GgufImportPlan | null>(null);
  const [importResult, setImportResult] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [searchSource, setSearchSource] = useState<DownloadSource | null>(null);
  const [downloadPlan, setDownloadPlan] = useState<ModelDownloadPlan | null>(null);
  const [removalPlan, setRemovalPlan] = useState<ModelRemovalPlan | null>(null);
  const selectImportMutation = useMutation({
    mutationFn: selectAndPlanGgufImport,
    onSuccess: (plan) => {
      if (plan) {
        setImportResult(null);
        setImportPlan(plan);
      }
    },
  });
  const applyImportMutation = useMutation({
    mutationFn: (planId: string) => applyGgufImport(planId),
    onSuccess: async (result) => {
      setImportPlan(null);
      setImportResult(`已索引外部模型：${result.model.displayName}`);
      await queryClient.invalidateQueries({ queryKey: ["model-library"] });
    },
  });
  const remoteSearchMutation = useMutation({
    mutationFn: ({ source, query }: { source: DownloadSource; query: string }) =>
      searchRemoteModels(source, query),
  });
  const repositoryMutation = useMutation({
    mutationFn: ({ source, repository }: { source: DownloadSource; repository: string }) =>
      getRemoteModelRepository(source, repository),
  });
  const downloads = useQuery({
    queryKey: ["model-downloads"],
    queryFn: getModelDownloads,
    refetchOnWindowFocus: false,
    refetchInterval: (query) => modelDownloadPollingInterval(windowActive, query.state.data),
  });
  const planDownloadMutation = useMutation({
    mutationFn: ({
      source,
      repository,
      remotePath,
    }: {
      source: DownloadSource;
      repository: string;
      remotePath: string;
    }) => planModelDownload(source, repository, remotePath),
    onSuccess: (plan) => setDownloadPlan(plan),
  });
  const startDownloadMutation = useMutation({
    mutationFn: startModelDownload,
    onSuccess: async () => {
      setDownloadPlan(null);
      await queryClient.invalidateQueries({ queryKey: ["model-downloads"] });
    },
  });
  const cancelDownloadMutation = useMutation({
    mutationFn: cancelModelDownload,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["model-downloads"] }),
  });
  const resumeDownloadMutation = useMutation({
    mutationFn: resumeModelDownload,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["model-downloads"] }),
  });
  const planRemovalMutation = useMutation({
    mutationFn: planModelRemoval,
    onSuccess: (plan) => {
      setImportResult(null);
      setRemovalPlan(plan);
    },
  });
  const applyRemovalMutation = useMutation({
    mutationFn: applyModelRemoval,
    onSuccess: async (result) => {
      setRemovalPlan(null);
      setImportResult(
        result.sourceFilePreserved
          ? `已移除外部索引，源文件保持不变：${result.displayName}`
          : result.removalKind === "moveManagedFileToTrash"
            ? `已将托管模型移到废纸篓：${result.displayName}`
            : `已清理失效模型索引：${result.displayName}`,
      );
      await queryClient.invalidateQueries({ queryKey: ["model-library"] });
      await queryClient.invalidateQueries({ queryKey: ["audit-log"] });
    },
  });

  useEffect(() => {
    if (downloads.data?.some((download) => download.state === "ready")) {
      void queryClient.invalidateQueries({ queryKey: ["model-library"] });
    }
  }, [downloads.data, queryClient]);

  if (hardware.isPending || library.isPending) {
    return <div className="state-message">正在按需读取硬件与模型目录…</div>;
  }
  if (hardware.isError || library.isError) {
    return (
      <div className="state-message error">{errorMessage(hardware.error ?? library.error)}</div>
    );
  }

  const profile = hardware.data;
  const modelLibrary = library.data;
  const activeSearchSource =
    searchSource ?? modelLibrary.defaultDownloadSource ?? DownloadSourceValues[0];
  return (
    <div className="page-content models-page">
      <header className="page-header model-page-header">
        <div>
          <p className="eyebrow">本地模型</p>
          <h1>模型库</h1>
          <p>管理 HAL100 托管模型和本地导入模型。硬件信息只在打开本页或手动刷新时读取。</p>
        </div>
        <div className="model-header-actions">
          <button
            className="secondary-button refresh-button"
            disabled={hardware.isFetching}
            onClick={() => hardware.refetch()}
            type="button"
          >
            <RefreshCw className={hardware.isFetching ? "spinning" : ""} size={14} />
            {hardware.isFetching ? "检测中…" : "重新检测硬件"}
          </button>
          <button
            className="primary-button refresh-button"
            disabled={selectImportMutation.isPending}
            onClick={() => selectImportMutation.mutate()}
            type="button"
          >
            <FolderInput size={14} />
            {selectImportMutation.isPending ? "正在检查…" : "导入 GGUF"}
          </button>
        </div>
      </header>

      {selectImportMutation.isError && (
        <p className="inline-error model-page-error">{errorMessage(selectImportMutation.error)}</p>
      )}
      {planRemovalMutation.isError && (
        <p className="inline-error model-page-error">{errorMessage(planRemovalMutation.error)}</p>
      )}

      <details className="hardware-card disclosure-card">
        <summary className="hardware-heading">
          <div className="hardware-icon">
            <Cpu size={20} />
          </div>
          <div>
            <p className="eyebrow">设备适配</p>
            <h2>{profile.chip}</h2>
            <small>
              {formatBytes(profile.totalUnifiedMemoryBytes)} 内存 · 建议模型{" "}
              {profile.recommendation.parameterRange}
            </small>
          </div>
          <span className="disclosure-label">
            <span className="details-closed-copy">查看详情</span>
            <span className="details-open-copy">收起详情</span>
            <ChevronRight size={14} />
          </span>
        </summary>
        <div className="hardware-disclosure-content">
          <div className="hardware-metrics">
            <div>
              <span>统一内存</span>
              <strong>{formatBytes(profile.totalUnifiedMemoryBytes)}</strong>
            </div>
            <div>
              <span>CPU 核心</span>
              <strong>{profile.physicalCpuCores} 核</strong>
            </div>
            <div>
              <span>模型空间可用</span>
              <strong>{formatBytes(profile.modelStorageAvailableBytes)}</strong>
            </div>
          </div>
          <div className="hardware-recommendation">
            <div>
              <span>保守适配建议</span>
              <strong>
                {profile.recommendation.summary} · {profile.recommendation.parameterRange}
              </strong>
            </div>
            <p>{profile.recommendation.quantization}</p>
          </div>
        </div>
      </details>

      <section className="model-settings-card">
        <div>
          <p className="eyebrow">获取偏好</p>
          <h2>默认下载源</h2>
          <p>仅作为搜索和下载的默认值；每次下载仍会记录实际来源。</p>
        </div>
        <fieldset className="source-selector">
          <legend className="source-selector-label">默认模型下载源</legend>
          {(Object.keys(downloadSourceCopy) as DownloadSource[]).map((source) => (
            <button
              aria-pressed={modelLibrary.defaultDownloadSource === source}
              disabled={sourceMutation.isPending}
              key={source}
              onClick={() => sourceMutation.mutate(source)}
              type="button"
            >
              {downloadSourceCopy[source]}
            </button>
          ))}
        </fieldset>
        <span className="source-state">
          {modelLibrary.defaultDownloadSource
            ? `当前默认：${downloadSourceCopy[modelLibrary.defaultDownloadSource]}`
            : "尚未选择，HAL100 不会替你指定来源"}
        </span>
        {sourceMutation.isError && (
          <p className="inline-error">{errorMessage(sourceMutation.error)}</p>
        )}
      </section>

      <section className="remote-catalog-card">
        <div className="remote-catalog-heading">
          <div>
            <p className="eyebrow">远程目录</p>
            <h2>查找 GGUF 模型</h2>
            <p>按需查询远程模型目录；HAL100 不会在后台轮询模型源。</p>
          </div>
          <span>最多显示 20 项</span>
        </div>
        <form
          className="model-search-form"
          onSubmit={(event) => {
            event.preventDefault();
            const query = searchQuery.trim();
            if (query.length < 2) return;
            if (isExactModelRepository(query)) {
              remoteSearchMutation.reset();
              repositoryMutation.mutate({ source: activeSearchSource, repository: query });
            } else {
              repositoryMutation.reset();
              remoteSearchMutation.mutate({ source: activeSearchSource, query });
            }
          }}
        >
          <label>
            <span>下载源</span>
            <select
              aria-label="搜索来源"
              onChange={(event) => setSearchSource(event.target.value as DownloadSource)}
              value={activeSearchSource}
            >
              {DownloadSourceValues.map((source) => (
                <option key={source} value={source}>
                  {downloadSourceCopy[source]}
                </option>
              ))}
            </select>
          </label>
          <label className="model-search-input">
            <span>模型名称或仓库</span>
            <Search aria-hidden="true" className="model-search-icon" size={15} />
            <input
              aria-label="模型名称或仓库"
              maxLength={100}
              onChange={(event) => setSearchQuery(event.target.value)}
              placeholder="例如 Qwen3 GGUF 或 owner/repository"
              type="search"
              value={searchQuery}
            />
          </label>
          <button
            className="primary-button"
            disabled={remoteSearchMutation.isPending || searchQuery.trim().length < 2}
            type="submit"
          >
            {remoteSearchMutation.isPending ? "正在查询…" : "搜索模型"}
          </button>
        </form>
        {remoteSearchMutation.isError && (
          <p className="inline-error">{errorMessage(remoteSearchMutation.error)}</p>
        )}
        {remoteSearchMutation.data && (
          <div className="remote-search-results">
            <p>
              {downloadSourceCopy[remoteSearchMutation.data.source]} 返回{" "}
              {remoteSearchMutation.data.items.length} 个结果
            </p>
            {remoteSearchMutation.data.items.length > 0 ? (
              remoteSearchMutation.data.items.map((item) => (
                <article className="remote-model-row" key={`${item.source}:${item.repository}`}>
                  <div>
                    <strong>{item.displayName}</strong>
                    <code>{item.repository}</code>
                  </div>
                  <span>{item.license ?? "许可证未声明"}</span>
                  <span>{item.gated || item.private ? "需要授权" : "公开模型"}</span>
                  <button
                    className="secondary-button"
                    disabled={repositoryMutation.isPending}
                    onClick={() =>
                      repositoryMutation.mutate({
                        source: item.source,
                        repository: item.repository,
                      })
                    }
                    type="button"
                  >
                    查看 GGUF 文件
                  </button>
                </article>
              ))
            ) : (
              <p className="remote-empty">没有找到可识别的公开 GGUF 仓库。</p>
            )}
          </div>
        )}
        {repositoryMutation.isError && (
          <p className="inline-error">{errorMessage(repositoryMutation.error)}</p>
        )}
        {repositoryMutation.data && (
          <RemoteRepositoryFiles
            planningPath={
              planDownloadMutation.isPending ? planDownloadMutation.variables?.remotePath : null
            }
            repository={repositoryMutation.data}
            onPlan={(remotePath) =>
              planDownloadMutation.mutate({
                source: repositoryMutation.data.source,
                repository: repositoryMutation.data.repository,
                remotePath,
              })
            }
          />
        )}
        {planDownloadMutation.isError && (
          <p className="inline-error">{errorMessage(planDownloadMutation.error)}</p>
        )}
        {downloads.isError && <p className="inline-error">{errorMessage(downloads.error)}</p>}
        {downloads.data && downloads.data.length > 0 && (
          <ModelDownloadTasks
            cancellingId={
              cancelDownloadMutation.isPending ? cancelDownloadMutation.variables : null
            }
            downloads={downloads.data}
            onCancel={(downloadId) => cancelDownloadMutation.mutate(downloadId)}
            onResume={(downloadId) => resumeDownloadMutation.mutate(downloadId)}
            resumingId={resumeDownloadMutation.isPending ? resumeDownloadMutation.variables : null}
          />
        )}
      </section>

      <section className="model-library-card">
        <div className="model-library-heading">
          <div>
            <p className="eyebrow">本机目录</p>
            <h2>已索引模型</h2>
          </div>
          <span>{modelLibrary.models.length} 个模型</span>
        </div>
        {importResult && <p className="inline-success">{importResult}</p>}
        {modelLibrary.models.length > 0 ? (
          <div className="model-list">
            <div className="model-list-header">
              <span>模型</span>
              <span>量化</span>
              <span>大小</span>
              <span>状态</span>
              <span>操作</span>
            </div>
            {modelLibrary.models.map((model) => (
              <ModelRow
                key={model.id}
                model={model}
                onPlanRemoval={(modelId) => planRemovalMutation.mutate(modelId)}
                planning={
                  planRemovalMutation.isPending && planRemovalMutation.variables === model.id
                }
              />
            ))}
          </div>
        ) : (
          <div className="model-empty-state">
            <Database className="empty-model-icon" size={22} />
            <strong>模型库还是空的</strong>
            <p>可从上方远程目录下载模型，或导入电脑中已有的 GGUF 文件。</p>
          </div>
        )}
        <div className="storage-path">
          <FolderOpen size={14} />
          <code>{modelLibrary.modelStoragePath}</code>
        </div>
      </section>

      <section className="idle-cost-note">
        <HardDrive className="idle-cost-icon" size={16} />
        <p>本页没有后台轮询；关闭窗口后不会继续运行硬件扫描或模型目录扫描。</p>
      </section>

      {importPlan && (
        <GgufImportConfirmationDialog
          applying={applyImportMutation.isPending}
          error={applyImportMutation.isError ? errorMessage(applyImportMutation.error) : null}
          onApply={() => applyImportMutation.mutate(importPlan.planId)}
          onCancel={() => {
            if (!applyImportMutation.isPending) setImportPlan(null);
          }}
          plan={importPlan}
        />
      )}
      {downloadPlan && (
        <ModelDownloadConfirmationDialog
          error={startDownloadMutation.isError ? errorMessage(startDownloadMutation.error) : null}
          onCancel={() => {
            if (!startDownloadMutation.isPending) setDownloadPlan(null);
          }}
          onStart={() => startDownloadMutation.mutate(downloadPlan.planId)}
          plan={downloadPlan}
          starting={startDownloadMutation.isPending}
        />
      )}
      {removalPlan && (
        <ModelRemovalConfirmationDialog
          applying={applyRemovalMutation.isPending}
          error={applyRemovalMutation.isError ? errorMessage(applyRemovalMutation.error) : null}
          onApply={() => applyRemovalMutation.mutate(removalPlan.planId)}
          onCancel={() => {
            if (!applyRemovalMutation.isPending) setRemovalPlan(null);
          }}
          plan={removalPlan}
        />
      )}
    </div>
  );
}

const DownloadSourceValues: DownloadSource[] = ["huggingFace", "modelScope"];

const activeDownloadStates = new Set(["pending", "downloading", "verifying", "installing"]);

export function modelDownloadPollingInterval(
  windowActive: boolean,
  downloads?: ModelDownloadSnapshot[],
): number | false {
  return windowActive && downloads?.some((download) => activeDownloadStates.has(download.state))
    ? 500
    : false;
}

const downloadStateCopy: Record<ModelDownloadSnapshot["state"], string> = {
  pending: "等待开始",
  downloading: "正在下载",
  paused: "已暂停",
  verifying: "正在校验",
  installing: "正在安装",
  ready: "已完成",
  failed: "下载失败",
  cancelled: "已取消",
};

function RemoteRepositoryFiles({
  repository,
  planningPath,
  onPlan,
}: {
  repository: RemoteModelRepository;
  planningPath: string | null | undefined;
  onPlan: (path: string) => void;
}) {
  return (
    <div className="remote-repository-files">
      <div>
        <div className="remote-repository-identity">
          <strong>{repository.displayName}</strong>
          <code>{repository.repository}</code>
        </div>
        <span>
          {repository.files.length} 个 GGUF 文件 · {repository.license ?? "许可证未声明"}
        </span>
      </div>
      <div className="remote-file-list">
        {repository.files.map((file) => (
          <article key={`${file.revision}:${file.path}`}>
            <div>
              <strong>{file.quantization ?? "未知量化"}</strong>
              <code>{file.path}</code>
            </div>
            <span>{formatBytes(file.sizeBytes)}</span>
            <span>{file.sha256 ? "可校验 SHA-256" : "缺少哈希"}</span>
            <button
              aria-label={`下载 ${file.quantization ?? file.path}`}
              className="secondary-button"
              disabled={!file.sha256 || (planningPath !== null && planningPath !== undefined)}
              onClick={() => onPlan(file.path)}
              title={file.sha256 ? undefined : "缺少 SHA-256，HAL100 不会安装"}
              type="button"
            >
              {planningPath === file.path ? "正在检查…" : "下载"}
            </button>
          </article>
        ))}
      </div>
    </div>
  );
}

function ModelDownloadTasks({
  downloads,
  cancellingId,
  resumingId,
  onCancel,
  onResume,
}: {
  downloads: ModelDownloadSnapshot[];
  cancellingId: string | null | undefined;
  resumingId: string | null | undefined;
  onCancel: (id: string) => void;
  onResume: (id: string) => void;
}) {
  return (
    <div className="download-task-list">
      <div className="download-task-heading">
        <strong>下载任务</strong>
        <span>仅任务运行期间刷新进度</span>
      </div>
      {downloads.map((download) => {
        const progress =
          download.expectedSizeBytes > 0
            ? Math.min(100, (download.downloadedBytes / download.expectedSizeBytes) * 100)
            : 0;
        const active = activeDownloadStates.has(download.state);
        return (
          <article key={download.downloadId}>
            <div className="download-task-title">
              <strong>{download.fileName.split("/").at(-1)}</strong>
              <span>{downloadStateCopy[download.state]}</span>
            </div>
            <div
              aria-label={`下载进度 ${Math.round(progress)}%`}
              aria-valuemax={100}
              aria-valuemin={0}
              aria-valuenow={Math.round(progress)}
              className="download-progress"
              role="progressbar"
            >
              <span style={{ width: `${progress}%` }} />
            </div>
            <div className="download-task-meta">
              <span>
                {formatBytes(download.downloadedBytes)} / {formatBytes(download.expectedSizeBytes)}
              </span>
              {download.errorCode && <code>{download.errorCode}</code>}
              {active ? (
                <button
                  className="secondary-button"
                  disabled={cancellingId === download.downloadId}
                  onClick={() => onCancel(download.downloadId)}
                  type="button"
                >
                  {cancellingId === download.downloadId ? "正在取消…" : "取消"}
                </button>
              ) : download.canResume ? (
                <button
                  className="secondary-button"
                  disabled={resumingId === download.downloadId}
                  onClick={() => onResume(download.downloadId)}
                  type="button"
                >
                  <RotateCcw size={12} />
                  {resumingId === download.downloadId ? "正在恢复…" : "继续"}
                </button>
              ) : null}
            </div>
          </article>
        );
      })}
    </div>
  );
}

const integrationStateCopy: Record<
  OpenCodeIntegrationState,
  { label: string; tone: "ok" | "neutral" | "warning" }
> = {
  notConfigured: { label: "尚未配置", tone: "neutral" },
  configured: { label: "已由 HAL100 配置", tone: "ok" },
  conflict: { label: "存在配置冲突", tone: "warning" },
  modifiedOutsideHal100: { label: "配置已被外部修改", tone: "warning" },
};

const externalIntegrationStateCopy: Record<
  ExternalAgentIntegrationState,
  { label: string; tone: "ok" | "neutral" | "warning" }
> = {
  notInstalled: { label: "未安装", tone: "neutral" },
  installedNotConfigured: { label: "尚未配置", tone: "neutral" },
  configured: { label: "已由 HAL100 配置", tone: "ok" },
  needsRefresh: { label: "配置需要刷新", tone: "warning" },
  conflict: { label: "存在配置冲突", tone: "warning" },
  modifiedOutsideHal100: { label: "配置已被外部修改", tone: "warning" },
  unsupportedVersion: { label: "版本暂不支持", tone: "warning" },
  blocked: { label: "接入被阻止", tone: "warning" },
};

const externalAgentProtocolCopy: Record<ExternalAgentGatewayProtocol, string> = {
  openAiChatCompletions: "Chat Completions",
  openAiResponses: "Responses",
  anthropicMessages: "Anthropic Messages",
};

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function ManagedAgentConfigurationDialog({
  displayName,
  plan,
  applying,
  error,
  onCancel,
  onApply,
}: {
  displayName: string;
  plan: OpenCodeConfigPlan | ExternalAgentConfigurationPlan;
  applying: boolean;
  error: string | null;
  onCancel: () => void;
  onApply: () => void;
}) {
  const runtime = isTauriRuntime();
  return (
    <div className="dialog-backdrop" role="presentation">
      <section
        aria-labelledby="managed-agent-dialog-title"
        aria-modal="true"
        className="dialog"
        role="dialog"
      >
        <div className="dialog-heading">
          <div>
            <p className="eyebrow">需要确认</p>
            <h2 id="managed-agent-dialog-title">配置 {displayName}</h2>
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
          将修改全局配置 <code>{plan.configPath}</code>。以下是唯一会由 HAL100 管理的字段：
        </p>
        <div className="change-preview">
          {plan.changes.map((change) => (
            <div key={change.path}>
              <code>+ {change.path}</code>
              <span>{change.value}</span>
            </div>
          ))}
        </div>
        <div className="safety-summary">
          <KeyRound size={17} />
          <p>
            Key 单独保存在 <code>{plan.credentialPath}</code>，权限为 0600；
            {plan.createsBackup ? "应用前会创建时间戳备份。" : "当前没有旧配置，无需创建备份。"}
            不会修改默认模型或已有 Provider。
          </p>
        </div>
        {"warnings" in plan && plan.warnings.length > 0 && (
          <div className="warning-list">
            <AlertTriangle size={17} />
            <div>
              {plan.warnings.map((warning) => (
                <p key={warning}>{warning}</p>
              ))}
            </div>
          </div>
        )}
        {!runtime && <p className="inline-notice">浏览器预览模式只能查看变更，不能应用。</p>}
        {error && <p className="inline-error">{error}</p>}
        <div className="dialog-actions">
          <button className="secondary-button" disabled={applying} onClick={onCancel} type="button">
            取消
          </button>
          <button
            className="primary-button"
            disabled={applying || !runtime}
            onClick={onApply}
            type="button"
          >
            {applying ? "正在验证并应用…" : "确认并应用配置"}
          </button>
        </div>
      </section>
    </div>
  );
}

function ManagedAgentDisconnectDialog({
  displayName,
  plan,
  applying,
  error,
  onCancel,
  onApply,
}: {
  displayName: string;
  plan: ExternalAgentDisconnectPlan;
  applying: boolean;
  error: string | null;
  onCancel: () => void;
  onApply: () => void;
}) {
  const runtime = isTauriRuntime();
  return (
    <div className="dialog-backdrop" role="presentation">
      <section
        aria-labelledby="managed-agent-disconnect-title"
        aria-modal="true"
        className="dialog"
        role="dialog"
      >
        <div className="dialog-heading">
          <div>
            <p className="eyebrow">需要确认</p>
            <h2 id="managed-agent-disconnect-title">断开 {displayName}</h2>
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
          只会从 <code>{plan.configPath}</code> 移除 HAL100 自己管理的内容：
        </p>
        <div className="change-preview">
          {plan.changes.map((change) => (
            <div key={`${change.action}:${change.path}`}>
              <code>- {change.path}</code>
              <span>
                {change.action === "removeManagedCredential"
                  ? "吊销并删除专属 Key"
                  : "移除受管分片"}
              </span>
            </div>
          ))}
        </div>
        <div className="safety-summary">
          <ShieldCheck size={17} />
          <p>
            应用前会备份配置。用户的默认模型、其他 Provider 和项目配置不会被修改；
            {displayName} 专属 Key 吊销后无法继续调用 HAL100 Gateway。
          </p>
        </div>
        {!runtime && <p className="inline-notice">浏览器预览模式只能查看变更，不能断开接入。</p>}
        {error && <p className="inline-error">{error}</p>}
        <div className="dialog-actions">
          <button className="secondary-button" disabled={applying} onClick={onCancel} type="button">
            取消
          </button>
          <button
            className="danger-button"
            disabled={applying || !runtime}
            onClick={onApply}
            type="button"
          >
            {applying ? "等待原生确认…" : "确认断开接入"}
          </button>
        </div>
      </section>
    </div>
  );
}

function GenericClientAccess() {
  const queryClient = useQueryClient();
  const [displayName, setDisplayName] = useState("");
  const [issuedCredential, setIssuedCredential] = useState<GenericClientCredential | null>(null);
  const [copied, setCopied] = useState(false);
  const catalog = useQuery({
    queryKey: ["generic-client-catalog"],
    queryFn: getGenericClientCatalog,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  const createMutation = useMutation({
    mutationFn: createGenericClient,
    onSuccess: (credential) => {
      setDisplayName("");
      setIssuedCredential(credential);
      queryClient.invalidateQueries({ queryKey: ["generic-client-catalog"] });
      queryClient.invalidateQueries({ queryKey: ["audit-log"] });
    },
  });
  const revokeMutation = useMutation({
    mutationFn: revokeGenericClient,
    onSuccess: (nextCatalog) => {
      queryClient.setQueryData(["generic-client-catalog"], nextCatalog);
      queryClient.invalidateQueries({ queryKey: ["audit-log"] });
    },
  });

  const copyIssuedKey = async () => {
    if (!issuedCredential) return;
    try {
      await navigator.clipboard.writeText(issuedCredential.apiKey);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  };

  return (
    <section className="generic-access-card" aria-labelledby="generic-access-title">
      <div className="usage-section-heading">
        <div>
          <p className="eyebrow">通用接入</p>
          <h2 id="generic-access-title">OpenAI / Anthropic 客户端</h2>
        </div>
        <span>每个软件使用独立 Key</span>
      </div>
      <p className="section-description">
        Base URL 保持固定；切换模型或后端无需修改客户端。独立 Key 让 Token 能准确归属到具体软件。
      </p>

      <div className="generic-endpoint-grid">
        <article>
          <strong>OpenAI 兼容</strong>
          <code>http://127.0.0.1:10100/v1</code>
          <small>/v1/chat/completions · /v1/responses</small>
          <span>model: hal100-active</span>
        </article>
        <article>
          <strong>Anthropic Messages</strong>
          <code>http://127.0.0.1:10100</code>
          <small>/v1/messages</small>
          <span>支持 x-api-key、SSE和缓存 Usage · model: hal100-active</span>
        </article>
      </div>

      <form
        className="generic-client-form"
        onSubmit={(event) => {
          event.preventDefault();
          const name = displayName.trim();
          if (name && !createMutation.isPending) createMutation.mutate(name);
        }}
      >
        <label htmlFor="generic-client-name">客户端名称</label>
        <input
          id="generic-client-name"
          maxLength={80}
          onChange={(event) => setDisplayName(event.target.value)}
          placeholder="例如：Continue、团队脚本、我的编辑器"
          value={displayName}
        />
        <button
          className="primary-button"
          disabled={!displayName.trim() || createMutation.isPending || !isTauriRuntime()}
          type="submit"
        >
          <UserRoundPlus size={14} />
          {createMutation.isPending ? "正在签发…" : "生成独立 Key"}
        </button>
      </form>
      {!isTauriRuntime() && (
        <p className="inline-notice">
          浏览器预览不会签发凭据；Tauri 开发版只在创建时显示一次明文 Key。
        </p>
      )}
      {createMutation.isError && (
        <p className="inline-error">{errorMessage(createMutation.error)}</p>
      )}
      {revokeMutation.isError && (
        <p className="inline-error">{errorMessage(revokeMutation.error)}</p>
      )}

      {catalog.isPending ? (
        <div className="state-message compact-state">正在读取本地客户端凭据…</div>
      ) : catalog.isError ? (
        <div className="state-message error compact-state">{errorMessage(catalog.error)}</div>
      ) : catalog.data.clients.length === 0 ? (
        <div className="generic-client-empty">
          <KeyRound size={18} />
          <span>尚未签发通用客户端 Key。OpenCode 专属凭据不会显示在这里。</span>
        </div>
      ) : (
        <div className="generic-client-list">
          {catalog.data.clients.map((client) => (
            <article key={client.clientAppId}>
              <div>
                <strong>{client.displayName}</strong>
                <span>{client.displayPrefix}</span>
              </div>
              <small>{formatRequestTime(client.createdAtMs)}</small>
              <button
                className="danger-button compact-button"
                disabled={revokeMutation.isPending}
                onClick={() => revokeMutation.mutate(client.clientAppId)}
                type="button"
              >
                撤销 Key
              </button>
            </article>
          ))}
        </div>
      )}

      {issuedCredential && (
        <div className="dialog-backdrop" role="presentation">
          <section
            aria-labelledby="issued-key-title"
            aria-modal="true"
            className="dialog issued-key-dialog"
            role="dialog"
          >
            <div className="dialog-heading">
              <div>
                <p className="eyebrow">仅显示一次</p>
                <h2 id="issued-key-title">保存 {issuedCredential.client.displayName} 的 Key</h2>
              </div>
            </div>
            <p>关闭后 HAL100 无法再次显示明文；数据库只保存 SHA-256 摘要。</p>
            <code className="issued-key-value">{issuedCredential.apiKey}</code>
            <div className="dialog-actions">
              <button className="secondary-button" onClick={copyIssuedKey} type="button">
                <ClipboardCopy size={14} />
                {copied ? "已复制" : "复制 Key"}
              </button>
              <button
                className="primary-button"
                onClick={() => {
                  setIssuedCredential(null);
                  setCopied(false);
                }}
                type="button"
              >
                我已保存，关闭
              </button>
            </div>
          </section>
        </div>
      )}
    </section>
  );
}

function PiCodingAgentIntegrationCard() {
  const queryClient = useQueryClient();
  const detection = useQuery<ExternalAgentDetection>({
    queryKey: ["pi-coding-agent-detection"],
    queryFn: getPiCodingAgentDetection,
  });
  const [plan, setPlan] = useState<ExternalAgentConfigurationPlan | null>(null);
  const [disconnectPlan, setDisconnectPlan] = useState<ExternalAgentDisconnectPlan | null>(null);
  const [resultMessage, setResultMessage] = useState<string | null>(null);
  const planMutation = useMutation({
    mutationFn: planPiCodingAgentConfiguration,
    onSuccess: (nextPlan) => {
      setResultMessage(null);
      setPlan(nextPlan);
    },
  });
  const applyMutation = useMutation({
    mutationFn: (planId: string) => applyPiCodingAgentConfiguration(planId),
    onSuccess: async (result) => {
      setPlan(null);
      setResultMessage(
        result.backupPath
          ? `Pi 配置完成，备份已保存到 ${result.backupPath}`
          : "Pi 配置完成，独立凭据已生效。",
      );
      await queryClient.invalidateQueries({ queryKey: ["pi-coding-agent-detection"] });
    },
  });
  const disconnectPlanMutation = useMutation({
    mutationFn: planPiCodingAgentDisconnection,
    onSuccess: (nextPlan) => {
      setResultMessage(null);
      setDisconnectPlan(nextPlan);
    },
  });
  const disconnectMutation = useMutation({
    mutationFn: (planId: string) => applyPiCodingAgentDisconnection(planId),
    onSuccess: async (result) => {
      setDisconnectPlan(null);
      setResultMessage(
        result.backupPath
          ? `Pi 接入已断开，配置备份保存在 ${result.backupPath}`
          : "Pi 接入已断开，专属凭据已吊销。",
      );
      await queryClient.invalidateQueries({ queryKey: ["pi-coding-agent-detection"] });
    },
  });

  if (detection.isPending) {
    return <section className="integration-card state-message">正在检测 Pi Coding Agent…</section>;
  }
  if (detection.isError) {
    return (
      <section className="integration-card state-message error">
        {errorMessage(detection.error)}
      </section>
    );
  }

  const data = detection.data;
  const stateCopy = externalIntegrationStateCopy[data.integrationState];
  const connected =
    data.integrationState === "configured" || data.integrationState === "needsRefresh";
  const cannotConfigure =
    !data.installed ||
    data.integrationState === "conflict" ||
    data.integrationState === "modifiedOutsideHal100" ||
    data.integrationState === "unsupportedVersion" ||
    data.integrationState === "blocked";

  return (
    <>
      <section className="integration-card">
        <div className="integration-heading">
          <div className="integration-brand">π</div>
          <div>
            <h2>Pi Coding Agent</h2>
            <p>
              {data.installed
                ? `已检测到 ${data.version ?? "未知版本"}`
                : "未从常用安装位置检测到官方 Pi CLI"}
            </p>
          </div>
          <span className={`status-pill ${stateCopy.tone}`}>{stateCopy.label}</span>
        </div>
        <details className="inline-disclosure">
          <summary>
            <span>连接详情</span>
            <ChevronRight size={14} />
          </summary>
          <dl className="integration-details">
            <div>
              <dt>Gateway Base URL</dt>
              <dd>http://127.0.0.1:10100/v1</dd>
            </div>
            <div>
              <dt>Pi 模型配置</dt>
              <dd>{data.configPath}</dd>
            </div>
            <div>
              <dt>模型契约</dt>
              <dd>{data.modelProfileRevision}</dd>
            </div>
            <div>
              <dt>隔离边界</dt>
              <dd>{connected ? "Pi 专属 Key · 独立于内置 Runtime" : "配置后启用"}</dd>
            </div>
          </dl>
        </details>
        {data.warnings.length > 0 && (
          <div className="warning-list">
            <AlertTriangle size={17} />
            <div>
              {data.warnings.map((warning) => (
                <p key={warning}>{warning}</p>
              ))}
            </div>
          </div>
        )}
        {(planMutation.isError || disconnectPlanMutation.isError) && (
          <p className="inline-error">
            {errorMessage(planMutation.error ?? disconnectPlanMutation.error)}
          </p>
        )}
        {resultMessage && <p className="inline-success">{resultMessage}</p>}
        <div className="integration-actions">
          <button
            className="secondary-button"
            disabled={detection.isFetching}
            onClick={() => detection.refetch()}
            type="button"
          >
            {detection.isFetching ? "检测中…" : "重新检测"}
          </button>
          <button
            className="primary-button"
            disabled={
              cannotConfigure || planMutation.isPending || data.integrationState === "configured"
            }
            onClick={() => planMutation.mutate()}
            type="button"
          >
            {planMutation.isPending
              ? "正在生成预览…"
              : data.integrationState === "configured"
                ? "配置已生效"
                : data.integrationState === "needsRefresh"
                  ? "刷新 Pi 配置"
                  : "配置 Pi"}
          </button>
          {connected && (
            <button
              className="danger-button"
              disabled={disconnectPlanMutation.isPending}
              onClick={() => disconnectPlanMutation.mutate()}
              type="button"
            >
              {disconnectPlanMutation.isPending ? "正在生成预览…" : "断开接入"}
            </button>
          )}
        </div>
      </section>
      {plan && (
        <ManagedAgentConfigurationDialog
          applying={applyMutation.isPending}
          displayName="Pi Coding Agent"
          error={applyMutation.isError ? errorMessage(applyMutation.error) : null}
          onApply={() => applyMutation.mutate(plan.planId)}
          onCancel={() => {
            if (!applyMutation.isPending) {
              void discardPiCodingAgentConfigurationPlan(plan.planId);
              setPlan(null);
            }
          }}
          plan={plan}
        />
      )}
      {disconnectPlan && (
        <ManagedAgentDisconnectDialog
          applying={disconnectMutation.isPending}
          displayName="Pi Coding Agent"
          error={disconnectMutation.isError ? errorMessage(disconnectMutation.error) : null}
          onApply={() => disconnectMutation.mutate(disconnectPlan.planId)}
          onCancel={() => {
            if (!disconnectMutation.isPending) {
              void discardPiCodingAgentDisconnectionPlan(disconnectPlan.planId);
              setDisconnectPlan(null);
            }
          }}
          plan={disconnectPlan}
        />
      )}
    </>
  );
}

function OpenClawIntegrationCard() {
  const queryClient = useQueryClient();
  const detection = useQuery<ExternalAgentDetection>({
    queryKey: ["openclaw-detection"],
    queryFn: getOpenClawDetection,
  });
  const [protocol, setProtocol] = useState<ExternalAgentGatewayProtocol>("openAiChatCompletions");
  const [plan, setPlan] = useState<ExternalAgentConfigurationPlan | null>(null);
  const [disconnectPlan, setDisconnectPlan] = useState<ExternalAgentDisconnectPlan | null>(null);
  const [resultMessage, setResultMessage] = useState<string | null>(null);
  useEffect(() => {
    if (detection.data?.configuredProtocol) {
      setProtocol(detection.data.configuredProtocol);
    }
  }, [detection.data?.configuredProtocol]);
  const planMutation = useMutation({
    mutationFn: (selectedProtocol: ExternalAgentGatewayProtocol) =>
      planOpenClawConfiguration(selectedProtocol),
    onSuccess: (nextPlan) => {
      setResultMessage(null);
      setPlan(nextPlan);
    },
  });
  const applyMutation = useMutation({
    mutationFn: (planId: string) => applyOpenClawConfiguration(planId),
    onSuccess: async (result) => {
      setPlan(null);
      setResultMessage(
        result.backupPath
          ? `OpenClaw 配置完成，备份已保存到 ${result.backupPath}`
          : "OpenClaw 配置完成，独立凭据已生效。",
      );
      await queryClient.invalidateQueries({ queryKey: ["openclaw-detection"] });
    },
  });
  const disconnectPlanMutation = useMutation({
    mutationFn: planOpenClawDisconnection,
    onSuccess: (nextPlan) => {
      setResultMessage(null);
      setDisconnectPlan(nextPlan);
    },
  });
  const disconnectMutation = useMutation({
    mutationFn: (planId: string) => applyOpenClawDisconnection(planId),
    onSuccess: async (result) => {
      setDisconnectPlan(null);
      setResultMessage(
        result.backupPath
          ? `OpenClaw 接入已断开，配置备份保存在 ${result.backupPath}`
          : "OpenClaw 接入已断开，专属凭据已吊销。",
      );
      await queryClient.invalidateQueries({ queryKey: ["openclaw-detection"] });
    },
  });

  if (detection.isPending) {
    return <section className="integration-card state-message">正在检测 OpenClaw…</section>;
  }
  if (detection.isError) {
    return (
      <section className="integration-card state-message error">
        {errorMessage(detection.error)}
      </section>
    );
  }

  const data = detection.data;
  const stateCopy = externalIntegrationStateCopy[data.integrationState];
  const connected =
    data.integrationState === "configured" || data.integrationState === "needsRefresh";
  const selectedProtocolAlreadyActive =
    data.integrationState === "configured" && data.configuredProtocol === protocol;
  const cannotConfigure =
    !data.installed ||
    data.integrationState === "conflict" ||
    data.integrationState === "modifiedOutsideHal100" ||
    data.integrationState === "unsupportedVersion" ||
    data.integrationState === "blocked";

  return (
    <>
      <section className="integration-card">
        <div className="integration-heading">
          <div className="integration-brand openclaw-brand">CL</div>
          <div>
            <h2>OpenClaw</h2>
            <p>
              {data.installed
                ? `已检测到 ${data.version ?? "未知版本"}`
                : "未从常用安装位置检测到官方 OpenClaw CLI"}
            </p>
          </div>
          <span className={`status-pill ${stateCopy.tone}`}>{stateCopy.label}</span>
        </div>
        <details className="inline-disclosure">
          <summary>
            <span>连接详情</span>
            <ChevronRight size={14} />
          </summary>
          <dl className="integration-details">
            <div>
              <dt>OpenClaw 配置</dt>
              <dd>{data.configPath}</dd>
            </div>
            <div>
              <dt>当前协议</dt>
              <dd>
                {data.configuredProtocol
                  ? externalAgentProtocolCopy[data.configuredProtocol]
                  : "尚未配置"}
              </dd>
            </div>
            <div>
              <dt>模型契约</dt>
              <dd>{data.modelProfileRevision}</dd>
            </div>
            <div>
              <dt>隔离边界</dt>
              <dd>{connected ? "OpenClaw 专属 Key · 文件型 SecretRef" : "配置后启用"}</dd>
            </div>
          </dl>
        </details>
        <label className="integration-protocol-selector">
          <span>Gateway 协议</span>
          <select
            disabled={planMutation.isPending || applyMutation.isPending}
            onChange={(event) => setProtocol(event.target.value as ExternalAgentGatewayProtocol)}
            value={protocol}
          >
            <option value="openAiChatCompletions">Chat Completions</option>
            <option value="openAiResponses">Responses</option>
            <option value="anthropicMessages">Anthropic Messages</option>
          </select>
          <small>切换协议只替换 HAL100 自己的 Provider 分片，不改变 OpenClaw 默认模型。</small>
        </label>
        {data.warnings.length > 0 && (
          <div className="warning-list">
            <AlertTriangle size={17} />
            <div>
              {data.warnings.map((warning) => (
                <p key={warning}>{warning}</p>
              ))}
            </div>
          </div>
        )}
        {(planMutation.isError || disconnectPlanMutation.isError) && (
          <p className="inline-error">
            {errorMessage(planMutation.error ?? disconnectPlanMutation.error)}
          </p>
        )}
        {resultMessage && <p className="inline-success">{resultMessage}</p>}
        <div className="integration-actions">
          <button
            className="secondary-button"
            disabled={detection.isFetching}
            onClick={() => detection.refetch()}
            type="button"
          >
            {detection.isFetching ? "检测中…" : "重新检测"}
          </button>
          <button
            className="primary-button"
            disabled={cannotConfigure || planMutation.isPending || selectedProtocolAlreadyActive}
            onClick={() => planMutation.mutate(protocol)}
            type="button"
          >
            {planMutation.isPending
              ? "正在调用官方工具验证…"
              : selectedProtocolAlreadyActive
                ? "所选协议已生效"
                : connected
                  ? "切换 OpenClaw 协议"
                  : "配置 OpenClaw"}
          </button>
          {connected && (
            <button
              className="danger-button"
              disabled={disconnectPlanMutation.isPending}
              onClick={() => disconnectPlanMutation.mutate()}
              type="button"
            >
              {disconnectPlanMutation.isPending ? "正在生成预览…" : "断开接入"}
            </button>
          )}
        </div>
      </section>
      {plan && (
        <ManagedAgentConfigurationDialog
          applying={applyMutation.isPending}
          displayName="OpenClaw"
          error={applyMutation.isError ? errorMessage(applyMutation.error) : null}
          onApply={() => applyMutation.mutate(plan.planId)}
          onCancel={() => {
            if (!applyMutation.isPending) {
              void discardOpenClawConfigurationPlan(plan.planId);
              setPlan(null);
            }
          }}
          plan={plan}
        />
      )}
      {disconnectPlan && (
        <ManagedAgentDisconnectDialog
          applying={disconnectMutation.isPending}
          displayName="OpenClaw"
          error={disconnectMutation.isError ? errorMessage(disconnectMutation.error) : null}
          onApply={() => disconnectMutation.mutate(disconnectPlan.planId)}
          onCancel={() => {
            if (!disconnectMutation.isPending) {
              void discardOpenClawDisconnectionPlan(disconnectPlan.planId);
              setDisconnectPlan(null);
            }
          }}
          plan={disconnectPlan}
        />
      )}
    </>
  );
}

function HermesAgentIntegrationCard() {
  const queryClient = useQueryClient();
  const detection = useQuery<ExternalAgentDetection>({
    queryKey: ["hermes-agent-detection"],
    queryFn: getHermesAgentDetection,
  });
  const [plan, setPlan] = useState<ExternalAgentConfigurationPlan | null>(null);
  const [disconnectPlan, setDisconnectPlan] = useState<ExternalAgentDisconnectPlan | null>(null);
  const [resultMessage, setResultMessage] = useState<string | null>(null);
  const planMutation = useMutation({
    mutationFn: planHermesAgentConfiguration,
    onSuccess: (nextPlan) => {
      setResultMessage(null);
      setPlan(nextPlan);
    },
  });
  const applyMutation = useMutation({
    mutationFn: (planId: string) => applyHermesAgentConfiguration(planId),
    onSuccess: async (result) => {
      setPlan(null);
      setResultMessage(
        result.backupPath
          ? `Hermes 配置完成，非敏感 YAML 备份已保存到 ${result.backupPath}`
          : "Hermes 配置完成，独立凭据已生效。",
      );
      await queryClient.invalidateQueries({ queryKey: ["hermes-agent-detection"] });
    },
  });
  const disconnectPlanMutation = useMutation({
    mutationFn: planHermesAgentDisconnection,
    onSuccess: (nextPlan) => {
      setResultMessage(null);
      setDisconnectPlan(nextPlan);
    },
  });
  const disconnectMutation = useMutation({
    mutationFn: (planId: string) => applyHermesAgentDisconnection(planId),
    onSuccess: async (result) => {
      setDisconnectPlan(null);
      setResultMessage(
        result.backupPath
          ? `Hermes 接入已断开，非敏感 YAML 备份保存在 ${result.backupPath}`
          : "Hermes 接入已断开，专属凭据已吊销。",
      );
      await queryClient.invalidateQueries({ queryKey: ["hermes-agent-detection"] });
    },
  });

  if (detection.isPending) {
    return <section className="integration-card state-message">正在检测 Hermes Agent…</section>;
  }
  if (detection.isError) {
    return (
      <section className="integration-card state-message error">
        {errorMessage(detection.error)}
      </section>
    );
  }

  const data = detection.data;
  const stateCopy = externalIntegrationStateCopy[data.integrationState];
  const connected =
    data.integrationState === "configured" || data.integrationState === "needsRefresh";
  const cannotConfigure =
    !data.installed ||
    data.integrationState === "conflict" ||
    data.integrationState === "modifiedOutsideHal100" ||
    data.integrationState === "unsupportedVersion" ||
    data.integrationState === "blocked";

  return (
    <>
      <section className="integration-card">
        <div className="integration-heading">
          <div className="integration-brand hermes-brand">H</div>
          <div>
            <h2>Hermes Agent</h2>
            <p>
              {data.installed
                ? `已检测到 ${data.version ?? "未知版本"}`
                : "未从常用安装位置检测到官方 Hermes CLI"}
            </p>
          </div>
          <span className={`status-pill ${stateCopy.tone}`}>{stateCopy.label}</span>
        </div>
        <details className="inline-disclosure">
          <summary>
            <span>连接详情</span>
            <ChevronRight size={14} />
          </summary>
          <dl className="integration-details">
            <div>
              <dt>Hermes default Profile</dt>
              <dd>{data.configPath}</dd>
            </div>
            <div>
              <dt>Gateway 协议</dt>
              <dd>Chat Completions</dd>
            </div>
            <div>
              <dt>运行前置条件</dt>
              <dd>Hermes ≥ 0.18.2 · 上下文 ≥ 64000 Token</dd>
            </div>
            <div>
              <dt>隔离边界</dt>
              <dd>
                {connected
                  ? "Hermes 专属 Key · .env 独立变量"
                  : "只管理 providers.hal100 与专属变量"}
              </dd>
            </div>
          </dl>
        </details>
        {data.warnings.length > 0 && (
          <div className="warning-list">
            <AlertTriangle size={17} />
            <div>
              {data.warnings.map((warning) => (
                <p key={warning}>{warning}</p>
              ))}
            </div>
          </div>
        )}
        {(planMutation.isError || disconnectPlanMutation.isError) && (
          <p className="inline-error">
            {errorMessage(planMutation.error ?? disconnectPlanMutation.error)}
          </p>
        )}
        {resultMessage && <p className="inline-success">{resultMessage}</p>}
        <div className="integration-actions">
          <button
            className="secondary-button"
            disabled={detection.isFetching}
            onClick={() => detection.refetch()}
            type="button"
          >
            {detection.isFetching ? "检测中…" : "重新检测"}
          </button>
          <button
            className="primary-button"
            disabled={
              cannotConfigure || planMutation.isPending || data.integrationState === "configured"
            }
            onClick={() => planMutation.mutate()}
            type="button"
          >
            {planMutation.isPending
              ? "正在调用官方 CLI 验证…"
              : data.integrationState === "configured"
                ? "配置已生效"
                : data.integrationState === "needsRefresh"
                  ? "刷新 Hermes 配置"
                  : "配置 Hermes"}
          </button>
          {connected && (
            <button
              className="danger-button"
              disabled={disconnectPlanMutation.isPending}
              onClick={() => disconnectPlanMutation.mutate()}
              type="button"
            >
              {disconnectPlanMutation.isPending ? "正在生成预览…" : "断开接入"}
            </button>
          )}
        </div>
      </section>
      {plan && (
        <ManagedAgentConfigurationDialog
          applying={applyMutation.isPending}
          displayName="Hermes Agent"
          error={applyMutation.isError ? errorMessage(applyMutation.error) : null}
          onApply={() => applyMutation.mutate(plan.planId)}
          onCancel={() => {
            if (!applyMutation.isPending) {
              void discardHermesAgentConfigurationPlan(plan.planId);
              setPlan(null);
            }
          }}
          plan={plan}
        />
      )}
      {disconnectPlan && (
        <ManagedAgentDisconnectDialog
          applying={disconnectMutation.isPending}
          displayName="Hermes Agent"
          error={disconnectMutation.isError ? errorMessage(disconnectMutation.error) : null}
          onApply={() => disconnectMutation.mutate(disconnectPlan.planId)}
          onCancel={() => {
            if (!disconnectMutation.isPending) {
              void discardHermesAgentDisconnectionPlan(disconnectPlan.planId);
              setDisconnectPlan(null);
            }
          }}
          plan={disconnectPlan}
        />
      )}
    </>
  );
}

function IntegrationsPage() {
  const queryClient = useQueryClient();
  const ecosystem = useQuery({
    queryKey: ["agent-ecosystem-catalog"],
    queryFn: getAgentEcosystemCatalog,
  });
  const detection = useQuery({ queryKey: ["opencode-detection"], queryFn: getOpenCodeDetection });
  const [plan, setPlan] = useState<OpenCodeConfigPlan | null>(null);
  const [disconnectPlan, setDisconnectPlan] = useState<ExternalAgentDisconnectPlan | null>(null);
  const [resultMessage, setResultMessage] = useState<string | null>(null);
  const planMutation = useMutation({
    mutationFn: planOpenCodeConfiguration,
    onSuccess: (nextPlan) => {
      setResultMessage(null);
      setPlan(nextPlan);
    },
  });
  const applyMutation = useMutation({
    mutationFn: (planId: string) => applyOpenCodeConfiguration(planId),
    onSuccess: async (result) => {
      setPlan(null);
      setResultMessage(
        result.backupPath
          ? `配置完成，备份已保存到 ${result.backupPath}`
          : "配置完成，OpenCode 专属凭据已生效。",
      );
      await queryClient.invalidateQueries({ queryKey: ["opencode-detection"] });
    },
  });
  const disconnectPlanMutation = useMutation({
    mutationFn: planOpenCodeDisconnection,
    onSuccess: (nextPlan) => {
      setResultMessage(null);
      setDisconnectPlan(nextPlan);
    },
  });
  const disconnectMutation = useMutation({
    mutationFn: (planId: string) => applyOpenCodeDisconnection(planId),
    onSuccess: async (result) => {
      setDisconnectPlan(null);
      setResultMessage(
        result.backupPath
          ? `接入已断开，配置备份保存在 ${result.backupPath}`
          : "接入已断开，OpenCode 专属凭据已吊销。",
      );
      await queryClient.invalidateQueries({ queryKey: ["opencode-detection"] });
    },
  });

  if (detection.isPending || ecosystem.isPending) {
    return <div className="state-message">正在读取 Agent 接入边界并检测外部客户端…</div>;
  }
  if (detection.isError || ecosystem.isError) {
    return (
      <div className="state-message error">{errorMessage(detection.error ?? ecosystem.error)}</div>
    );
  }
  const data = detection.data;
  const ecosystemData = ecosystem.data;
  const stateCopy = integrationStateCopy[data.integrationState];
  const cannotPlan =
    data.integrationState === "conflict" || data.integrationState === "modifiedOutsideHal100";

  return (
    <div className="page-content integrations-page">
      <header className="page-header">
        <div>
          <p className="eyebrow">客户端接入</p>
          <h1>软件接入</h1>
          <p>让外部 Agent 通过固定的 HAL100 Gateway 调用模型，并按独立身份准确归集 Token。</p>
        </div>
      </header>

      <section className="agent-boundary-card" aria-labelledby="agent-boundary-title">
        <div className="agent-boundary-heading">
          <div>
            <p className="eyebrow">运行边界</p>
            <h2 id="agent-boundary-title">内置 Runtime 与外部 Agent 相互独立</h2>
          </div>
          <ShieldCheck size={20} />
        </div>
        <div className="agent-boundary-grid">
          <article>
            <span className="boundary-kind">HAL100 私有组件</span>
            <strong>{ecosystemData.builtInRuntime.displayName}（内置）</strong>
            <p>
              底层使用固定版本 {ecosystemData.builtInRuntime.engineName}；
              {ecosystemData.builtInRuntime.isolationSummary}。
            </p>
            <code>{ecosystemData.builtInRuntime.clientAppId}</code>
          </article>
          <article>
            <span className="boundary-kind">用户安装的软件</span>
            <strong>外部 Agent 集成</strong>
            <p>独立安装、配置、会话和升级；HAL100 只管理明确预览过的配置片段和专属 Key。</p>
            <code>opencode · pi-coding-agent · openclaw · hermes-agent</code>
          </article>
        </div>
      </section>

      <section className="integration-card">
        <div className="integration-heading">
          <div className="integration-brand">OC</div>
          <div>
            <h2>OpenCode</h2>
            <p>
              {data.installed
                ? `已检测到 ${data.version ?? "未知版本"}`
                : "未从常用安装位置检测到 CLI"}
            </p>
          </div>
          <span className={`status-pill ${stateCopy.tone}`}>{stateCopy.label}</span>
        </div>
        <details className="inline-disclosure">
          <summary>
            <span>连接详情</span>
            <ChevronRight size={14} />
          </summary>
          <dl className="integration-details">
            <div>
              <dt>Gateway Base URL</dt>
              <dd>http://127.0.0.1:10100/v1</dd>
            </div>
            <div>
              <dt>全局配置</dt>
              <dd>{data.configPath}</dd>
            </div>
            <div>
              <dt>配置格式</dt>
              <dd>{data.configFormat.toUpperCase()}</dd>
            </div>
            <div>
              <dt>用量归属</dt>
              <dd>{data.integrationState === "configured" ? "OpenCode 专属 Key" : "配置后启用"}</dd>
            </div>
          </dl>
        </details>

        {data.warnings.length > 0 && (
          <div className="warning-list">
            <AlertTriangle size={17} />
            <div>
              {data.warnings.map((warning) => (
                <p key={warning}>{warning}</p>
              ))}
            </div>
          </div>
        )}
        {(planMutation.isError || disconnectPlanMutation.isError) && (
          <p className="inline-error">
            {errorMessage(planMutation.error ?? disconnectPlanMutation.error)}
          </p>
        )}
        {resultMessage && <p className="inline-success">{resultMessage}</p>}

        <div className="integration-actions">
          <button
            className="secondary-button"
            disabled={detection.isFetching}
            onClick={() => detection.refetch()}
            type="button"
          >
            {detection.isFetching ? "检测中…" : "重新检测"}
          </button>
          <button
            className="primary-button"
            disabled={
              cannotPlan || planMutation.isPending || data.integrationState === "configured"
            }
            onClick={() => planMutation.mutate()}
            type="button"
          >
            {planMutation.isPending
              ? "正在生成预览…"
              : data.integrationState === "configured"
                ? "配置已生效"
                : "配置 OpenCode"}
          </button>
          {data.integrationState === "configured" && (
            <button
              className="danger-button"
              disabled={disconnectPlanMutation.isPending}
              onClick={() => disconnectPlanMutation.mutate()}
              type="button"
            >
              {disconnectPlanMutation.isPending ? "正在生成预览…" : "断开接入"}
            </button>
          )}
        </div>
      </section>

      <PiCodingAgentIntegrationCard />

      <OpenClawIntegrationCard />

      <HermesAgentIntegrationCard />

      <details className="disclosure-card generic-access-disclosure">
        <summary>
          <div>
            <p className="eyebrow">通用接入</p>
            <strong>OpenAI / Anthropic 客户端</strong>
            <span>为其他软件创建独立 Key 与兼容端点</span>
          </div>
          <span className="disclosure-label">
            <span className="details-closed-copy">展开配置</span>
            <span className="details-open-copy">收起配置</span>
            <ChevronRight size={14} />
          </span>
        </summary>
        <GenericClientAccess />
      </details>

      {plan && (
        <ManagedAgentConfigurationDialog
          applying={applyMutation.isPending}
          displayName="OpenCode"
          error={applyMutation.isError ? errorMessage(applyMutation.error) : null}
          onApply={() => applyMutation.mutate(plan.planId)}
          onCancel={() => {
            if (!applyMutation.isPending) {
              void discardOpenCodeConfigurationPlan(plan.planId);
              setPlan(null);
            }
          }}
          plan={plan}
        />
      )}
      {disconnectPlan && (
        <ManagedAgentDisconnectDialog
          applying={disconnectMutation.isPending}
          displayName="OpenCode"
          error={disconnectMutation.isError ? errorMessage(disconnectMutation.error) : null}
          onApply={() => disconnectMutation.mutate(disconnectPlan.planId)}
          onCancel={() => {
            if (!disconnectMutation.isPending) {
              void discardOpenCodeDisconnectionPlan(disconnectPlan.planId);
              setDisconnectPlan(null);
            }
          }}
          plan={disconnectPlan}
        />
      )}
    </div>
  );
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
  apiRoot: "http://127.0.0.1:8000/v1/",
  authMethod: "none",
  apiKey: null,
};

function BackendsPage() {
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
  const [selectedModelId, setSelectedModelId] = useState("");
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
      <header className="page-header model-page-header">
        <div>
          <p className="eyebrow">本地推理</p>
          <h1>推理后端</h1>
          <p>HAL100 托管固定版本的 llama.cpp，并把当前模型接入本地 Gateway。</p>
        </div>
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
      </header>

      {operationError && <p className="inline-error">{errorMessage(operationError)}</p>}

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
          <div className="engine-details">
            <div>
              <span>运行状态</span>
              <strong>{runtimeCopy[data.runtimeState]}</strong>
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
          <div className="engine-actions">
            {data.installState === "notInstalled" ? (
              <button
                className="primary-button"
                disabled={installPlanMutation.isPending}
                onClick={() => installPlanMutation.mutate()}
                type="button"
              >
                <Download size={14} />
                {installPlanMutation.isPending ? "正在生成计划…" : "安装 llama.cpp"}
              </button>
            ) : (
              <button
                className="secondary-button"
                disabled={removePlanMutation.isPending}
                onClick={() => removePlanMutation.mutate()}
                type="button"
              >
                <Trash2 size={14} />
                {removePlanMutation.isPending ? "正在检查…" : "卸载引擎"}
              </button>
            )}
          </div>
        </section>

        <section className="runtime-card">
          <div>
            <p className="eyebrow">模型运行时</p>
            <h2>启动或切换模型</h2>
            <p>只列出已通过本地状态检查的 GGUF；切换时会停止当前 llama-server 再加载新模型。</p>
          </div>
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
            <button
              className="secondary-button"
              disabled={
                data.runtimeState === "stopped" ||
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
          </div>
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
                  data.runtimeState === "stopped" ||
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
        </section>
      </div>

      <details className="routing-card disclosure-card">
        <summary className="routing-heading">
          <div>
            <p className="eyebrow">高级配置</p>
            <h2>Gateway 路由与模型别名</h2>
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
              {backendCatalog.data.backends.filter((backend) => backend.runtimeAvailable).length}
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
                <option disabled={!backend.runtimeAvailable} key={backend.id} value={backend.id}>
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

      <section className="external-backends-card" aria-labelledby="external-backends-title">
        <div className="routing-heading">
          <div>
            <p className="eyebrow">外部推理服务</p>
            <h2 id="external-backends-title">已配置后端</h2>
          </div>
          <div className="backend-heading-actions">
            <button
              className="secondary-button"
              disabled={discoverBackendsMutation.isPending}
              onClick={() => discoverBackendsMutation.mutate()}
              type="button"
            >
              <Search size={13} />
              {discoverBackendsMutation.isPending ? "正在探测…" : "发现本机服务"}
            </button>
            <button
              className="secondary-button"
              onClick={() => setEditingBackend({ ...emptyBackendDraft })}
              type="button"
            >
              添加外部后端
            </button>
          </div>
        </div>
        {discoverBackendsMutation.data && (
          <section className="discovery-results" aria-label="本机后端发现结果">
            <div className="discovery-summary">
              <strong>
                已检查 {discoverBackendsMutation.data.checkedTargets} 个固定回环地址，发现{" "}
                {discoverBackendsMutation.data.candidates.length} 个候选
              </strong>
              <span>只按需检查 127.0.0.1 的常用端口，不扫描局域网。</span>
            </div>
            {discoverBackendsMutation.data.candidates.map((candidate) => (
              <div className="discovery-candidate" key={`${candidate.kind}-${candidate.apiRoot}`}>
                <div>
                  <strong>{candidate.displayName}</strong>
                  <code>{candidate.apiRoot}</code>
                  <small>
                    {candidate.evidence}
                    {candidate.version ? ` · ${candidate.version}` : ""}
                  </small>
                </div>
                <button
                  className="secondary-button compact-button"
                  onClick={() =>
                    setEditingBackend({
                      id: null,
                      displayName: candidate.displayName,
                      kind: candidate.kind,
                      apiRoot: candidate.apiRoot,
                      authMethod: "none",
                      apiKey: null,
                    })
                  }
                  type="button"
                >
                  使用此候选
                </button>
              </div>
            ))}
            {discoverBackendsMutation.data.candidates.length === 0 && (
              <p className="routing-empty">
                未发现免认证的本机候选；仍可使用“添加外部后端”手动配置。
              </p>
            )}
          </section>
        )}
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
                      <span>{backendKindLabels[backend.kind]}</span>
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
          <p className="routing-empty">
            尚未配置外部后端。可接入 OpenAI/Anthropic 兼容服务、Ollama、vLLM 或 llama.cpp Server。
          </p>
        )}
      </section>

      <section className="idle-cost-note">
        <ShieldCheck className="idle-cost-icon" size={16} />
        <p>未启动模型时没有 llama-server 进程；引擎状态只在打开本页或手动刷新时检查。</p>
      </section>

      {enginePlan && (
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
      {editingBackend && (
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

function formatRequestTime(timestampMs: number): string {
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(new Date(timestampMs));
}

function usageClientDisplayName(clientAppId: string, fallback: string): string {
  if (clientAppId === "hal100-agent-cloud") return "HAL100 Agent · 云端单次";
  if (clientAppId === "hal100-agent") return "HAL100 Agent · 本地";
  return fallback;
}

function formatCompactTokens(tokens: number): string {
  return new Intl.NumberFormat("zh-CN", {
    notation: "compact",
    maximumFractionDigits: 1,
  }).format(tokens);
}

function formatUsageChartTime(timestampMs: number): string {
  return new Intl.DateTimeFormat("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).format(new Date(timestampMs));
}

function requestTokenValue(request: UsageRequestSummary, field: "output" | "total"): number {
  if (field === "output") return request.outputTokens ?? 0;
  return request.totalTokens ?? (request.inputTokens ?? 0) + (request.outputTokens ?? 0);
}

function UsageTrendChart({ requests }: { requests: UsageRequestSummary[] }) {
  const points = [...requests].sort((a, b) => a.startedAtMs - b.startedAtMs).slice(-30);
  const width = 720;
  const height = 224;
  const plot = { left: 50, right: 14, top: 18, bottom: 32 };
  const plotWidth = width - plot.left - plot.right;
  const plotHeight = height - plot.top - plot.bottom;
  const baseline = plot.top + plotHeight;
  const peak = Math.max(1, ...points.map((point) => requestTokenValue(point, "total")));
  const pointX = (index: number) =>
    plot.left + (points.length === 1 ? plotWidth / 2 : (index / (points.length - 1)) * plotWidth);
  const pointY = (value: number) => baseline - (value / peak) * plotHeight;
  const totalPoints = points
    .map((point, index) => `${pointX(index)},${pointY(requestTokenValue(point, "total"))}`)
    .join(" ");
  const outputPoints = points
    .map((point, index) => `${pointX(index)},${pointY(requestTokenValue(point, "output"))}`)
    .join(" ");
  const areaPath =
    points.length > 0
      ? `M ${pointX(0)} ${baseline} L ${totalPoints.replaceAll(",", " ")} L ${pointX(points.length - 1)} ${baseline} Z`
      : "";
  const xLabelIndexes = points.length
    ? [...new Set([0, Math.floor((points.length - 1) / 2), points.length - 1])]
    : [];

  return (
    <article className="usage-chart-card usage-trend-card">
      <div className="usage-chart-heading">
        <div>
          <p className="eyebrow">最近请求</p>
          <h2>用量趋势</h2>
        </div>
        <ul className="usage-chart-legend" aria-label="图例">
          <li className="total">总 Token</li>
          <li className="output">输出</li>
        </ul>
      </div>
      {points.length === 0 ? (
        <div className="usage-chart-empty">
          <ChartNoAxesCombined size={20} />
          <span>有请求后显示最近 30 条用量趋势</span>
        </div>
      ) : (
        <svg
          aria-labelledby="usage-trend-title usage-trend-description"
          className="usage-trend-chart"
          role="img"
          viewBox={`0 0 ${width} ${height}`}
        >
          <title id="usage-trend-title">最近请求 Token 用量趋势</title>
          <desc id="usage-trend-description">
            展示最近 {points.length} 条请求的总 Token 与输出 Token，不进行后台实时刷新。
          </desc>
          <defs>
            <linearGradient id="usage-area-gradient" x1="0" x2="0" y1="0" y2="1">
              <stop offset="0%" stopColor="var(--accent)" stopOpacity="0.22" />
              <stop offset="100%" stopColor="var(--accent)" stopOpacity="0.01" />
            </linearGradient>
          </defs>
          {[1, 0.5, 0].map((ratio) => {
            const y = baseline - ratio * plotHeight;
            return (
              <g className="usage-chart-grid" key={ratio}>
                <line x1={plot.left} x2={width - plot.right} y1={y} y2={y} />
                <text x={plot.left - 10} y={y + 3}>
                  {formatCompactTokens(Math.round(peak * ratio))}
                </text>
              </g>
            );
          })}
          <path className="usage-chart-area" d={areaPath} />
          <polyline className="usage-chart-line total" points={totalPoints} />
          <polyline className="usage-chart-line output" points={outputPoints} />
          {xLabelIndexes.map((index) => (
            <text
              className="usage-chart-x-label"
              key={points[index].requestId}
              textAnchor={index === 0 ? "start" : index === points.length - 1 ? "end" : "middle"}
              x={pointX(index)}
              y={height - 8}
            >
              {formatUsageChartTime(points[index].startedAtMs)}
            </text>
          ))}
        </svg>
      )}
      <p className="usage-chart-caption">按单次请求绘制，最多读取当前页面已有的 30 条记录。</p>
    </article>
  );
}

function UsageCompositionCard({
  requests,
  totals,
}: {
  requests: UsageRequestSummary[];
  totals: UsageTotals;
}) {
  const cachedTokens = Math.min(totals.cachedTokens, totals.inputTokens);
  const segments = [
    { label: "非缓存输入", value: Math.max(0, totals.inputTokens - cachedTokens), tone: "input" },
    { label: "缓存输入", value: cachedTokens, tone: "cached" },
    { label: "输出", value: totals.outputTokens, tone: "output" },
  ];
  const segmentTotal = Math.max(
    1,
    segments.reduce((sum, segment) => sum + segment.value, 0),
  );
  let segmentOffset = 0;
  const clientTotals = new Map<string, number>();
  for (const request of requests) {
    const client = usageClientDisplayName(request.clientAppId, request.clientDisplayName);
    clientTotals.set(client, (clientTotals.get(client) ?? 0) + requestTokenValue(request, "total"));
  }
  const clients = [...clientTotals.entries()]
    .sort((a, b) => b[1] - a[1])
    .slice(0, 3)
    .map(([label, value]) => ({ label, value }));
  const recentTotal = Math.max(
    1,
    clients.reduce((sum, client) => sum + client.value, 0),
  );

  return (
    <article className="usage-chart-card usage-composition-card">
      <div className="usage-chart-heading">
        <div>
          <p className="eyebrow">累计构成</p>
          <h2>Token 构成</h2>
        </div>
      </div>
      <div className="usage-composition-main">
        <svg
          aria-label="累计 Token 构成环形图"
          className="usage-donut"
          role="img"
          viewBox="0 0 120 120"
        >
          <circle className="usage-donut-track" cx="60" cy="60" fill="none" r="43" />
          <g transform="rotate(-90 60 60)">
            {segments.map((segment) => {
              const share = (segment.value / segmentTotal) * 100;
              const offset = segmentOffset;
              segmentOffset += share;
              return (
                <circle
                  className={`usage-donut-segment ${segment.tone}`}
                  cx="60"
                  cy="60"
                  fill="none"
                  key={segment.label}
                  pathLength="100"
                  r="43"
                  strokeDasharray={`${share} ${100 - share}`}
                  strokeDashoffset={-offset}
                />
              );
            })}
          </g>
          <text className="usage-donut-value" textAnchor="middle" x="60" y="58">
            {formatCompactTokens(totals.totalTokens)}
          </text>
          <text className="usage-donut-label" textAnchor="middle" x="60" y="74">
            Token
          </text>
        </svg>
        <div className="usage-composition-legend">
          {segments.map((segment) => (
            <div key={segment.label}>
              <span className={`usage-legend-dot ${segment.tone}`} />
              <span>{segment.label}</span>
              <strong>{formatTokens(segment.value)}</strong>
            </div>
          ))}
        </div>
      </div>
      <div className="usage-client-breakdown">
        <span>最近请求来源</span>
        {clients.length === 0 ? (
          <p>暂无客户端记录</p>
        ) : (
          clients.map((client) => (
            <div key={client.label}>
              <div>
                <strong>{client.label}</strong>
                <span>{formatCompactTokens(client.value)}</span>
              </div>
              <i aria-hidden="true">
                <span style={{ width: `${Math.max(3, (client.value / recentTotal) * 100)}%` }} />
              </i>
            </div>
          ))
        )}
      </div>
    </article>
  );
}

function UsagePage() {
  const usage = useQuery({
    queryKey: ["usage-dashboard"],
    queryFn: getUsageDashboard,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });

  if (usage.isPending) {
    return <div className="state-message">正在读取本机 Token 统计…</div>;
  }
  if (usage.isError) {
    return <div className="state-message error">{errorMessage(usage.error)}</div>;
  }

  const data = usage.data;
  return (
    <div className="page-content usage-page">
      <header className="page-header model-page-header">
        <div>
          <p className="eyebrow">Gateway 精确计量</p>
          <h1>Token 统计</h1>
          <p>汇总所有通过 HAL100 Gateway 的请求；Token 数直接采用推理后端返回的 usage。</p>
        </div>
        <button
          className="secondary-button refresh-button"
          disabled={usage.isFetching}
          onClick={() => usage.refetch()}
          type="button"
        >
          <RefreshCw className={usage.isFetching ? "spinning" : ""} size={14} />
          {usage.isFetching ? "刷新中…" : "刷新统计"}
        </button>
      </header>

      <section className="usage-overview" aria-label="Token 汇总">
        <article className="usage-highlight-card">
          <div>
            <span>累计精确用量</span>
            <strong>{formatTokens(data.totals.totalTokens)}</strong>
            <small>总 Token · 不会按字符数估算</small>
          </div>
          <div className="usage-request-count">
            <strong>{formatTokens(data.totals.requestCount)}</strong>
            <span>请求数</span>
          </div>
        </article>
        <div className="usage-kpi-rail">
          <article>
            <span>输入</span>
            <strong>{formatTokens(data.totals.inputTokens)}</strong>
            <small>包含缓存输入</small>
          </article>
          <article>
            <span>缓存命中</span>
            <strong>{formatTokens(data.totals.cachedTokens)}</strong>
            <small>不重复计入总量</small>
          </article>
          <article>
            <span>输出</span>
            <strong>{formatTokens(data.totals.outputTokens)}</strong>
            <small>后端响应精确值</small>
          </article>
        </div>
      </section>

      <section className="usage-visual-grid" aria-label="用量可视化">
        <UsageTrendChart requests={data.recentRequests} />
        <UsageCompositionCard requests={data.recentRequests} totals={data.totals} />
      </section>

      <details
        className="usage-requests-card disclosure-card"
        open={data.recentRequests.length === 0 ? true : undefined}
      >
        <summary className="usage-section-heading">
          <div>
            <p className="eyebrow">最近请求</p>
            <h2>请求明细</h2>
          </div>
          <span className="disclosure-label">
            {data.recentRequests.length === 0 ? (
              "暂无记录"
            ) : (
              <>
                <span className="details-closed-copy">
                  {data.recentRequests.length} 条 · 展开查看
                </span>
                <span className="details-open-copy">
                  {data.recentRequests.length} 条 · 收起明细
                </span>
              </>
            )}
            <ChevronRight size={14} />
          </span>
        </summary>
        {data.recentRequests.length === 0 ? (
          <div className="usage-empty-state">
            <ChartNoAxesCombined className="usage-empty-icon" size={22} />
            <strong>尚无 Token 用量记录</strong>
            <span>启动模型并从 OpenCode 或“测试模型”发起一次请求后，这里会出现记录。</span>
          </div>
        ) : (
          <div className="usage-table-scroll">
            <table className="usage-table">
              <thead>
                <tr>
                  <th>时间 / 客户端</th>
                  <th>模型</th>
                  <th>输入</th>
                  <th>缓存</th>
                  <th>输出</th>
                  <th>总计</th>
                  <th>状态</th>
                </tr>
              </thead>
              <tbody>
                {data.recentRequests.map((request) => (
                  <tr key={request.requestId}>
                    <td>
                      <strong>
                        {usageClientDisplayName(request.clientAppId, request.clientDisplayName)}
                      </strong>
                      <span>{formatRequestTime(request.startedAtMs)}</span>
                    </td>
                    <td>
                      <strong>{request.resolvedModel}</strong>
                      <span>{request.backendId}</span>
                    </td>
                    <td>{formatTokens(request.inputTokens)}</td>
                    <td>{formatTokens(request.cachedTokens)}</td>
                    <td>{formatTokens(request.outputTokens)}</td>
                    <td>{formatTokens(request.totalTokens)}</td>
                    <td>
                      <span className={`usage-status ${request.status}`}>
                        {request.status === "succeeded" ? "成功" : "失败"}
                      </span>
                      <small>
                        {request.usageAccuracy === "exact_backend_response"
                          ? "响应精确值"
                          : request.usageAccuracy === "exact_backend_event"
                            ? "事件精确值"
                            : "不可用"}
                      </small>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </details>
      <section className="idle-cost-note">
        <Database className="idle-cost-icon" size={16} />
        <p>统计页不轮询；用量由后台异步批量写入 SQLite WAL，只在打开本页或手动刷新时查询。</p>
      </section>
    </div>
  );
}

function ModelTestPage() {
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
    <div className="page-content model-test-page">
      <header className="page-header">
        <div>
          <p className="eyebrow">单轮验证</p>
          <h1>测试模型</h1>
          <p>向当前模型发送一次非流式请求，验证 llama.cpp、Gateway、鉴权和 Token 计量闭环。</p>
        </div>
        <span className={`status-pill ${runtimeReady ? "ok" : "neutral"}`}>
          {runtimeReady ? `运行中 · ${engine.activeModelName ?? "当前模型"}` : "尚未启动模型"}
        </span>
      </header>

      <section className="model-test-grid">
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

const auditEventLabels: Record<string, string> = {
  agent_action_discarded: "Agent 计划已失效",
  agent_action_executed: "Agent 计划已执行",
  agent_action_failed: "Agent 计划执行失败",
  agent_action_recheck_failed: "Agent 操作后复检失败",
  agent_action_planned: "Agent 生成操作计划",
  agent_cloud_session_started: "启用 Agent 云端会话",
  agent_cloud_session_stopped: "退出 Agent 云端会话",
  agent_run_cancelled: "Agent 任务取消",
  agent_run_completed: "Agent 任务完成",
  agent_run_failed: "Agent 任务失败",
  agent_run_started: "Agent 任务开始",
  agent_runtime_started: "启动 Agent 模型",
  agent_runtime_stopped: "停止 Agent 模型",
  data_retention_applied: "清理历史数据",
  engine_installed: "安装推理引擎",
  engine_removed: "卸载推理引擎",
  generic_client_created: "签发客户端 Key",
  generic_client_revoked: "撤销客户端 Key",
  launch_at_login_changed: "修改登录启动",
  model_downloaded: "下载模型",
  model_imported: "导入模型",
  onboarding_completed: "完成首次设置",
  retention_settings_changed: "修改保留策略",
};

const auditDetailLabels: Record<string, string> = {
  action: "操作",
  alias: "模型别名",
  auditDeleted: "删除审计记录",
  auditRetentionDays: "审计保留天数",
  backendId: "后端",
  displayName: "名称",
  enabled: "是否启用",
  engine: "引擎",
  errorCode: "错误代码",
  fileName: "文件",
  format: "格式",
  model: "模型",
  modelId: "模型 ID",
  ownership: "所有权",
  repository: "仓库",
  reason: "原因",
  resolvedModel: "实际模型",
  revision: "修订",
  sizeBytes: "大小（字节）",
  source: "来源",
  toolCalls: "工具调用次数",
  toolPolicy: "工具策略",
  usageDeleted: "删除 Token 记录",
  usageRetentionDays: "Token 保留天数",
  version: "版本",
};

function AuditPage() {
  const [eventType, setEventType] = useState("all");
  const [search, setSearch] = useState("");
  const [selectedEventId, setSelectedEventId] = useState<string | null>(null);
  const audit = useQuery({
    queryKey: ["audit-log"],
    queryFn: getAuditLog,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });

  if (audit.isPending) {
    return <div className="state-message">正在读取本机审计记录…</div>;
  }
  if (audit.isError) {
    return <div className="state-message error">{errorMessage(audit.error)}</div>;
  }

  const normalizedSearch = search.trim().toLocaleLowerCase("zh-CN");
  const eventTypes = [...new Set(audit.data.events.map((event) => event.eventType))];
  const events = audit.data.events.filter((event) => {
    if (eventType !== "all" && event.eventType !== eventType) return false;
    if (!normalizedSearch) return true;
    return [
      event.targetId,
      auditEventLabels[event.eventType] ?? event.eventType,
      ...event.details.flatMap((detail) => [detail.key, detail.value]),
    ]
      .join(" ")
      .toLocaleLowerCase("zh-CN")
      .includes(normalizedSearch);
  });
  const selectedEvent = audit.data.events.find((event) => event.id === selectedEventId) ?? null;

  return (
    <div className="page-content audit-page">
      <header className="page-header model-page-header">
        <div>
          <p className="eyebrow">本机受控操作</p>
          <h1>审计记录</h1>
          <p>查看安装、卸载、模型获取、凭据和数据策略操作；不包含提示词、回答或 API Key。</p>
        </div>
        <button
          className="secondary-button refresh-button"
          disabled={audit.isFetching}
          onClick={() => audit.refetch()}
          type="button"
        >
          <RefreshCw className={audit.isFetching ? "spinning" : ""} size={14} />
          {audit.isFetching ? "刷新中…" : "刷新记录"}
        </button>
      </header>

      <section className="audit-toolbar" aria-label="审计筛选">
        <ListFilter size={16} />
        <label>
          <span>操作类型</span>
          <select value={eventType} onChange={(event) => setEventType(event.target.value)}>
            <option value="all">全部操作</option>
            {eventTypes.map((type) => (
              <option key={type} value={type}>
                {auditEventLabels[type] ?? type}
              </option>
            ))}
          </select>
        </label>
        <label className="audit-search">
          <span>搜索目标</span>
          <input
            onChange={(event) => setSearch(event.target.value)}
            placeholder="名称、目标或动作"
            type="search"
            value={search}
          />
        </label>
        <span className="audit-count">
          显示 {events.length} / {audit.data.totalCount}
        </span>
      </section>

      <section className="audit-log-card" aria-label="审计事件">
        {audit.data.events.length === 0 ? (
          <div className="usage-empty-state">
            <ScrollText className="usage-empty-icon" size={22} />
            <strong>尚无受控操作记录</strong>
            <span>完成一次模型下载、引擎安装或客户端 Key 签发后，这里会出现记录。</span>
          </div>
        ) : events.length === 0 ? (
          <div className="usage-empty-state">
            <Search className="usage-empty-icon" size={22} />
            <strong>没有匹配记录</strong>
            <span>调整操作类型或搜索词后重试。</span>
          </div>
        ) : (
          <div className="audit-list">
            {events.map((event) => (
              <button
                className={`audit-row${selectedEventId === event.id ? " selected" : ""}`}
                key={event.id}
                onClick={() => setSelectedEventId(event.id)}
                type="button"
              >
                <span className="audit-time">{formatRequestTime(event.createdAtMs)}</span>
                <strong>{auditEventLabels[event.eventType] ?? event.eventType}</strong>
                <span>
                  {event.details.find((detail) => detail.key === "displayName")?.value ??
                    event.targetId}
                </span>
                <small>已记录</small>
                <ChevronRight size={15} />
              </button>
            ))}
          </div>
        )}
      </section>
      <section className="idle-cost-note">
        <ShieldCheck className="idle-cost-icon" size={16} />
        <p>审计页不轮询；详情仅返回固定白名单字段，路径、凭据、提示词和回答不会进入页面。</p>
      </section>

      {selectedEvent && (
        <div className="dialog-backdrop audit-detail-backdrop" role="presentation">
          <section
            aria-labelledby="audit-detail-title"
            aria-modal="true"
            className="dialog audit-detail-dialog"
            role="dialog"
          >
            <div className="dialog-heading">
              <div>
                <p className="eyebrow">审计详情</p>
                <h2 id="audit-detail-title">
                  {auditEventLabels[selectedEvent.eventType] ?? selectedEvent.eventType}
                </h2>
              </div>
              <button
                aria-label="关闭审计详情"
                className="icon-button"
                onClick={() => setSelectedEventId(null)}
                type="button"
              >
                <X size={17} />
              </button>
            </div>
            <dl className="audit-detail-list">
              <div>
                <dt>时间</dt>
                <dd>{formatRequestTime(selectedEvent.createdAtMs)}</dd>
              </div>
              <div>
                <dt>目标类型</dt>
                <dd>{selectedEvent.targetType}</dd>
              </div>
              <div>
                <dt>目标标识</dt>
                <dd>{selectedEvent.targetId}</dd>
              </div>
              {selectedEvent.details.map((detail) => (
                <div key={detail.key}>
                  <dt>{auditDetailLabels[detail.key] ?? detail.key}</dt>
                  <dd>{detail.value}</dd>
                </div>
              ))}
            </dl>
            <p className="inline-notice">该详情已经由 Rust Core 按字段白名单脱敏。</p>
          </section>
        </div>
      )}
    </div>
  );
}

function retentionOption(value: string): number | null {
  return value === "forever" ? null : Number(value);
}

function retentionSelectValue(value: number | null): string {
  return value == null ? "forever" : String(value);
}

function SettingsPage({
  darkMode,
  onToggleTheme,
}: {
  darkMode: boolean;
  onToggleTheme: () => void;
}) {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const settings = useQuery({
    queryKey: ["desktop-settings"],
    queryFn: getDesktopSettings,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  const cleanupPreview = useQuery({
    queryKey: ["data-cleanup-preview"],
    queryFn: getDataCleanupPreview,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  const modelLibrary = useQuery({
    queryKey: ["model-library"],
    queryFn: getModelLibrary,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  const hardware = useQuery({
    queryKey: ["hardware-profile"],
    queryFn: getHardwareProfile,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  const engine = useQuery({
    queryKey: ["llama-cpp-status"],
    queryFn: getLlamaCppStatus,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  const openCode = useQuery({
    queryKey: ["opencode-detection"],
    queryFn: getOpenCodeDetection,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  const [usageRetentionDays, setUsageRetentionDays] = useState<number | null>(90);
  const [auditRetentionDays, setAuditRetentionDays] = useState<number | null>(365);
  const [setupLaunchChoice, setSetupLaunchChoice] = useState<boolean | null>(null);

  useEffect(() => {
    if (settings.data) {
      setUsageRetentionDays(settings.data.usageRetentionDays);
      setAuditRetentionDays(settings.data.auditRetentionDays);
    }
  }, [settings.data]);

  const launchMutation = useMutation({
    mutationFn: setLaunchAtLogin,
    onSuccess: (nextSettings) => {
      queryClient.setQueryData(["desktop-settings"], nextSettings);
      queryClient.invalidateQueries({ queryKey: ["audit-log"] });
    },
  });
  const sourceMutation = useMutation({
    mutationFn: setDefaultDownloadSource,
    onSuccess: (nextLibrary) => {
      queryClient.setQueryData(["model-library"], nextLibrary);
    },
  });
  const completionMutation = useMutation({
    mutationFn: (launchAtLogin: boolean) => completeOnboarding({ launchAtLogin }),
    onSuccess: (nextSettings) => {
      queryClient.setQueryData(["desktop-settings"], nextSettings);
      queryClient.invalidateQueries({ queryKey: ["audit-log"] });
    },
  });
  const retentionMutation = useMutation({
    mutationFn: (draft: RetentionSettingsDraft) => updateRetentionSettings(draft),
    onSuccess: (draft) => {
      queryClient.setQueryData<DesktopSettings>(["desktop-settings"], (current) =>
        current
          ? {
              ...current,
              usageRetentionDays: draft.usageRetentionDays,
              auditRetentionDays: draft.auditRetentionDays,
            }
          : current,
      );
      queryClient.invalidateQueries({ queryKey: ["data-cleanup-preview"] });
      queryClient.invalidateQueries({ queryKey: ["audit-log"] });
    },
  });
  const cleanupMutation = useMutation({
    mutationFn: applyDataRetention,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["data-cleanup-preview"] });
      queryClient.invalidateQueries({ queryKey: ["usage-dashboard"] });
      queryClient.invalidateQueries({ queryKey: ["audit-log"] });
    },
  });

  useEffect(() => {
    if (completionMutation.isSuccess && settings.data?.onboardingCompleted) {
      navigate("/", { replace: true });
    }
  }, [completionMutation.isSuccess, navigate, settings.data?.onboardingCompleted]);

  if (settings.isPending) {
    return <div className="state-message">正在读取桌面设置…</div>;
  }
  if (settings.isError) {
    return <div className="state-message error">{errorMessage(settings.error)}</div>;
  }

  const data = settings.data;
  const setupRequired = !data.onboardingCompleted;
  const selectedSource = modelLibrary.data?.defaultDownloadSource ?? null;
  const sourceReady = selectedSource !== null;
  const launchReady = setupRequired ? setupLaunchChoice !== null : true;
  const completedSetupItems = Number(sourceReady) + Number(launchReady);
  const setupProgress = completedSetupItems * 50;
  const readyModelCount =
    modelLibrary.data?.models.filter((model) => model.state === "ready").length ?? 0;
  const engineInstalled = engine.data?.installState === "installed";
  const openCodeConfigured = openCode.data?.integrationState === "configured";
  const retentionChanged =
    usageRetentionDays !== data.usageRetentionDays ||
    auditRetentionDays !== data.auditRetentionDays;
  const preview = cleanupPreview.data;
  const cleanupCount = (preview?.usageRequestCount ?? 0) + (preview?.auditEventCount ?? 0);

  return (
    <div className="page-content settings-page">
      <header className="page-header">
        <div>
          <p className="eyebrow">全局设置</p>
          <h1>设置</h1>
          <p>集中管理 HAL100 的基础偏好、本机环境和数据策略；每项都可以随时返回调整。</p>
        </div>
        <span className={`settings-readiness ${setupRequired ? "attention" : "ready"}`}>
          <span />
          {setupRequired ? "需要完成基础设置" : "基础设置已完成"}
        </span>
      </header>

      <section className={`setup-center compact${setupRequired ? " is-first-run" : ""}`}>
        <div className="setup-center-heading">
          <div>
            <p className="eyebrow">{setupRequired ? "首次使用" : "配置概览"}</p>
            <h2>初始化配置中心</h2>
            <p>
              {setupRequired
                ? "完成两项基础选择即可；其余功能按需配置。"
                : "调整基础偏好，快速查看本机准备状态。"}
            </p>
          </div>
          <div
            aria-label={`基础设置完成 ${completedSetupItems} / 2 项`}
            className="setup-progress-summary"
            role="status"
          >
            <strong>{completedSetupItems} / 2</strong>
            <span>基础项</span>
          </div>
        </div>

        <div className="setup-progress-track" aria-hidden="true">
          <span style={{ width: `${setupProgress}%` }} />
        </div>

        <div className="setup-essential-grid">
          <div className="setup-essential-item">
            <div className="setup-essential-heading">
              <div>
                <strong>模型下载源</strong>
                <span>搜索时仍可临时切换</span>
              </div>
              <span className={`status-pill ${sourceReady ? "ok" : "warning"}`}>
                {sourceReady ? "已选择" : "必选"}
              </span>
            </div>
            <fieldset aria-label="默认模型下载源" className="source-selector setup-source-selector">
              <button
                aria-pressed={selectedSource === "huggingFace"}
                disabled={sourceMutation.isPending}
                onClick={() => sourceMutation.mutate("huggingFace")}
                type="button"
              >
                Hugging Face
              </button>
              <button
                aria-pressed={selectedSource === "modelScope"}
                disabled={sourceMutation.isPending}
                onClick={() => sourceMutation.mutate("modelScope")}
                type="button"
              >
                ModelScope
              </button>
            </fieldset>
          </div>

          <div className="setup-essential-item">
            <div className="setup-essential-heading">
              <div>
                <strong>随系统登录启动</strong>
                <span>推荐保持关闭</span>
              </div>
              <span className={`status-pill ${launchReady ? "ok" : "warning"}`}>
                {launchReady ? "已选择" : "必选"}
              </span>
            </div>
            {setupRequired ? (
              <fieldset aria-label="随系统登录启动偏好" className="setup-choice-group">
                <button
                  aria-pressed={setupLaunchChoice === false}
                  onClick={() => setSetupLaunchChoice(false)}
                  type="button"
                >
                  保持关闭
                </button>
                <button
                  aria-pressed={setupLaunchChoice === true}
                  onClick={() => setSetupLaunchChoice(true)}
                  type="button"
                >
                  登录时启动
                </button>
              </fieldset>
            ) : (
              <button
                aria-pressed={data.launchAtLoginEnabled}
                className={`toggle-button${data.launchAtLoginEnabled ? " enabled" : ""}`}
                disabled={launchMutation.isPending || !isTauriRuntime()}
                onClick={() => launchMutation.mutate(!data.launchAtLoginEnabled)}
                type="button"
              >
                <span />
                {data.launchAtLoginEnabled ? "已开启" : "已关闭"}
              </button>
            )}
          </div>
        </div>

        <section className="setup-overview-strip" aria-label="环境准备状态">
          <article>
            <span className="setup-overview-icon">
              <Cpu size={15} />
            </span>
            <div>
              <small>这台 Mac</small>
              <strong>
                {hardware.isPending
                  ? "检测中"
                  : hardware.data
                    ? `${hardware.data.chip} · ${formatBytes(hardware.data.totalUnifiedMemoryBytes)}`
                    : "检测失败"}
              </strong>
            </div>
            <button
              aria-label="重新检测设备"
              className="icon-button"
              disabled={hardware.isFetching}
              onClick={() => hardware.refetch()}
              type="button"
            >
              <RefreshCw className={hardware.isFetching ? "spinning" : ""} size={14} />
            </button>
          </article>
          <Link to="/backends">
            <span className="setup-overview-icon">
              <HardDrive size={15} />
            </span>
            <div>
              <small>本地推理</small>
              <strong>
                {engineInstalled ? `llama.cpp · ${readyModelCount} 个模型` : "尚未安装引擎"}
              </strong>
            </div>
            <ChevronRight size={14} />
          </Link>
          <Link to="/integrations">
            <span className="setup-overview-icon">
              <Cable size={15} />
            </span>
            <div>
              <small>软件接入</small>
              <strong>{openCodeConfigured ? "OpenCode 已接入" : "尚未配置"}</strong>
            </div>
            <ChevronRight size={14} />
          </Link>
        </section>

        {(sourceMutation.isError || launchMutation.isError) && (
          <p className="inline-error">
            {errorMessage(sourceMutation.error ?? launchMutation.error)}
          </p>
        )}

        {setupRequired && (
          <div className="setup-completion-bar compact">
            <p>
              <strong>
                {sourceReady && launchReady ? "可以完成基础设置" : "请完成两项必选设置"}
              </strong>
              <span>模型、后端和软件接入可稍后处理。</span>
            </p>
            <button
              className="primary-button"
              disabled={!sourceReady || !launchReady || completionMutation.isPending}
              onClick={() => completionMutation.mutate(setupLaunchChoice ?? false)}
              type="button"
            >
              {completionMutation.isPending ? "正在保存…" : "完成设置"}
              {!completionMutation.isPending && <ChevronRight size={14} />}
            </button>
          </div>
        )}
        {completionMutation.isError && (
          <p className="inline-error">{errorMessage(completionMutation.error)}</p>
        )}
      </section>
      <section className="settings-card">
        <div className="settings-card-heading">
          <div>
            <p className="eyebrow">通用</p>
            <h2>外观与窗口行为</h2>
          </div>
          <span className="status-pill neutral">标准模式</span>
        </div>
        <div className="settings-row">
          <div>
            <strong>界面外观</strong>
            <span>当前使用{darkMode ? "深色" : "浅色"}外观；选择保存在本机 WebView。</span>
          </div>
          <button className="secondary-button" onClick={onToggleTheme} type="button">
            {darkMode ? <Sun size={14} /> : <Moon size={14} />}
            切换为{darkMode ? "浅色" : "深色"}
          </button>
        </div>
        <div className="settings-row static-setting">
          <div>
            <strong>关闭窗口时</strong>
            <span>{data.closeBehavior}；只有托盘“退出 HAL100”才结束后台核心。</span>
          </div>
          <span className="status-pill ok">固定安全行为</span>
        </div>
      </section>

      <section className="settings-card">
        <div className="settings-card-heading">
          <div>
            <p className="eyebrow">Token 与数据</p>
            <h2>本机保留策略</h2>
          </div>
          <span>默认 Token 90 天 · 审计 365 天</span>
        </div>
        <div className="retention-grid">
          <label>
            <span>Token 请求记录</span>
            <select
              onChange={(event) => setUsageRetentionDays(retentionOption(event.target.value))}
              value={retentionSelectValue(usageRetentionDays)}
            >
              <option value="30">30 天</option>
              <option value="90">90 天</option>
              <option value="180">180 天</option>
              <option value="365">365 天</option>
              <option value="forever">永久保留</option>
            </select>
          </label>
          <label>
            <span>审计记录</span>
            <select
              onChange={(event) => setAuditRetentionDays(retentionOption(event.target.value))}
              value={retentionSelectValue(auditRetentionDays)}
            >
              <option value="30">30 天</option>
              <option value="90">90 天</option>
              <option value="180">180 天</option>
              <option value="365">365 天</option>
              <option value="forever">永久保留</option>
            </select>
          </label>
        </div>
        <div className="settings-actions-row">
          <div>
            <strong>当前可清理 {cleanupPreview.isPending ? "…" : `${cleanupCount} 条`}</strong>
            <span>保存策略不会立即删除；“按策略清理”才会显示原生确认并执行。</span>
          </div>
          <button
            className="secondary-button"
            disabled={!retentionChanged || retentionMutation.isPending || !isTauriRuntime()}
            onClick={() => retentionMutation.mutate({ usageRetentionDays, auditRetentionDays })}
            type="button"
          >
            {retentionMutation.isPending ? "保存中…" : "保存保留策略"}
          </button>
          <button
            className="danger-button"
            disabled={
              cleanupPreview.isPending ||
              cleanupCount === 0 ||
              cleanupMutation.isPending ||
              !isTauriRuntime()
            }
            onClick={() => cleanupMutation.mutate()}
            type="button"
          >
            {cleanupMutation.isPending ? "正在清理…" : "按策略清理"}
          </button>
        </div>
        {retentionMutation.isError && (
          <p className="inline-error">{errorMessage(retentionMutation.error)}</p>
        )}
        {cleanupMutation.isError && (
          <p className="inline-error">{errorMessage(cleanupMutation.error)}</p>
        )}
        {cleanupMutation.data && (
          <p className="inline-success">
            已删除 {cleanupMutation.data.usageRequestsDeleted} 条 Token 请求记录和{" "}
            {cleanupMutation.data.auditEventsDeleted} 条审计记录。
          </p>
        )}
      </section>

      {!isTauriRuntime() && (
        <section className="idle-cost-note">
          <ShieldCheck className="idle-cost-icon" size={16} />
          <p>浏览器预览可体验配置流程，但不会修改系统登录项、SQLite 或执行数据清理。</p>
        </section>
      )}
    </div>
  );
}

function PlaceholderPage({ title }: { title: string }) {
  return (
    <div className="page-content placeholder-page">
      <p className="eyebrow">模块边界已建立</p>
      <h1>{title}</h1>
      <p>该模块的界面规范已经确认，业务能力将在对应迭代中接入 Rust Core。</p>
      <div className="placeholder-card">
        <span>当前状态</span>
        <strong>等待纵向功能实现</strong>
        <small>不会使用无效按钮或模拟系统操作代替真实能力。</small>
      </div>
    </div>
  );
}

function getInitialDarkMode(): boolean {
  const storedTheme = window.localStorage.getItem("hal100-theme");
  if (storedTheme === "dark") return true;
  if (storedTheme === "light") return false;
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false;
}

export default function App() {
  const [darkMode, setDarkMode] = useState(getInitialDarkMode);
  const desktopSettings = useQuery({
    queryKey: ["desktop-settings"],
    queryFn: getDesktopSettings,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });

  useEffect(() => {
    document.documentElement.dataset.theme = darkMode ? "dark" : "light";
    window.localStorage.setItem("hal100-theme", darkMode ? "dark" : "light");
  }, [darkMode]);

  if (desktopSettings.isPending) {
    return <div className="app-bootstrap-state">正在连接 HAL100 Core…</div>;
  }
  if (desktopSettings.isError) {
    return <div className="app-bootstrap-state error">{errorMessage(desktopSettings.error)}</div>;
  }
  return (
    <div className="app-shell">
      <Sidebar
        darkMode={darkMode}
        onToggleTheme={() => setDarkMode((value) => !value)}
        setupRequired={!desktopSettings.data.onboardingCompleted}
      />
      <main className="main-area">
        <div className="window-drag-region" data-tauri-drag-region aria-hidden="true" />
        <Routes>
          <Route
            path="/"
            element={<OverviewPage setupRequired={!desktopSettings.data.onboardingCompleted} />}
          />
          <Route path="/models" element={<ModelsPage />} />
          <Route path="/backends" element={<BackendsPage />} />
          <Route path="/integrations" element={<IntegrationsPage />} />
          <Route path="/usage" element={<UsagePage />} />
          <Route path="/agent" element={<AgentPage />} />
          <Route path="/test" element={<ModelTestPage />} />
          <Route path="/audit" element={<AuditPage />} />
          {navigation
            .slice(1)
            .filter(
              (item) =>
                item.path !== "/integrations" &&
                item.path !== "/models" &&
                item.path !== "/backends" &&
                item.path !== "/usage" &&
                item.path !== "/agent" &&
                item.path !== "/test" &&
                item.path !== "/audit",
            )
            .map((item) => (
              <Route
                key={item.path}
                path={item.path}
                element={<PlaceholderPage title={item.label} />}
              />
            ))}
          <Route
            path="/settings"
            element={
              <SettingsPage
                darkMode={darkMode}
                onToggleTheme={() => setDarkMode((value) => !value)}
              />
            }
          />
        </Routes>
      </main>
    </div>
  );
}
