import { useQuery } from "@tanstack/react-query";
import { ChevronRight, ListFilter, RefreshCw, ScrollText, Search } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router-dom";
import { Drawer } from "../../components/ui/Drawer";
import { getAuditLog } from "../../lib/desktop-api";
import { ActivityPageShell } from "./ActivityPageShell";

const eventLabels: Record<string, string> = {
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

const detailLabels: Record<string, string> = {
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

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
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

function outcome(eventType: string): "failed" | "succeeded" {
  return eventType.includes("failed") ? "failed" : "succeeded";
}

export default function AuditPage() {
  const [eventType, setEventType] = useState("all");
  const [search, setSearch] = useState("");
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [selectedEventId, setSelectedEventId] = useState<string | null>(null);
  const audit = useQuery({
    queryKey: ["audit-log"],
    queryFn: getAuditLog,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });

  if (audit.isPending) return <div className="state-message">正在读取本机操作记录…</div>;
  if (audit.isError) return <div className="state-message error">{errorMessage(audit.error)}</div>;

  const normalizedSearch = search.trim().toLocaleLowerCase("zh-CN");
  const eventTypes = [...new Set(audit.data.events.map((event) => event.eventType))];
  const events = audit.data.events.filter((event) => {
    if (eventType !== "all" && event.eventType !== eventType) return false;
    if (!normalizedSearch) return true;
    return [
      event.targetId,
      eventLabels[event.eventType] ?? event.eventType,
      ...event.details.flatMap((detail) => [detail.key, detail.value]),
    ]
      .join(" ")
      .toLocaleLowerCase("zh-CN")
      .includes(normalizedSearch);
  });
  const selectedEvent = audit.data.events.find((event) => event.id === selectedEventId) ?? null;
  const filtersActive = eventType !== "all" || Boolean(normalizedSearch);
  const actions = (
    <div className="compact-header-actions">
      <button
        aria-expanded={filtersOpen}
        className={`secondary-button${filtersActive ? " active" : ""}`}
        disabled={audit.data.events.length === 0}
        onClick={() => setFiltersOpen((value) => !value)}
        type="button"
      >
        <ListFilter size={14} />
        筛选{filtersActive ? " · 已启用" : ""}
      </button>
      <button
        className="secondary-button refresh-button"
        disabled={audit.isFetching}
        onClick={() => audit.refetch()}
        type="button"
      >
        <RefreshCw className={audit.isFetching ? "spinning" : ""} size={14} />
        {audit.isFetching ? "刷新中…" : "刷新"}
      </button>
    </div>
  );

  return (
    <ActivityPageShell
      action={actions}
      description="查看最近 50 条安装、模型、软件接入、Agent 与数据策略的受控操作。"
      title="操作记录"
    >
      {filtersOpen && (
        <section className="audit-toolbar" aria-label="操作记录筛选">
          <label>
            <span>操作类型</span>
            <select value={eventType} onChange={(event) => setEventType(event.target.value)}>
              <option value="all">全部操作</option>
              {eventTypes.map((type) => (
                <option key={type} value={type}>
                  {eventLabels[type] ?? type}
                </option>
              ))}
            </select>
          </label>
          <label className="audit-search">
            <span>搜索</span>
            <input
              onChange={(event) => setSearch(event.target.value)}
              placeholder="动作或对象"
              type="search"
              value={search}
            />
          </label>
          <span className="audit-count">
            {events.length} / {audit.data.totalCount}
          </span>
        </section>
      )}

      <section className="audit-log-card" aria-label="操作记录列表">
        {audit.data.events.length === 0 ? (
          <div className="usage-empty-state">
            <ScrollText className="usage-empty-icon" size={22} />
            <strong>尚无受控操作记录</strong>
            <span>完成一次模型下载、引擎安装或客户端 Key 签发后，这里会出现记录。</span>
            <div className="empty-state-actions">
              <Link className="primary-button" to="/workspace/models">
                前往模型库
              </Link>
              <Link className="secondary-button" to="/integrations">
                前往软件接入
              </Link>
            </div>
          </div>
        ) : events.length === 0 ? (
          <div className="usage-empty-state">
            <Search className="usage-empty-icon" size={22} />
            <strong>没有匹配记录</strong>
            <span>调整操作类型或搜索词后重试。</span>
          </div>
        ) : (
          <div className="audit-list">
            {events.map((event) => {
              const eventOutcome = outcome(event.eventType);
              const object =
                event.details.find((detail) => detail.key === "displayName")?.value ??
                event.targetId;
              return (
                <button
                  aria-label={`查看${eventLabels[event.eventType] ?? event.eventType}详情`}
                  className={`audit-row${selectedEventId === event.id ? " selected" : ""}`}
                  key={event.id}
                  onClick={() => setSelectedEventId(event.id)}
                  type="button"
                >
                  <span className="audit-time">{formatRequestTime(event.createdAtMs)}</span>
                  <strong>{eventLabels[event.eventType] ?? event.eventType}</strong>
                  <span>{object}</span>
                  <small className={eventOutcome}>
                    {eventOutcome === "succeeded" ? "成功" : "失败"}
                  </small>
                  <ChevronRight size={15} />
                </button>
              );
            })}
          </div>
        )}
      </section>

      {selectedEvent && (
        <Drawer
          description="仅显示 Rust Core 返回的脱敏白名单字段。"
          eyebrow="操作详情"
          onClose={() => setSelectedEventId(null)}
          title={eventLabels[selectedEvent.eventType] ?? selectedEvent.eventType}
        >
          <dl className="audit-detail-list">
            <div>
              <dt>时间</dt>
              <dd>{formatRequestTime(selectedEvent.createdAtMs)}</dd>
            </div>
            <div>
              <dt>结果</dt>
              <dd>{outcome(selectedEvent.eventType) === "succeeded" ? "成功" : "失败"}</dd>
            </div>
            <div>
              <dt>事件类型</dt>
              <dd>{selectedEvent.eventType}</dd>
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
                <dt>{detailLabels[detail.key] ?? detail.key}</dt>
                <dd>{detail.value}</dd>
              </div>
            ))}
          </dl>
          <p className="inline-notice">
            路径、凭据、提示词和回答不会进入此页面；详情字段由 Core 固定白名单控制。
          </p>
        </Drawer>
      )}
    </ActivityPageShell>
  );
}
