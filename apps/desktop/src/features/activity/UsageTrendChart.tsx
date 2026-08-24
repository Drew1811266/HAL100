import { useState } from "react";
import type { UsageDailySummary, UsageHourlySummary } from "../../lib/desktop-api";
import { buildUsageTrendModel, type UsageTrendRange } from "./usage-domain";

function formatTokens(tokens: number): string {
  return new Intl.NumberFormat("zh-CN").format(tokens);
}

function formatCompactTokens(tokens: number): string {
  return new Intl.NumberFormat("zh-CN", {
    notation: "compact",
    maximumFractionDigits: 1,
  }).format(tokens);
}

function chartScaleMaximum(value: number): number {
  if (value <= 0) return 1;
  const magnitude = 10 ** Math.floor(Math.log10(value));
  const normalized = value / magnitude;
  const ceiling = normalized <= 1 ? 1 : normalized <= 2 ? 2 : normalized <= 5 ? 5 : 10;
  return ceiling * magnitude;
}

const SERIES = [
  { key: "cacheHitInputTokens", label: "输入（缓存命中）", tone: "cache-hit" },
  { key: "cacheMissInputTokens", label: "输入（缓存未命中）", tone: "cache-miss" },
  { key: "outputTokens", label: "输出", tone: "output" },
] as const;

type SeriesKey = (typeof SERIES)[number]["key"];

