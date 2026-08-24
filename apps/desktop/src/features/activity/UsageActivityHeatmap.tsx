import type { KeyboardEvent } from "react";
import type { UsageDailySummary } from "../../lib/desktop-api";
import {
  addLocalDays,
  formatMonthDay,
  localDateFromKey,
  localDateKey,
  startOfLocalDay,
} from "./usage-domain";

interface HeatmapCell {
  date: Date;
  dateKey: string;
  requestCount: number;
  totalTokens: number;
  outside: boolean;
}

function formatTokens(tokens: number): string {
  return new Intl.NumberFormat("zh-CN").format(tokens);
}

function formatFullDate(date: Date): string {
  return new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "numeric",
    day: "numeric",
  }).format(date);
}

function buildHeatmap(dailyUsage: UsageDailySummary[]): {
  cells: HeatmapCell[];
  rangeStart: Date;
  today: Date;
} {
  const today = startOfLocalDay(new Date());
  const rangeStart = addLocalDays(today, -364);
  const gridStart = addLocalDays(rangeStart, -((rangeStart.getDay() + 6) % 7));
  const summaries = new Map(dailyUsage.map((day) => [day.date, day]));
  return {
    rangeStart,
    today,
    cells: Array.from({ length: 371 }, (_, index) => {
      const date = addLocalDays(gridStart, index);
      const dateKey = localDateKey(date);
      const summary = summaries.get(dateKey);
      return {
        date,
        dateKey,
        requestCount: summary?.requestCount ?? 0,
        totalTokens: summary?.totalTokens ?? 0,
        outside: date < rangeStart || date > today,
      };
    }),
  };
}

function heatmapLevel(requestCount: number, peak: number): number {
  if (requestCount === 0 || peak === 0) return 0;
  return Math.min(4, Math.max(1, Math.ceil(Math.sqrt(requestCount / peak) * 4)));
}

