import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { HardDrive, ShieldCheck } from "lucide-react";
import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { Drawer } from "../../components/ui/Drawer";
import { PageHeader } from "../../components/ui/PageHeader";
import {
  applyDataRetention,
  type DesktopSettings,
  getAppOverview,
  getDataCleanupPreview,
  getDesktopSettings,
  getModelLibrary,
  isTauriRuntime,
  type RetentionSettingsDraft,
  setDefaultDownloadSource,
  setLaunchAtLogin,
  updateRetentionSettings,
} from "../../lib/desktop-api";

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
  const [usageRetentionDays, setUsageRetentionDays] = useState<number | null>(90);
  const [auditRetentionDays, setAuditRetentionDays] = useState<number | null>(365);
  const [storageOpen, setStorageOpen] = useState(false);
  const [aboutOpen, setAboutOpen] = useState(false);

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

  if (settings.isPending) {
    return <div className="state-message">正在读取桌面设置…</div>;
  }
  if (settings.isError) {
    return <div className="state-message error">{errorMessage(settings.error)}</div>;
  }

  const data = settings.data;
  const selectedSource = modelLibrary.data?.defaultDownloadSource ?? null;
  const preview = cleanupPreview.data;
  const cleanupCount = (preview?.usageRequestCount ?? 0) + (preview?.auditEventCount ?? 0);
  const settingsSaving =
    launchMutation.isPending || sourceMutation.isPending || retentionMutation.isPending;

  const changeRetention = (
    nextUsageRetentionDays: number | null,
    nextAuditRetentionDays: number | null,
  ) => {
    setUsageRetentionDays(nextUsageRetentionDays);
    setAuditRetentionDays(nextAuditRetentionDays);
    if (isTauriRuntime()) {
      retentionMutation.mutate({
        usageRetentionDays: nextUsageRetentionDays,
        auditRetentionDays: nextAuditRetentionDays,
      });
    }
  };

  return (
    <div className="page-content settings-page">
      <PageHeader
        action={
          <span className={`status-pill ${settingsSaving ? "neutral" : "ok"}`}>
            {settingsSaving ? "正在保存…" : "设置已保存"}
          </span>
        }
        description="管理全局偏好、数据保留与产品信息。"
        title="设置"
      />

      {!data.onboardingCompleted && (
        <section className="settings-first-run-note">
          <div>
            <strong>首次使用从首页开始</strong>
            <p>先选择本地模型、已有服务或云端服务；这里的偏好都可以稍后再调整。</p>
          </div>
          <Link className="secondary-button" to="/">
            返回首页
          </Link>
        </section>
      )}

      <div className="settings-grid-v2">
        <section className="settings-card settings-section-v2">
          <div className="settings-card-heading">
            <h2>通用</h2>
            <span>外观与启动</span>
          </div>
          <div className="settings-row">
            <div>
              <strong>深色外观</strong>
              <span>只影响 HAL100，不改变系统设置。</span>
            </div>
            <button
              aria-label="切换深色外观"
              aria-pressed={darkMode}
              className={`toggle-button${darkMode ? " enabled" : ""}`}
              onClick={onToggleTheme}
              type="button"
            >
              <span />
              <span className="visually-hidden">{darkMode ? "已开启" : "已关闭"}</span>
            </button>
          </div>
          <div className="settings-row">
            <div>
              <strong>随系统登录启动</strong>
              <span>达到稳定使用后再按需开启。</span>
            </div>
            <button
              aria-label="切换登录启动"
              aria-pressed={data.launchAtLoginEnabled}
              className={`toggle-button${data.launchAtLoginEnabled ? " enabled" : ""}`}
              disabled={launchMutation.isPending || !isTauriRuntime()}
              onClick={() => launchMutation.mutate(!data.launchAtLoginEnabled)}
              type="button"
            >
              <span />
              <span className="visually-hidden">
                {data.launchAtLoginEnabled ? "已开启" : "已关闭"}
              </span>
            </button>
          </div>
        </section>

        <section className="settings-card settings-section-v2">
          <div className="settings-card-heading">
            <h2>模型来源</h2>
            <span>下载时仍可临时切换</span>
          </div>
          <div className="settings-row">
            <div>
              <strong>默认下载源</strong>
              <span>只用于 HAL100 管理的本地模型。</span>
            </div>
            <select
              aria-label="默认模型下载源"
              disabled={sourceMutation.isPending || !isTauriRuntime()}
              onChange={(event) =>
                sourceMutation.mutate(event.target.value as "huggingFace" | "modelScope")
              }
              value={selectedSource ?? "huggingFace"}
            >
              <option value="huggingFace">Hugging Face</option>
              <option value="modelScope">ModelScope</option>
            </select>
          </div>
          <div className="settings-row">
            <div>
              <strong>模型存储</strong>
              <span>技术路径按需查看。</span>
            </div>
            <button className="secondary-button" onClick={() => setStorageOpen(true)} type="button">
              查看占用
            </button>
          </div>
        </section>

        <section className="settings-card settings-section-v2">
          <div className="settings-card-heading">
            <h2>数据保留</h2>
            <span>仅保存在本机</span>
          </div>
          <div className="settings-row">
            <div>
              <strong>用量记录</strong>
              <span>保存精确 Token 和脱敏请求摘要。</span>
            </div>
            <select
              aria-label="用量记录保留时间"
              onChange={(event) =>
                changeRetention(retentionOption(event.target.value), auditRetentionDays)
              }
              value={retentionSelectValue(usageRetentionDays)}
            >
              <option value="30">30 天</option>
              <option value="90">90 天</option>
              <option value="180">180 天</option>
              <option value="365">365 天</option>
              <option value="forever">永久保留</option>
            </select>
          </div>
          <div className="settings-row">
            <div>
              <strong>操作记录</strong>
              <span>不保存提示词、回答或 API Key。</span>
            </div>
            <select
              aria-label="操作记录保留时间"
              onChange={(event) =>
                changeRetention(usageRetentionDays, retentionOption(event.target.value))
              }
              value={retentionSelectValue(auditRetentionDays)}
            >
              <option value="30">30 天</option>
              <option value="90">90 天</option>
              <option value="180">180 天</option>
              <option value="365">365 天</option>
              <option value="forever">永久保留</option>
            </select>
          </div>
          <div className="settings-row">
            <div>
              <strong>清理过期记录</strong>
              <span>
                预计可清理 {cleanupPreview.isPending ? "…" : `${cleanupCount} 条`}
                ，执行前会再次确认。
              </span>
            </div>
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
              {cleanupMutation.isPending ? "正在清理…" : "预览并清理"}
            </button>
          </div>
        </section>

        <section className="settings-card settings-section-v2">
          <div className="settings-card-heading">
            <h2>关于</h2>
            <span>HAL100 {overview.data?.version ?? "—"}</span>
          </div>
          <div className="settings-row">
            <div>
              <strong>产品与系统信息</strong>
              <span>版本、平台与后台行为。</span>
            </div>
            <button className="secondary-button" onClick={() => setAboutOpen(true)} type="button">
              产品详情
            </button>
          </div>
          <div className="settings-row">
            <div>
              <strong>首次使用引导</strong>
              <span>重新查看三种连接方式。</span>
            </div>
            <Link className="secondary-button" to="/?guide=1">
              重新体验
            </Link>
          </div>
        </section>
      </div>

      {(sourceMutation.isError || launchMutation.isError || retentionMutation.isError) && (
        <p className="inline-error">
          {errorMessage(sourceMutation.error ?? launchMutation.error ?? retentionMutation.error)}
        </p>
      )}
      {cleanupMutation.isError && (
        <p className="inline-error">{errorMessage(cleanupMutation.error)}</p>
      )}
      {cleanupMutation.data && (
        <p className="inline-success">
          已删除 {cleanupMutation.data.usageRequestsDeleted} 条 Token 请求记录和{" "}
          {cleanupMutation.data.auditEventsDeleted} 条操作记录。
        </p>
      )}

      {storageOpen && (
        <Drawer
          description="HAL100 管理的模型与外部导入索引。"
          eyebrow="模型存储"
          onClose={() => setStorageOpen(false)}
          title="模型存储"
        >
          <div className="settings-drawer-summary">
            <HardDrive size={20} />
            <div>
              <strong>{modelLibrary.data?.models.length ?? 0} 个模型</strong>
              <span>存储路径和单个模型大小可在模型库中查看。</span>
            </div>
          </div>
          <Link className="primary-button" to="/workspace/models">
            打开模型库
          </Link>
        </Drawer>
      )}

      {aboutOpen && (
        <Drawer
          description="本机正在运行的产品和平台信息。"
          eyebrow="关于"
          onClose={() => setAboutOpen(false)}
          title={`HAL100 ${overview.data?.version ?? ""}`}
        >
          <dl className="settings-about-list">
            <div>
              <dt>平台</dt>
              <dd>
                {overview.data
                  ? `${overview.data.platform.os} · ${overview.data.platform.architecture}`
                  : "正在读取…"}
              </dd>
            </div>
            <div>
              <dt>窗口关闭后</dt>
              <dd>{data.closeBehavior}</dd>
            </div>
            <div>
              <dt>数据位置</dt>
              <dd>仅保存在本机</dd>
            </div>
          </dl>
          <Link className="secondary-button" to="/agent">
            让 Agent 检查环境
          </Link>
        </Drawer>
      )}

      {!isTauriRuntime() && (
        <section className="idle-cost-note">
          <ShieldCheck className="idle-cost-icon" size={16} />
          <p>浏览器预览不会修改系统登录项、SQLite 或执行数据清理。</p>
        </section>
      )}
    </div>
  );
}