export function UsageTrendChart({
  anchorDate,
  dailyUsage,
  earliestUsageAtMs,
  hourlyUsage,
  range,
  requestCount,
  onShowLatest,
}: {
  anchorDate: string;
  dailyUsage: UsageDailySummary[];
  earliestUsageAtMs: number | null;
  hourlyUsage: UsageHourlySummary[];
  range: UsageTrendRange;
  requestCount: number;
  onShowLatest: () => void;
}) {
  const [activeIndex, setActiveIndex] = useState<number | null>(null);
  const [hiddenSeries, setHiddenSeries] = useState<Set<SeriesKey>>(() => new Set());
  const model = buildUsageTrendModel(range, anchorDate, dailyUsage, hourlyUsage, earliestUsageAtMs);
  const points = model.points;
  const visibleSeries = SERIES.filter((series) => !hiddenSeries.has(series.key));
  const plottablePoints = points.filter((point) => !point.future && !point.unavailable);
  const peak = Math.max(
    0,
    ...visibleSeries.flatMap((series) => plottablePoints.map((point) => point[series.key])),
  );
  const scaleMaximum = chartScaleMaximum(peak);
  const width = 920;
  const height = 300;
  const plot = { left: 62, right: 18, top: 24, bottom: 40 };
  const plotWidth = width - plot.left - plot.right;
  const plotHeight = height - plot.top - plot.bottom;
  const baseline = plot.top + plotHeight;
  const pointX = (index: number) =>
    plot.left + (points.length === 1 ? plotWidth / 2 : (index / (points.length - 1)) * plotWidth);
  const pointY = (value: number) => baseline - (value / scaleMaximum) * plotHeight;
  const xLabelIndexes =
    range === "day"
      ? [0, 6, 12, 18, 23]
      : points.length <= 7
        ? points.map((_, index) => index)
        : [0, 0.25, 0.5, 0.75, 1].map((ratio) => Math.round((points.length - 1) * ratio));
  const activePoint = activeIndex == null ? null : points[activeIndex];
  const toggleSeries = (key: SeriesKey) => {
    setHiddenSeries((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else if (current.size < SERIES.length - 1) next.add(key);
      return next;
    });
  };

  return (
    <article className="usage-chart-card usage-trend-card">
      <div className="usage-chart-heading">
        <div className="usage-chart-heading-copy">
          <h2>Token 变化</h2>
          <p>
            {model.rangeLabel} · {model.grainLabel}
          </p>
        </div>
        <span className="usage-chart-unit">单位：Token</span>
      </div>
      <fieldset className="usage-token-series-legend">
        <legend className="visually-hidden">点击可显示或隐藏 Token 类型</legend>
        {SERIES.map((series) => {
          const total = points.reduce((sum, point) => sum + point[series.key], 0);
          const hidden = hiddenSeries.has(series.key);
          return (
            <button
              aria-pressed={!hidden}
              className={`${series.tone}${hidden ? " muted" : ""}`}
              key={series.key}
              onClick={() => toggleSeries(series.key)}
              type="button"
            >
              <span>{series.label}</span>
              <strong>{formatTokens(total)}</strong>
            </button>
          );
        })}
      </fieldset>
      {requestCount === 0 ? (
        <div className="usage-chart-empty usage-range-empty">
          <strong>这个时间范围没有请求</strong>
          <span>未来时间不会按 0 计入趋势；可以切换范围或回到最近有数据的日期。</span>
          <button className="secondary-button" onClick={onShowLatest} type="button">
            查看最近用量
          </button>
        </div>
      ) : (
        <div className="usage-trend-visual">
          <svg
            aria-labelledby="usage-trend-title usage-trend-description"
            className="usage-trend-chart"
            role="img"
            viewBox={`0 0 ${width} ${height}`}
          >
            <title id="usage-trend-title">{model.rangeLabel} Token 用量趋势</title>
            <desc id="usage-trend-description">
              三条折线分别表示缓存命中的输入 Token、缓存未命中的输入 Token 和输出
              Token。未来时间不绘制。
            </desc>
            {[1, 0.5, 0].map((ratio) => {
              const y = baseline - ratio * plotHeight;
              return (
                <g className="usage-chart-grid" key={ratio}>
                  <line x1={plot.left} x2={width - plot.right} y1={y} y2={y} />
                  <text textAnchor="end" x={plot.left - 12} y={y + 4}>
                    {formatCompactTokens(scaleMaximum * ratio)}
                  </text>
                </g>
              );
            })}
            {visibleSeries.map((series) => {
              const plotted = points
                .map((point, index) => ({ point, index }))
                .filter(({ point }) => !point.future && !point.unavailable);
              return (
                <g className={`usage-chart-series ${series.tone}`} key={series.key}>
                  <polyline
                    className={`usage-chart-line ${series.tone}`}
                    points={plotted
                      .map(({ point, index }) => `${pointX(index)},${pointY(point[series.key])}`)
                      .join(" ")}
                  />
                  {plotted
                    .filter(({ point, index }) => point[series.key] > 0 || index === activeIndex)
                    .map(({ point, index }) => (
                      <circle
                        className={`usage-chart-point ${series.tone}`}
                        cx={pointX(index)}
                        cy={pointY(point[series.key])}
                        key={`${point.key}-${series.key}`}
                        r={index === activeIndex ? 4 : 2.4}
                      />
                    ))}
                </g>
              );
            })}
            {activePoint && !activePoint.future && !activePoint.unavailable && (
              <line
                className="usage-chart-crosshair"
                x1={pointX(activeIndex ?? 0)}
                x2={pointX(activeIndex ?? 0)}
                y1={plot.top}
                y2={baseline}
              />
            )}
            {xLabelIndexes.map((index) => (
              <text
                className="usage-chart-x-label"
                key={points[index].key}
                textAnchor={index === 0 ? "start" : index === points.length - 1 ? "end" : "middle"}
                x={pointX(index)}
                y={height - 9}
              >
                {points[index].label}
              </text>
            ))}
          </svg>
          <div className="usage-chart-hit-columns">
            {points.map((point, index) => (
              <button
                aria-label={`${point.tooltipLabel}，显示详细 Token 值`}
                key={`${point.key}-target`}
                onBlur={() => setActiveIndex(null)}
                onClick={() => setActiveIndex(index)}
                onFocus={() => setActiveIndex(index)}
                onMouseEnter={() => setActiveIndex(index)}
                onMouseLeave={() => setActiveIndex(null)}
                type="button"
              />
            ))}
          </div>
          {activePoint && !activePoint.future && !activePoint.unavailable && (
            <div
              className="usage-chart-tooltip"
              style={{
                left: `${Math.min(88, Math.max(12, (pointX(activeIndex ?? 0) / width) * 100))}%`,
              }}
            >
              <strong>{activePoint.tooltipLabel}</strong>
              <span>{activePoint.requestCount} 次请求</span>
              {SERIES.map((series) => (
                <span className={series.tone} key={series.key}>
                  {series.label}：{formatTokens(activePoint[series.key])}
                </span>
              ))}
            </div>
          )}
        </div>
      )}
      <p className="usage-chart-caption">
        输入（缓存未命中）= 输入总量 − 缓存命中；三类数据都来自后端 usage 精确值。
      </p>
    </article>
  );
}