export function UsageActivityHeatmap({
  dailyUsage,
  onSelectDate,
  selectedDate,
  statusFiltered,
}: {
  dailyUsage: UsageDailySummary[];
  onSelectDate: (date: string) => void;
  selectedDate: string | null;
  statusFiltered: boolean;
}) {
  const { cells, rangeStart, today } = buildHeatmap(dailyUsage);
  const visibleCells = cells.filter((cell) => !cell.outside);
  const activeCells = visibleCells.filter((cell) => cell.requestCount > 0);
  const focusDate = selectedDate ?? localDateKey(today);
  const peak = Math.max(0, ...activeCells.map((cell) => cell.requestCount));
  const busiest = activeCells.reduce<HeatmapCell | null>(
    (current, cell) => (!current || cell.requestCount > current.requestCount ? cell : current),
    null,
  );
  const rawMonthLabels = Array.from({ length: 53 }, (_, weekIndex) => {
    const cell = cells[weekIndex * 7];
    const previous = weekIndex === 0 ? null : cells[(weekIndex - 1) * 7];
    return previous?.date.getMonth() === cell.date.getMonth()
      ? ""
      : `${cell.date.getMonth() + 1}月`;
  });
  const nextMonthIndex = rawMonthLabels.findIndex((label, index) => index > 0 && label !== "");
  const monthLabels = rawMonthLabels.map((label, index) =>
    index === 0 && nextMonthIndex > 0 && nextMonthIndex < 3 ? "" : label,
  );
  const weekdays = [
    { key: "monday", label: "一" },
    { key: "tuesday", label: "" },
    { key: "wednesday", label: "三" },
    { key: "thursday", label: "" },
    { key: "friday", label: "五" },
    { key: "saturday", label: "" },
    { key: "sunday", label: "日" },
  ];
  const handleCellKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const offset =
      event.key === "ArrowLeft"
        ? -7
        : event.key === "ArrowRight"
          ? 7
          : event.key === "ArrowUp"
            ? -1
            : event.key === "ArrowDown"
              ? 1
              : 0;
    if (offset === 0) return;
    const target = cells[index + offset];
    if (!target || target.outside) return;
    event.preventDefault();
    onSelectDate(target.dateKey);
    event.currentTarget
      .closest(".usage-heatmap-grid")
      ?.querySelector<HTMLButtonElement>(`[data-usage-date="${target.dateKey}"]`)
      ?.focus();
  };
  const description = busiest
    ? `${activeCells.length} 个活跃日；峰值为 ${formatFullDate(busiest.date)} 的 ${busiest.requestCount} 次请求。`
    : "这个年度窗口内没有符合当前筛选条件的请求。";

  return (
    <article className="usage-chart-card usage-activity-card">
      <div className="usage-chart-heading">
        <div className="usage-chart-heading-copy">
          <h2>每日请求活动</h2>
          <p>
            {formatFullDate(rangeStart)}—{formatFullDate(today)} · 365 天 · 本地时间
          </p>
        </div>
        {selectedDate ? (
          <strong className="usage-active-days">
            已选 {formatMonthDay(localDateFromKey(selectedDate))} · 再点取消
          </strong>
        ) : (
          <span className="usage-heatmap-instruction">点击日期查看当天</span>
        )}
      </div>
      <section
        aria-describedby="usage-activity-description"
        aria-label="过去 365 天每日请求活动"
        className="usage-heatmap-scroll"
      >
        <div className="usage-heatmap-canvas">
          <div className="usage-heatmap-months" aria-hidden="true">
            {monthLabels.map((label, index) => (
              <span key={`${cells[index * 7].dateKey}-month`}>{label}</span>
            ))}
          </div>
          <div className="usage-heatmap-body">
            <div className="usage-heatmap-weekdays" aria-hidden="true">
              {weekdays.map((weekday) => (
                <span key={weekday.key}>{weekday.label}</span>
              ))}
            </div>
            <table className="usage-heatmap-grid" aria-label="过去 365 天每日请求格子">
              <tbody>
                {weekdays.map((weekday, weekdayIndex) => (
                  <tr key={weekday.key}>
                    {Array.from({ length: 53 }, (_, weekIndex) => {
                      const index = weekIndex * 7 + weekdayIndex;
                      const cell = cells[index];
                      if (cell.outside) {
                        return (
                          <td key={cell.dateKey}>
                            <span className="outside" />
                          </td>
                        );
                      }
                      const label = `${new Intl.DateTimeFormat("zh-CN", { year: "numeric", month: "long", day: "numeric" }).format(cell.date)} · ${cell.requestCount} 次请求 · ${formatTokens(cell.totalTokens)} Token`;
                      return (
                        <td key={cell.dateKey}>
                          <button
                            aria-label={label}
                            aria-pressed={cell.dateKey === selectedDate}
                            className={`level-${heatmapLevel(cell.requestCount, peak)}${cell.dateKey === selectedDate ? " selected" : ""}`}
                            data-usage-date={cell.dateKey}
                            onClick={() => onSelectDate(cell.dateKey)}
                            onKeyDown={(event) => handleCellKeyDown(event, index)}
                            tabIndex={cell.dateKey === focusDate ? 0 : -1}
                            title={label}
                            type="button"
                          />
                        </td>
                      );
                    })}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className="usage-heatmap-footer" aria-hidden="true">
            <span>少</span>
            {[0, 1, 2, 3, 4].map((level) => (
              <i className={`level-${level}`} key={level} />
            ))}
            <span>多</span>
          </div>
        </div>
      </section>
      <p className="usage-chart-caption" id="usage-activity-description">
        颜色只表示{statusFiltered ? "筛选后的" : "全部状态的"}请求次数，不表示 Token 消耗。
        {description} 点击任意日期可联动整页查看当天数据，再次点击已选日期可返回此前范围。
      </p>
    </article>
  );
}
