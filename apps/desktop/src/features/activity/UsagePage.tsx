import { useQuery } from "@tanstack/react-query";
import {
  ChartNoAxesCombined,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  RefreshCw,
  RotateCcw,
} from "lucide-react";
import { useMemo, useState } from "react";
import { Link } from "react-router-dom";
import type {
  UsageDimensionSummary,
  UsageRequestSummary,
  UsageScopeQuery,
  UsageTotals,
} from "../../lib/desktop-api";
import { getUsageFilterOptions, getUsageScope } from "../../lib/desktop-api";
import { ActivityPageShell } from "./ActivityPageShell";
import { UsageActivityHeatmap } from "./UsageActivityHeatmap";
import { UsageTrendChart } from "./UsageTrendChart";
import {
  addLocalDays,
  canMoveUsageAnchorForward,
  localDateKey,
  shiftUsageAnchor,
  startOfLocalDay,
  type UsageTrendRange,
  usageScopeBounds,
  usageScopeLabel,
  usageTokenParts,
} from "./usage-domain";

interface UsageFilters {
  clientAppId: string;
  resolvedModel: string;
  backendId: string;
  status: string;
}

interface UsageHeatmapSelection {
  date: string;
  returnAnchorDate: string;
  returnRange: UsageTrendRange;
}

const EMPTY_FILTERS: UsageFilters = {
  clientAppId: "",
  resolvedModel: "",
  backendId: "",
  status: "",
};

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function formatTokens(tokens: number | null | undefined): string {
  return tokens == null ? "—" : new Intl.NumberFormat("zh-CN").format(tokens);
}

function formatCompactTokens(tokens: number): string {
  return new Intl.NumberFormat("zh-CN", {
    notation: "compact",
    maximumFractionDigits: 1,
  }).format(tokens);
}

