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
import type { UsageRequestSummary, UsageScopeQuery, UsageTotals } from "../../lib/desktop-api";
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

const EMPTY_FILTERS: UsageFilters = {
  clientAppId: "",
  resolvedModel: "",
  backendId: "",
  status: "",
};

interface HeatmapSelection {
  date: string;
  previousAnchorDate: string | null;
  previousRange: UsageTrendRange;
}

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
  const [filters, setFilters] = useState<UsageFilters>(EMPTY_FILTERS);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [heatmapSelection, setHeatmapSelection] = useState<HeatmapSelection | null>(null);
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
  const activityStartMs = addLocalDays(today, -364).getTime();
  const activityEndMs = addLocalDays(today, 1).getTime();
  const query = useMemo<UsageScopeQuery>(
    () => ({
      startAtMs: scopeStartMs,
      endAtMsExclusive: scopeEndMs,
      seriesStartAtMs: Math.min(scopeStartMs, activityStartMs),
      seriesEndAtMsExclusive: Math.max(scopeEndMs, activityEndMs),
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
      activityEndMs,
      activityStartMs,
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
  const changeRange = (range: UsageTrendRange) => {
    setHeatmapSelection(null);
    setTrendRange(range);
  };
  const moveScope = (direction: -1 | 1) => {
    setHeatmapSelection(null);
    setAnchorDate(shiftUsageAnchor(trendRange, selectedDate, direction));
  };
  const selectHeatmapDate = (date: string) => {
    if (heatmapSelection?.date === date) {
      setAnchorDate(heatmapSelection.previousAnchorDate);
      setTrendRange(heatmapSelection.previousRange);
      setHeatmapSelection(null);
      return;
    }
    setHeatmapSelection({
      date,
      previousAnchorDate: heatmapSelection?.previousAnchorDate ?? anchorDate,
      previousRange: heatmapSelection?.previousRange ?? trendRange,
    });
    setAnchorDate(date);
    setTrendRange("day");
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
  const todayUsage =
    today.getTime() >= scopeStartMs && today.getTime() < scopeEndMs
      ? data.dailyUsage.find((entry) => entry.date === localDateKey(today))
      : undefined;
  const clientTokenTotal = Math.max(
    1,
    data.clientUsage.reduce((total, client) => total + client.totalTokens, 0),
  );

  return (
    <ActivityPageShell description="查看模型用量和受控操作，数据只保存在本机。" title="活动">
      {!hasAnyUsage ? (
        <section className="activity-empty-card">
          <div className="usage-empty-state">
            <ChartNoAxesCombined className="usage-empty-icon" size={22} />
            <strong>尚无用量记录</strong>
            <span>启动模型并从已接入的软件或“测试模型”发起请求后，这里会显示统计。</span>
            <div className="empty-state-actions">
              <Link className="primary-button" to="/workspace/test">
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
          <div className="activity-v2-summary">最近 14 天</div>
          <section className="activity-v2-metrics" aria-label="最近十四天用量摘要">
            <article className="activity-v2-metric primary">
              <span>总 Token</span>
              <strong>{formatTokens(data.totals.totalTokens)}</strong>
              <small>
                {formatTokens(data.totals.requestCount)} 个请求 · 成功率 {successRate}
              </small>
            </article>
            <article className="activity-v2-metric">
              <span>今天</span>
              <strong>{formatCompactTokens(todayUsage?.totalTokens ?? 0)}</strong>
              <small>{todayUsage?.requestCount ?? 0} 个请求</small>
            </article>
            <article className="activity-v2-metric">
              <span>缓存节省</span>
              <strong>{cacheHitRate}</strong>
              <small>少处理约 {formatCompactTokens(data.totals.cachedTokens)} Token</small>
            </article>
          </section>

          <div className="activity-v2-grid">
            <UsageTrendChart
              anchorDate={selectedDate}
              dailyUsage={data.dailyUsage}
              earliestUsageAtMs={filterOptions.data.earliestUsageAtMs}
              hourlyUsage={data.hourlyUsage}
              onShowLatest={showLatest}
              range={trendRange}
              requestCount={data.totals.requestCount}
            />

            <section className="activity-v2-client-card">
              <div className="activity-v2-section-heading">
                <div>
                  <h2>最近客户端</h2>
                  <p>按总 Token</p>
                </div>
              </div>
              <div className="activity-v2-client-list">
                {data.clientUsage.slice(0, 4).map((client) => {
                  const percentage = Math.round((client.totalTokens / clientTokenTotal) * 100);
                  return (
                    <div className="activity-v2-client" key={client.id}>
                      <strong>{usageClientDisplayName(client.id, client.displayName)}</strong>
                      <span>{percentage}%</span>
                      <div>
                        <i style={{ width: `${percentage}%` }} />
                      </div>
                    </div>
                  );
                })}
              </div>
            </section>
          </div>
          <UsageActivityHeatmap
            dailyUsage={data.dailyUsage}
            onSelectDate={selectHeatmapDate}
            selectedDate={heatmapSelection?.date ?? null}
            filtered={Object.values(filters).some(Boolean)}
          />

          <details
            className="activity-v2-advanced"
            open={detailsOpen}
            onToggle={(event) => setDetailsOpen(event.currentTarget.open)}
          >
            <summary>
              筛选、时间范围与请求明细 <ChevronRight size={14} />
            </summary>
            <div className="activity-v2-advanced-body">
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
                <button
                  className="text-button usage-latest-button"
                  onClick={showLatest}
                  type="button"
                >
                  最近有数据
                </button>
                {refreshAction}
              </section>
              <UsageFiltersBar
                filters={filters}
                onChange={setFilters}
                options={filterOptions.data}
              />
              <div className="usage-detail-grid">
                <UsageCompositionCard totals={data.totals} />
                <div className="usage-measurement-help">
                  <div className="usage-measurement-heading">
                    <div>
                      <h3>计量口径</h3>
                      <p>输入总量包含缓存命中；明细按命中、未命中和输出拆分。</p>
                    </div>
                    <strong>{measurementCoverage}</strong>
                  </div>
                  <p className="usage-measurement-caption">当前请求中有精确计量值的比例</p>
                </div>
              </div>
              <div className="usage-request-details">
                <div className="usage-request-details-heading">
                  <div>
                    <h3>请求明细</h3>
                    <p>按当前时间范围与筛选条件显示最近请求</p>
                  </div>
                  <span>{data.recentRequests.length} 条</span>
                </div>
                {detailsOpen &&
                  (data.recentRequests.length === 0 ? (
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
                  ))}
              </div>
            </div>
          </details>
        </>
      )}
    </ActivityPageShell>
  );
}
