import type { UsageDailySummary, UsageHourlySummary } from "../../lib/desktop-api";

export type UsageTrendRange = "year" | "month" | "week" | "day";

export interface UsageTokenParts {
  cacheHitInputTokens: number;
  cacheMissInputTokens: number;
  outputTokens: number;
}

export interface UsageTrendPoint extends UsageTokenParts {
  key: string;
  label: string;
  tooltipLabel: string;
  requestCount: number;
  future: boolean;
  unavailable: boolean;
}

export interface UsageTrendModel {
  rangeLabel: string;
  grainLabel: string;
  points: UsageTrendPoint[];
}

export interface UsageScopeBounds {
  start: Date;
  endExclusive: Date;
}

export function localDateKey(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

export function localDateFromKey(dateKey: string): Date {
  const [year, month, day] = dateKey.split("-").map(Number);
  return new Date(year, month - 1, day, 12, 0, 0, 0);
}

export function startOfLocalDay(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

export function addLocalDays(date: Date, days: number): Date {
  const result = new Date(date);
  result.setDate(result.getDate() + days);
  return result;
}

export function formatMonthDay(date: Date): string {
  return new Intl.DateTimeFormat("zh-CN", { month: "numeric", day: "numeric" }).format(date);
}

export function usageTokenParts(entry: {
  cachedTokens: number;
  inputTokens: number;
  outputTokens: number;
}): UsageTokenParts {
  const cacheHitInputTokens = Math.min(entry.cachedTokens, entry.inputTokens);
  return {
    cacheHitInputTokens,
    cacheMissInputTokens: Math.max(0, entry.inputTokens - cacheHitInputTokens),
    outputTokens: entry.outputTokens,
  };
}

function sumUsageTokenParts(
  entries: Array<{ cachedTokens: number; inputTokens: number; outputTokens: number }>,
): UsageTokenParts {
  return entries.reduce<UsageTokenParts>(
    (sum, entry) => {
      const parts = usageTokenParts(entry);
      return {
        cacheHitInputTokens: sum.cacheHitInputTokens + parts.cacheHitInputTokens,
        cacheMissInputTokens: sum.cacheMissInputTokens + parts.cacheMissInputTokens,
        outputTokens: sum.outputTokens + parts.outputTokens,
      };
    },
    { cacheHitInputTokens: 0, cacheMissInputTokens: 0, outputTokens: 0 },
  );
}

export function usageScopeBounds(range: UsageTrendRange, anchorDate: string): UsageScopeBounds {
  const anchor = localDateFromKey(anchorDate);
  if (range === "year") {
    return {
      start: new Date(anchor.getFullYear(), 0, 1),
      endExclusive: new Date(anchor.getFullYear() + 1, 0, 1),
    };
  }
  if (range === "month") {
    return {
      start: new Date(anchor.getFullYear(), anchor.getMonth(), 1),
      endExclusive: new Date(anchor.getFullYear(), anchor.getMonth() + 1, 1),
    };
  }
  if (range === "week") {
    const mondayIndex = (anchor.getDay() + 6) % 7;
    const start = startOfLocalDay(addLocalDays(anchor, -mondayIndex));
    return { start, endExclusive: addLocalDays(start, 7) };
  }
  const start = startOfLocalDay(anchor);
  return { start, endExclusive: addLocalDays(start, 1) };
}

export function shiftUsageAnchor(
  range: UsageTrendRange,
  anchorDate: string,
  direction: -1 | 1,
): string {
  const anchor = localDateFromKey(anchorDate);
  if (range === "year") anchor.setFullYear(anchor.getFullYear() + direction);
  if (range === "month") anchor.setMonth(anchor.getMonth() + direction);
  if (range === "week") anchor.setDate(anchor.getDate() + direction * 7);
  if (range === "day") anchor.setDate(anchor.getDate() + direction);
  return localDateKey(anchor);
}

export function canMoveUsageAnchorForward(range: UsageTrendRange, anchorDate: string): boolean {
  const next = shiftUsageAnchor(range, anchorDate, 1);
  return usageScopeBounds(range, next).start <= new Date();
}

export function usageScopeLabel(range: UsageTrendRange, anchorDate: string): string {
  const bounds = usageScopeBounds(range, anchorDate);
  const anchor = localDateFromKey(anchorDate);
  if (range === "year") return `${anchor.getFullYear()} 年`;
  if (range === "month") return `${anchor.getFullYear()} 年 ${anchor.getMonth() + 1} 月`;
  if (range === "week") {
    return `${formatMonthDay(bounds.start)}—${formatMonthDay(addLocalDays(bounds.endExclusive, -1))}`;
  }
  return new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "long",
    day: "numeric",
  }).format(anchor);
}