function formatPercent(numerator: number, denominator: number): string {
  if (denominator <= 0) return "—";
  return `${new Intl.NumberFormat("zh-CN", { maximumFractionDigits: 1 }).format((numerator / denominator) * 100)}%`;
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

function requestTokenParts(request: UsageRequestSummary) {
  const inputTokens = request.inputTokens ?? 0;
  const cachedTokens = Math.min(request.cachedTokens ?? 0, inputTokens);
  return {
    cacheHit: request.cachedTokens == null ? null : cachedTokens,
    cacheMiss: request.inputTokens == null ? null : Math.max(0, inputTokens - cachedTokens),
  };
}

function UsageClientSummary({ clients }: { clients: UsageDimensionSummary[] }) {
  return (
    <section className="usage-client-summary" aria-label="当前范围主要客户端">
      <span>主要客户端</span>
      <div>
        {clients.length === 0 ? (
          <p className="usage-client-empty">当前范围无客户端数据</p>
        ) : (
          clients.slice(0, 3).map((client) => (
            <p key={client.id}>
              <strong>{usageClientDisplayName(client.id, client.displayName)}</strong>
              <span>
                {formatCompactTokens(client.totalTokens)} Token · {client.requestCount} 次
              </span>
            </p>
          ))
        )}
      </div>
    </section>
  );
}

function UsageCompositionCard({ totals }: { totals: UsageTotals }) {
  const parts = usageTokenParts(totals);
  const segments = [
    { label: "输入（缓存未命中）", value: parts.cacheMissInputTokens, tone: "cache-miss" },
    { label: "输入（缓存命中）", value: parts.cacheHitInputTokens, tone: "cache-hit" },
    { label: "输出", value: parts.outputTokens, tone: "output" },
  ];
  const denominator = segments.reduce((sum, segment) => sum + segment.value, 0);
  return (
    <article className="usage-composition-card">
      <div className="usage-composition-heading">
        <div>
          <h3>Token 构成</h3>
          <p>当前时间范围与筛选条件</p>
        </div>
        <strong>{formatTokens(denominator)}</strong>
      </div>
      {denominator === 0 ? (
        <div className="usage-composition-empty">没有可构成的精确 Token 数据</div>
      ) : (
        <>
          <div className="usage-composition-bar" aria-label="Token 构成比例" role="img">
            {segments.map((segment) => (
              <span
                className={segment.tone}
                key={segment.label}
                style={{ width: `${(segment.value / denominator) * 100}%` }}
                title={`${segment.label} ${formatTokens(segment.value)}`}
              />
            ))}
          </div>
          <div className="usage-composition-legend">
            {segments.map((segment) => (
              <div key={segment.label}>
                <span className={`usage-legend-dot ${segment.tone}`} />
                <span>{segment.label}</span>
                <strong>{formatTokens(segment.value)}</strong>
                <small>{formatPercent(segment.value, denominator)}</small>
              </div>
            ))}
          </div>
        </>
      )}
    </article>
  );
}

function UsageFiltersBar({
  filters,
  onChange,
  options,
}: {
  filters: UsageFilters;
  onChange: (filters: UsageFilters) => void;
  options: Awaited<ReturnType<typeof getUsageFilterOptions>>;
}) {
  const activeCount = Object.values(filters).filter(Boolean).length;
  const setFilter = (key: keyof UsageFilters, value: string) =>
    onChange({ ...filters, [key]: value });
  return (
    <details className="usage-filter-disclosure">
      <summary>
        <span>
          筛选{activeCount > 0 ? ` · ${activeCount} 项` : ""}
          <ChevronDown size={14} />
        </span>
        {activeCount > 0 && <strong>已应用到整页</strong>}
      </summary>
      <div className="usage-filter-grid">
        <label>
          客户端
          <select
            value={filters.clientAppId}
            onChange={(event) => setFilter("clientAppId", event.target.value)}
          >
            <option value="">全部客户端</option>
            {options.clients.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>
        <label>
          模型
          <select
            value={filters.resolvedModel}
            onChange={(event) => setFilter("resolvedModel", event.target.value)}
          >
            <option value="">全部模型</option>
            {options.models.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>
        <label>
          后端
          <select
            value={filters.backendId}
            onChange={(event) => setFilter("backendId", event.target.value)}
          >
            <option value="">全部后端</option>
            {options.backends.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>
        <label>
          状态
          <select
            value={filters.status}
            onChange={(event) => setFilter("status", event.target.value)}
          >
            <option value="">全部状态</option>
            <option value="succeeded">成功</option>
            <option value="failed">失败</option>
            <option value="cancelled">已取消</option>
          </select>
        </label>
        <button
          className="text-button"
          disabled={activeCount === 0}
          onClick={() => onChange(EMPTY_FILTERS)}
          type="button"
        >
          <RotateCcw size={13} /> 清除筛选
        </button>
      </div>
    </details>
  );
}

export default function UsagePage() {
  const [trendRange, setTrendRange] = useState<UsageTrendRange>("month");
  const [anchorDate, setAnchorDate] = useState<string | null>(null);
  const [heatmapSelection, setHeatmapSelection] = useState<UsageHeatmapSelection | null>(null);
  const [filters, setFilters] = useState<UsageFilters>(EMPTY_FILTERS);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const filterOptions = useQuery({
    queryKey: ["usage-filter-options"],
    queryFn: getUsageFilterOptions,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  const latestDate = filterOptions.data?.latestUsageAtMs
    ? localDateKey(new Date(filterOptions.data.latestUsageAtMs))
    : localDateKey(new Date());
  const selectedDate = anchorDate ?? latestDate;
  const scopeBounds = usageScopeBounds(trendRange, selectedDate);
  const today = startOfLocalDay(new Date());
  const scopeStartMs = scopeBounds.start.getTime();
  const scopeEndMs = scopeBounds.endExclusive.getTime();
  const heatmapStartMs = addLocalDays(today, -364).getTime();
  const heatmapEndMs = addLocalDays(today, 1).getTime();
  const query = useMemo<UsageScopeQuery>(
    () => ({
      startAtMs: scopeStartMs,
      endAtMsExclusive: scopeEndMs,
      seriesStartAtMs: Math.min(scopeStartMs, heatmapStartMs),
      seriesEndAtMsExclusive: Math.max(scopeEndMs, heatmapEndMs),
      clientAppId: filters.clientAppId || null,
      resolvedModel: filters.resolvedModel || null,
      backendId: filters.backendId || null,
      status: (filters.status || null) as UsageScopeQuery["status"],
      limit: 50,
    }),
    [
      filters.backendId,
      filters.clientAppId,
      filters.resolvedModel,
      filters.status,
      heatmapEndMs,
      heatmapStartMs,
      scopeEndMs,
      scopeStartMs,
    ],
  );
  const usage = useQuery({
    queryKey: ["usage-scope", query],
    queryFn: () => getUsageScope(query),
    enabled: filterOptions.isSuccess,
    placeholderData: (previousData) => previousData,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });

  if (filterOptions.isPending || usage.isPending) {
    return <div className="state-message">正在读取本机 Token 统计…</div>;
  }
  if (filterOptions.isError) {
    return <div className="state-message error">{errorMessage(filterOptions.error)}</div>;
  }
  if (usage.isError) return <div className="state-message error">{errorMessage(usage.error)}</div>;

  const data = usage.data;
  const hasAnyUsage = filterOptions.data.latestUsageAtMs != null;
  const cacheHitRate = formatPercent(data.totals.cachedTokens, data.totals.inputTokens);
  const measurementCoverage = formatPercent(data.measuredRequestCount, data.totals.requestCount);
  const successRate = formatPercent(data.succeededRequestCount, data.totals.requestCount);
  const refreshing = usage.isFetching || filterOptions.isFetching;
  const filteredLatestDate = [...data.dailyUsage]
    .reverse()
    .find((entry) => entry.requestCount > 0)?.date;
  const showLatest = () => {
    setHeatmapSelection(null);
    setAnchorDate(filteredLatestDate ?? latestDate);
    if (trendRange === "day") setTrendRange("month");
  };
  const selectHeatmapDate = (date: string) => {
    if (heatmapSelection?.date === date) {
      setTrendRange(heatmapSelection.returnRange);
      setAnchorDate(heatmapSelection.returnAnchorDate);
      setHeatmapSelection(null);
      return;
    }

    setHeatmapSelection({
      date,
      returnAnchorDate: heatmapSelection?.returnAnchorDate ?? selectedDate,
      returnRange: heatmapSelection?.returnRange ?? trendRange,
    });
    setAnchorDate(date);
    setTrendRange("day");
  };
  const changeRange = (range: UsageTrendRange) => {
    setHeatmapSelection(null);
    setTrendRange(range);
  };
  const moveScope = (direction: -1 | 1) => {
    setHeatmapSelection(null);
    setAnchorDate(shiftUsageAnchor(trendRange, selectedDate, direction));
  };
  const refreshAction = (
    <button
      className="secondary-button refresh-button"
      disabled={refreshing}
      onClick={() => {
        filterOptions.refetch();
        usage.refetch();
      }}
      type="button"
    >
      <RefreshCw className={refreshing ? "spinning" : ""} size={14} />
      {refreshing ? "刷新中…" : "刷新"}
    </button>
  );
  const rangeOptions: Array<{ label: string; value: UsageTrendRange }> = [
    { label: "年", value: "year" },
    { label: "月", value: "month" },
    { label: "周", value: "week" },
    { label: "天", value: "day" },
  ];
  const previousAnchor = shiftUsageAnchor(trendRange, selectedDate, -1);
  const canMoveBackward =
    filterOptions.data.earliestUsageAtMs == null ||
    usageScopeBounds(trendRange, previousAnchor).endExclusive.getTime() >
      filterOptions.data.earliestUsageAtMs;

  return (
    <ActivityPageShell
      action={refreshAction}
      description="按统一时间范围查看经过 HAL100 的请求、Token 构成与明细。"
      title="用量"
    >
      {!hasAnyUsage ? (
        <section className="activity-empty-card">
          <div className="usage-empty-state">
            <ChartNoAxesCombined className="usage-empty-icon" size={22} />
            <strong>尚无用量记录</strong>
            <span>启动模型并从已接入的软件或“测试模型”发起请求后，这里会显示统计。</span>
            <div className="empty-state-actions">
              <Link className="primary-button" to="/test">
                测试当前模型
              </Link>
              <Link className="secondary-button" to="/integrations">
                前往软件接入
              </Link>
            </div>
          </div>
        </section>
      ) : (
        <>
          <section className="usage-scope-toolbar" aria-label="用量时间范围与筛选">
            <fieldset className="usage-range-switch">
              <legend className="visually-hidden">用量时间粒度</legend>
              {rangeOptions.map((option) => (
                <button
                  aria-pressed={trendRange === option.value}
                  className={trendRange === option.value ? "active" : ""}
                  key={option.value}
                  onClick={() => changeRange(option.value)}
                  type="button"
                >
                  {option.label}
                </button>
              ))}
            </fieldset>
            <div className="usage-period-navigation">
              <button
                aria-label="上一个时间范围"
                disabled={!canMoveBackward}
                onClick={() => moveScope(-1)}
                type="button"
              >
                <ChevronLeft size={16} />
              </button>
              <strong>{usageScopeLabel(trendRange, selectedDate)}</strong>
              <button
                aria-label="下一个时间范围"
                disabled={!canMoveUsageAnchorForward(trendRange, selectedDate)}
                onClick={() => moveScope(1)}
                type="button"
              >
                <ChevronRight size={16} />
              </button>
            </div>
            <button className="text-button usage-latest-button" onClick={showLatest} type="button">
              最近有数据
            </button>
          </section>
          <UsageFiltersBar filters={filters} onChange={setFilters} options={filterOptions.data} />

          <section className="usage-primary-overview" aria-label="当前范围用量摘要">
            <div className="usage-primary-metrics usage-primary-metrics-expanded">
              <p>
                <span>Token 总量</span>
                <strong>{formatTokens(data.totals.totalTokens)}</strong>
                <small>{usageScopeLabel(trendRange, selectedDate)}</small>
              </p>
              <p>
                <span>请求数</span>
                <strong>{formatTokens(data.totals.requestCount)}</strong>
                <small>成功率 {successRate}</small>
              </p>
              <p>
                <span>输入缓存命中率</span>
                <strong>{cacheHitRate}</strong>
                <small>{formatTokens(data.totals.cachedTokens)} 命中 Token</small>
              </p>
              <p>
                <span>计量覆盖率</span>
                <strong>{measurementCoverage}</strong>
                <small>
                  {data.measuredRequestCount} / {data.totals.requestCount} 个请求
                </small>
              </p>
            </div>
            <UsageClientSummary clients={data.clientUsage} />
          </section>

          <section className="usage-analysis-stack" aria-label="用量分析">
            <UsageTrendChart
              anchorDate={selectedDate}
              dailyUsage={data.dailyUsage}
              earliestUsageAtMs={filterOptions.data.earliestUsageAtMs}
              hourlyUsage={data.hourlyUsage}
              onShowLatest={showLatest}
              range={trendRange}
              requestCount={data.totals.requestCount}
            />
            <UsageActivityHeatmap
              dailyUsage={data.dailyUsage}
              onSelectDate={selectHeatmapDate}
              selectedDate={heatmapSelection?.date ?? null}
              statusFiltered={Boolean(filters.status)}
            />
          </section>

          <details
            className="usage-requests-card disclosure-card"
            onToggle={(event) => setDetailsOpen(event.currentTarget.open)}
          >
            <summary className="usage-section-heading">
              <div>
                <h2>Token 构成与请求明细</h2>
                <p>
                  当前范围 {data.totals.requestCount} 次请求；最多显示最近{" "}
                  {data.recentRequests.length} 条
                </p>
              </div>
              <span className="disclosure-label">
                <span className="details-closed-copy">展开</span>
                <span className="details-open-copy">收起</span>
                <ChevronRight size={14} />
              </span>
            </summary>
            {detailsOpen && (
              <>
                <div className="usage-detail-grid">
                  <UsageCompositionCard totals={data.totals} />
                  <div className="usage-measurement-help">
                    <h3>计量口径</h3>
                    <p>输入总量包含缓存命中；表格已拆成“缓存命中”和“缓存未命中”，避免重复相加。</p>
                    <p>
                      Token 来自推理后端 usage；未返回 usage 的请求仍计入请求数，但不计入 Token。
                    </p>
                  </div>
                </div>
                {data.recentRequests.length === 0 ? (
                  <div className="usage-table-empty">当前范围没有请求明细</div>
                ) : (
                  <div className="usage-table-scroll">
                    <table className="usage-table">
                      <thead>
                        <tr>
                          <th>时间 / 客户端</th>
                          <th>模型 / 后端</th>
                          <th>
                            输入
                            <br />
                            未命中
                          </th>
                          <th>
                            输入
                            <br />
                            命中
                          </th>
                          <th>输出</th>
                          <th>总计</th>
                          <th>状态 / 计量</th>
                        </tr>
                      </thead>
                      <tbody>
                        {data.recentRequests.map((request) => {
                          const parts = requestTokenParts(request);
                          return (
                            <tr key={request.requestId}>
                              <td>
                                <strong>
                                  {usageClientDisplayName(
                                    request.clientAppId,
                                    request.clientDisplayName,
                                  )}
                                </strong>
                                <span>{formatRequestTime(request.startedAtMs)}</span>
                              </td>
                              <td>
                                <strong>{request.resolvedModel}</strong>
                                <span>{request.backendId}</span>
                              </td>
                              <td>{formatTokens(parts.cacheMiss)}</td>
                              <td>{formatTokens(parts.cacheHit)}</td>
                              <td>{formatTokens(request.outputTokens)}</td>
                              <td>{formatTokens(request.totalTokens)}</td>
                              <td>
                                <span className={`usage-status ${request.status}`}>
                                  {request.status === "succeeded"
                                    ? "成功"
                                    : request.status === "cancelled"
                                      ? "已取消"
                                      : "失败"}
                                </span>
                                <small>
                                  {request.usageAccuracy === "exact_backend_response"
                                    ? "响应精确值"
                                    : request.usageAccuracy === "exact_backend_event"
                                      ? "事件精确值"
                                      : "未计量"}
                                </small>
                              </td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                  </div>
                )}
              </>
            )}
          </details>
        </>
      )}
    </ActivityPageShell>
  );
}
