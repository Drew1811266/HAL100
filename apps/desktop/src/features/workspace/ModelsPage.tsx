import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  ChevronRight,
  Cpu,
  Database,
  Download,
  FileCheck2,
  FolderInput,
  FolderOpen,
  RotateCcw,
  Search,
  Trash2,
  X,
} from "lucide-react";
import { useEffect, useState } from "react";
import { NavLink } from "react-router-dom";
import { Drawer } from "../../components/ui/Drawer";
import { PageHeader } from "../../components/ui/PageHeader";
import {
  applyGgufImport,
  applyModelRemoval,
  cancelModelDownload,
  type DownloadSource,
  type GgufImportPlan,
  getHardwareProfile,
  getModelDownloads,
  getModelLibrary,
  getRemoteModelRepository,
  isTauriRuntime,
  type LocalModelSummary,
  type ModelDownloadPlan,
  type ModelDownloadSnapshot,
  type ModelRemovalPlan,
  planModelDownload,
  planModelRemoval,
  type RemoteModelRepository,
  resumeModelDownload,
  searchRemoteModels,
  selectAndPlanGgufImport,
  startModelDownload,
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

export function ModelsPage() {
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
  const [addModelOpen, setAddModelOpen] = useState(false);
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
      setAddModelOpen(false);
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
      <PageHeader
        action={
          <button
            className="primary-button refresh-button"
            onClick={() => setAddModelOpen(true)}
            type="button"
          >
            <FolderInput size={14} />
            添加模型
          </button>
        }
        className="model-page-header"
        description="本机可供 HAL100 使用的模型；下载、导入和设备建议在添加流程中查看。"
        eyebrow="本地模型"
        title="模型库"
      />
      <SectionTabs label="模型与运行" tabs={workspaceTabs} />

      {planRemovalMutation.isError && (
        <p className="inline-error model-page-error">{errorMessage(planRemovalMutation.error)}</p>
      )}

      {addModelOpen && (
        <Drawer
          description="从远程目录下载，或索引电脑中已有的 GGUF 文件。所有写操作仍会先显示确认计划。"
          eyebrow="模型任务"
          onClose={() => setAddModelOpen(false)}
          title="添加模型"
        >
          <section className="drawer-section model-import-section">
            <div>
              <h3>导入电脑中的 GGUF</h3>
              <p>HAL100 只建立索引，不会移动或复制你选择的源文件。</p>
            </div>
            <button
              className="secondary-button refresh-button"
              disabled={selectImportMutation.isPending}
              onClick={() => selectImportMutation.mutate()}
              type="button"
            >
              <FolderInput size={14} />
              {selectImportMutation.isPending ? "正在检查…" : "选择 GGUF 文件"}
            </button>
          </section>
          {selectImportMutation.isError && (
            <p className="inline-error">{errorMessage(selectImportMutation.error)}</p>
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

          <section className="remote-catalog-card">
            <div className="remote-catalog-heading">
              <div>
                <p className="eyebrow">远程目录</p>
                <h2>查找 GGUF 模型</h2>
                <p>输入模型名称或仓库地址，按需查询远程 GGUF 模型。</p>
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
                resumingId={
                  resumeDownloadMutation.isPending ? resumeDownloadMutation.variables : null
                }
              />
            )}
          </section>
        </Drawer>
      )}

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
            <p>使用“添加模型”从远程目录下载，或导入电脑中已有的 GGUF 文件。</p>
            <button className="primary-button" onClick={() => setAddModelOpen(true)} type="button">
              <FolderInput size={14} />
              添加第一个模型
            </button>
          </div>
        )}
        <div className="storage-path">
          <FolderOpen size={14} />
          <code>{modelLibrary.modelStoragePath}</code>
        </div>
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

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}
