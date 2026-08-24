import { useQuery } from "@tanstack/react-query";
import { ChartNoAxesCombined, ChevronRight, RefreshCw } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router-dom";
import type { UsageRequestSummary, UsageTotals } from "../../lib/desktop-api";
import { getUsageDashboard } from "../../lib/desktop-api";
import { ActivityPageShell } from "./ActivityPageShell";

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
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
  const areaPath = `M ${pointX(0)} ${baseline} L ${totalPoints.replaceAll(",", " ")} L ${pointX(points.length - 1)} ${baseline} Z`;
  const xLabelIndexes = [...new Set([0, Math.floor((points.length - 1) / 2), points.length - 1])];

  return (
    <article className="usage-chart-card usage-trend-card">
      <div className="usage-chart-heading">
        <h2>最近请求趋势</h2>
        <ul className="usage-chart-legend" aria-label="图例">
          <li className="total">总 Token</li>
          <li className="output">输出</li>
        </ul>
      </div>
      <svg
        aria-labelledby="usage-trend-title usage-trend-description"
        className="usage-trend-chart"
        role="img"
        viewBox={`0 0 ${width} ${height}`}
      >
        <title id="usage-trend-title">最近请求 Token 用量趋势</title>
        <desc id="usage-trend-description">
          展示最近 {points.length} 条请求的总 Token 与输出 Token。
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
      <p className="usage-chart-caption">最多显示当前页面最近 30 条请求。</p>
    </article>
  );
}

function UsageCompositionCard({ totals }: { totals: UsageTotals }) {
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

  return (
    <article className="usage-composition-card">
      <h3>Token 构成</h3>
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
    </article>
  );
}

function UsageClientSummary({ requests }: { requests: UsageRequestSummary[] }) {
  const totals = new Map<string, number>();
  for (const request of requests) {
    const client = usageClientDisplayName(request.clientAppId, request.clientDisplayName);
    totals.set(client, (totals.get(client) ?? 0) + requestTokenValue(request, "total"));
  }
  const clients = [...totals.entries()].sort((a, b) => b[1] - a[1]).slice(0, 3);

  return (
    <section className="usage-client-summary" aria-label="最近客户端">
      <span>最近客户端</span>
      <div>
        {clients.map(([name, tokens]) => (
          <p key={name}>
            <strong>{name}</strong>
            <span>{formatCompactTokens(tokens)}</span>
          </p>
        ))}
      </div>
    </section>
  );
}

export default function UsagePage() {
  const [detailsOpen, setDetailsOpen] = useState(false);
  const usage = useQuery({
    queryKey: ["usage-dashboard"],
    queryFn: getUsageDashboard,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });

  if (usage.isPending) return <div className="state-message">正在读取本机 Token 统计…</div>;
  if (usage.isError) return <div className="state-message error">{errorMessage(usage.error)}</div>;

  const data = usage.data;
  const hasUsage = data.totals.requestCount > 0 && data.recentRequests.length > 0;
  const refreshAction = (
    <button
      className="secondary-button refresh-button"
      disabled={usage.isFetching}
      onClick={() => usage.refetch()}
      type="button"
    >
      <RefreshCw className={usage.isFetching ? "spinning" : ""} size={14} />
      {usage.isFetching ? "刷新中…" : "刷新"}
    </button>
  );

  return (
    <ActivityPageShell
      action={refreshAction}
      description="查看经过 HAL100 的模型请求和 Token 用量。"
      title="用量"
    >
      {!hasUsage ? (
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
          <section className="usage-primary-overview" aria-label="用量摘要">
            <div className="usage-primary-metrics">
              <p>
                <span>总 Token</span>
                <strong>{formatTokens(data.totals.totalTokens)}</strong>
              </p>
              <p>
                <span>请求数</span>
                <strong>{formatTokens(data.totals.requestCount)}</strong>
              </p>
            </div>
            <UsageClientSummary requests={data.recentRequests} />
          </section>

          <UsageTrendChart requests={data.recentRequests} />

          <details
            className="usage-requests-card disclosure-card"
            onToggle={(event) => setDetailsOpen(event.currentTarget.open)}
          >
            <summary className="usage-section-heading">
              <div>
                <h2>用量构成与请求明细</h2>
                <p>{data.recentRequests.length} 条最近请求</p>
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
                    <h3>计量说明</h3>
                    <p>Token 数直接采用推理后端返回的 usage，不按字符数估算。</p>
                    <p>页面不轮询，只在进入页面、相关请求完成或手动刷新时读取。</p>
                  </div>
                </div>
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
              </>
            )}
          </details>
        </>
      )}
    </ActivityPageShell>
  );
}
