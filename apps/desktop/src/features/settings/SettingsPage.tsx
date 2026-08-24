import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Cable,
  ChevronRight,
  Cpu,
  HardDrive,
  Moon,
  RefreshCw,
  ShieldCheck,
  Sun,
} from "lucide-react";
import { useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { PageHeader } from "../../components/ui/PageHeader";
import {
  applyDataRetention,
  completeOnboarding,
  type DesktopSettings,
  getAppOverview,
  getDataCleanupPreview,
  getDesktopSettings,
  getHardwareProfile,
  getLlamaCppStatus,
  getModelLibrary,
  getOpenCodeDetection,
  isTauriRuntime,
  type RetentionSettingsDraft,
  setDefaultDownloadSource,
  setLaunchAtLogin,
  updateRetentionSettings,
} from "../../lib/desktop-api";

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

function retentionOption(value: string): number | null {
  return value === "forever" ? null : Number(value);
}

function retentionSelectValue(value: number | null): string {
  return value == null ? "forever" : String(value);
}

export function SettingsPage({
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
  const overview = useQuery({
    queryKey: ["app-overview"],
    queryFn: getAppOverview,
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
    enabled: settings.data?.onboardingCompleted === false,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  const engine = useQuery({
    queryKey: ["llama-cpp-status"],
    queryFn: getLlamaCppStatus,
    enabled: settings.data?.onboardingCompleted === false,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  const openCode = useQuery({
    queryKey: ["opencode-detection"],
    queryFn: getOpenCodeDetection,
    enabled: settings.data?.onboardingCompleted === false,
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
      <PageHeader
        action={
          setupRequired ? (
            <span className="settings-readiness attention">
              <span />
              需要完成基础设置
            </span>
          ) : undefined
        }
        description="管理基础偏好、外观和本机数据策略。"
        eyebrow="全局设置"
        title="设置"
      />

      {setupRequired && (
        <section className="setup-center compact is-first-run">
          <div className="setup-center-heading">
            <div>
              <p className="eyebrow">首次使用</p>
              <h2>初始化配置中心</h2>
              <p>完成两项基础选择即可；其余功能按需配置。</p>
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
              <fieldset
                aria-label="默认模型下载源"
                className="source-selector setup-source-selector"
              >
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
            <Link to="/workspace/runtime">
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
          {completionMutation.isError && (
            <p className="inline-error">{errorMessage(completionMutation.error)}</p>
          )}
        </section>
      )}

      {!setupRequired && (
        <section className="settings-card">
          <div className="settings-card-heading">
            <div>
              <p className="eyebrow">基础偏好</p>
              <h2>下载与启动</h2>
            </div>
          </div>
          <div className="settings-row">
            <div>
              <strong>默认模型下载源</strong>
              <span>模型搜索时仍可临时切换来源。</span>
            </div>
            <fieldset
              aria-label="默认模型下载源"
              className="source-selector settings-source-selector"
            >
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
          <div className="settings-row">
            <div>
              <strong>随系统登录启动</strong>
              <span>关闭时仍可随时手动启动 HAL100。</span>
            </div>
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
          </div>
          {(sourceMutation.isError || launchMutation.isError) && (
            <p className="inline-error">
              {errorMessage(sourceMutation.error ?? launchMutation.error)}
            </p>
          )}
        </section>
      )}
      <section className="settings-card">
        <div className="settings-card-heading">
          <div>
            <p className="eyebrow">通用</p>
            <h2>界面外观</h2>
          </div>
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
      </section>

      <section className="settings-card">
        <div className="settings-card-heading">
          <div>
            <p className="eyebrow">Token 与数据</p>
            <h2>本机保留策略</h2>
          </div>
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

      <section className="settings-card">
        <div className="settings-card-heading">
          <div>
            <p className="eyebrow">关于</p>
            <h2>HAL100</h2>
          </div>
          <span className="status-pill neutral">开发版</span>
        </div>
        <div className="settings-row static-setting">
          <div>
            <strong>应用版本</strong>
            <span>当前本机运行的 HAL100 Desktop 版本。</span>
          </div>
          <strong>v{overview.data?.version ?? "—"}</strong>
        </div>
        <div className="settings-row static-setting">
          <div>
            <strong>运行平台</strong>
            <span>首版仅支持 Apple Silicon Mac。</span>
          </div>
          <span className={`status-pill ${overview.data?.platform.supported ? "ok" : "warning"}`}>
            {overview.data
              ? `${overview.data.platform.os} · ${overview.data.platform.architecture}`
              : "正在读取…"}
          </span>
        </div>
        <div className="settings-row static-setting">
          <div>
            <strong>关闭窗口时</strong>
            <span>{data.closeBehavior}；只有托盘“退出 HAL100”才结束后台核心。</span>
          </div>
          <span className="status-pill ok">固定安全行为</span>
        </div>
        <div className="settings-row">
          <div>
            <strong>帮助与环境诊断</strong>
            <span>检查模型、引擎、软件接入与系统权限准备情况。</span>
          </div>
          <Link className="secondary-button" to="/agent">
            打开 Agent 诊断
          </Link>
        </div>
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
