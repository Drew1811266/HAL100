import { describe, expect, it } from "vitest";
import type { AppOverview } from "../lib/desktop-api";
import { buildOverviewStatus } from "./status";

const readyOverview: AppOverview = {
  appName: "HAL100",
  version: "1.0.6",
  phase: "test",
  gatewayState: "运行中",
  databaseState: "已就绪",
  platform: { os: "macOS", architecture: "Apple Silicon", supported: true },
};

describe("overview presentation status", () => {
  const emptyReadiness = {
    engineInstalled: false,
    readyModelCount: 0,
    managedModelRunning: false,
    configuredServiceCount: 0,
    activeInferenceName: null,
    activeInferenceReady: false,
  };

  it("maps an active inference path to one ready summary", () => {
    expect(
      buildOverviewStatus(readyOverview, {
        ...emptyReadiness,
        activeInferenceName: "本机 Ollama",
        activeInferenceReady: true,
        configuredServiceCount: 1,
      }),
    ).toMatchObject({
      status: "ready",
      title: "HAL100 已准备就绪",
      actionPath: "/integrations",
    });
  });

  it("prioritizes the first missing capability after the core is ready", () => {
    expect(buildOverviewStatus(readyOverview, emptyReadiness)).toMatchObject({
      status: "attention",
      title: "还没有可用的模型或服务",
      actionPath: "/workspace/services",
    });
    expect(
      buildOverviewStatus(readyOverview, {
        ...emptyReadiness,
        readyModelCount: 1,
      }),
    ).toMatchObject({
      status: "attention",
      title: "模型已就绪，运行环境尚未准备",
      actionPath: "/workspace/runtime",
    });
  });

  it("does not treat a configured but inactive service as ready", () => {
    expect(
      buildOverviewStatus(readyOverview, { ...emptyReadiness, configuredServiceCount: 1 }),
    ).toMatchObject({
      status: "attention",
      title: "推理服务已添加，尚未启用",
      actionPath: "/workspace/services",
    });
  });

  it("escalates an abnormal dependency to the diagnostic action", () => {
    expect(
      buildOverviewStatus({ ...readyOverview, gatewayState: "异常" }, emptyReadiness),
    ).toMatchObject({ status: "error", actionPath: "/agent" });
  });
});
