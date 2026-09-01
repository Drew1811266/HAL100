import { describe, expect, it } from "vitest";
import type { AppOverview } from "../lib/desktop-api";
import { buildOverviewStatus } from "./status";

const readyOverview: AppOverview = {
  appName: "HAL100",
  version: "1.0.5",
  phase: "test",
  gatewayState: "运行中",
  databaseState: "已就绪",
  platform: { os: "macOS", architecture: "Apple Silicon", supported: true },
};

describe("overview presentation status", () => {
  it("prioritizes incomplete setup over normal runtime state", () => {
    expect(buildOverviewStatus(readyOverview, true)).toMatchObject({
      status: "attention",
      title: "基础设置尚未完成",
      actionPath: "/settings?setup=1",
    });
  });

  it("maps healthy core data to one ready summary", () => {
    expect(
      buildOverviewStatus(readyOverview, false, { engineInstalled: true, readyModelCount: 1 }),
    ).toMatchObject({
      status: "ready",
      title: "HAL100 已准备就绪",
      actionPath: "/integrations",
    });
  });

  it("prioritizes the first missing capability after the core is ready", () => {
    expect(
      buildOverviewStatus(readyOverview, false, { engineInstalled: false, readyModelCount: 0 }),
    ).toMatchObject({
      status: "attention",
      title: "核心已就绪，尚未添加模型",
      actionPath: "/workspace/models",
    });
    expect(
      buildOverviewStatus(readyOverview, false, { engineInstalled: false, readyModelCount: 1 }),
    ).toMatchObject({
      status: "attention",
      title: "模型已就绪，推理引擎尚未安装",
      actionPath: "/workspace/runtime",
    });
  });

  it("escalates an abnormal dependency to the diagnostic action", () => {
    expect(buildOverviewStatus({ ...readyOverview, gatewayState: "异常" }, false)).toMatchObject({
      status: "error",
      actionPath: "/agent",
    });
  });
});