function pointState(
  date: Date,
  now: Date,
  earliestUsageAtMs: number | null,
): Pick<UsageTrendPoint, "future" | "unavailable"> {
  return {
    future: startOfLocalDay(date) > startOfLocalDay(now),
    unavailable: earliestUsageAtMs != null && addLocalDays(date, 1).getTime() <= earliestUsageAtMs,
  };
}

export function buildUsageTrendModel(
  range: UsageTrendRange,
  anchorDate: string,
  dailyUsage: UsageDailySummary[],
  hourlyUsage: UsageHourlySummary[],
  earliestUsageAtMs: number | null,
): UsageTrendModel {
  const anchor = localDateFromKey(anchorDate);
  const now = new Date();
  const daily = new Map(dailyUsage.map((entry) => [entry.date, entry]));
  if (range === "year") {
    const year = anchor.getFullYear();
    return {
      rangeLabel: `${year} 年`,
      grainLabel: "按月汇总 · 本地时间",
      points: Array.from({ length: 12 }, (_, month) => {
        const prefix = `${year}-${String(month + 1).padStart(2, "0")}-`;
        const entries = dailyUsage.filter((entry) => entry.date.startsWith(prefix));
        const date = new Date(year, month, 1, 12);
        return {
          key: `${year}-${month + 1}`,
          label: `${month + 1}月`,
          tooltipLabel: `${year}年${month + 1}月`,
          requestCount: entries.reduce((sum, entry) => sum + entry.requestCount, 0),
          ...sumUsageTokenParts(entries),
          ...pointState(date, now, earliestUsageAtMs),
        };
      }),
    };
  }
  if (range === "month") {
    const year = anchor.getFullYear();
    const month = anchor.getMonth();
    const daysInMonth = new Date(year, month + 1, 0).getDate();
    return {
      rangeLabel: `${year} 年 ${month + 1} 月`,
      grainLabel: "按日汇总 · 本地时间",
      points: Array.from({ length: daysInMonth }, (_, index) => {
        const date = new Date(year, month, index + 1, 12);
        const key = localDateKey(date);
        const entry = daily.get(key);
        return {
          key,
          label: `${index + 1}日`,
          tooltipLabel: `${year}年${month + 1}月${index + 1}日`,
          requestCount: entry?.requestCount ?? 0,
          ...(entry
            ? usageTokenParts(entry)
            : { cacheHitInputTokens: 0, cacheMissInputTokens: 0, outputTokens: 0 }),
          ...pointState(date, now, earliestUsageAtMs),
        };
      }),
    };
  }
  if (range === "week") {
    const bounds = usageScopeBounds(range, anchorDate);
    const weekdayLabels = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];
    return {
      rangeLabel: usageScopeLabel(range, anchorDate),
      grainLabel: "按日汇总 · 本地时间",
      points: Array.from({ length: 7 }, (_, index) => {
        const date = addLocalDays(bounds.start, index);
        const key = localDateKey(date);
        const entry = daily.get(key);
        return {
          key,
          label: weekdayLabels[index],
          tooltipLabel: `${formatMonthDay(date)} ${weekdayLabels[index]}`,
          requestCount: entry?.requestCount ?? 0,
          ...(entry
            ? usageTokenParts(entry)
            : { cacheHitInputTokens: 0, cacheMissInputTokens: 0, outputTokens: 0 }),
          ...pointState(date, now, earliestUsageAtMs),
        };
      }),
    };
  }
  const hourly = new Map(hourlyUsage.map((entry) => [entry.hour, entry]));
  return {
    rangeLabel: usageScopeLabel(range, anchorDate),
    grainLabel: "按小时汇总 · 本地时间",
    points: Array.from({ length: 24 }, (_, hour) => {
      const entry = hourly.get(hour);
      const pointTime = new Date(anchor.getFullYear(), anchor.getMonth(), anchor.getDate(), hour);
      return {
        key: `${anchorDate}-${hour}`,
        label: `${String(hour).padStart(2, "0")}:00`,
        tooltipLabel: `${formatMonthDay(anchor)} ${String(hour).padStart(2, "0")}:00`,
        requestCount: entry?.requestCount ?? 0,
        ...(entry
          ? usageTokenParts(entry)
          : { cacheHitInputTokens: 0, cacheMissInputTokens: 0, outputTokens: 0 }),
        future: pointTime > now,
        unavailable:
          earliestUsageAtMs != null && pointTime.getTime() + 60 * 60 * 1_000 <= earliestUsageAtMs,
      };
    }),
  };
}
