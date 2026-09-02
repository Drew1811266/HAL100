import { describe, expect, it } from "vitest";
import { getUsageScope, isRuntimeProfileFailure } from "./desktop-api";

describe("runtime profile failure IPC contract", () => {
  it("recognizes the bounded structured failure returned by Rust", () => {
    expect(
      isRuntimeProfileFailure({
        code: "engineUnreachable",
        stage: "inspection",
        retryable: true,
        recoveryAction: "checkService",
      }),
    ).toBe(true);
  });

  it("rejects string and partial legacy errors", () => {
    expect(isRuntimeProfileFailure("外部推理引擎当前不可达")).toBe(false);
    expect(
      isRuntimeProfileFailure({
        code: "engineUnreachable",
        stage: "inspection",
      }),
    ).toBe(false);
    expect(
      isRuntimeProfileFailure({
        code: "inventedFailure",
        stage: "inspection",
        retryable: true,
        recoveryAction: "checkService",
      }),
    ).toBe(false);
  });
});

describe("browser usage preview filtering", () => {
  it("keeps totals, trend, breakdown, and requests consistent for a client filter", async () => {
    const endAtMsExclusive = Date.now() + 24 * 60 * 60 * 1_000;
    const startAtMs = endAtMsExclusive - 30 * 24 * 60 * 60 * 1_000;
    const summary = await getUsageScope({
      startAtMs,
      endAtMsExclusive,
      seriesStartAtMs: startAtMs,
      seriesEndAtMsExclusive: endAtMsExclusive,
      clientAppId: "hal100-agent",
      resolvedModel: null,
      backendId: null,
      status: null,
      limit: 50,
    });

    expect(summary.totals.requestCount).toBeGreaterThan(0);
    expect(summary.clientUsage).toEqual([
      expect.objectContaining({
        id: "hal100-agent",
        requestCount: summary.totals.requestCount,
        totalTokens: summary.totals.totalTokens,
      }),
    ]);
    expect(summary.recentRequests.every((request) => request.clientAppId === "hal100-agent")).toBe(
      true,
    );
    expect(summary.dailyUsage.reduce((sum, entry) => sum + entry.requestCount, 0)).toBe(
      summary.totals.requestCount,
    );
    expect(summary.dailyUsage.reduce((sum, entry) => sum + entry.totalTokens, 0)).toBe(
      summary.totals.totalTokens,
    );
  });
});
