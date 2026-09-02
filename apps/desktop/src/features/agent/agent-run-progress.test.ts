import { describe, expect, it } from "vitest";
import {
  AGENT_MODEL_START_TIMEOUT_SECONDS,
  AGENT_RESPONSE_TIMEOUT_SECONDS,
  getAgentRunProgress,
} from "./agent-run-progress";

describe("getAgentRunProgress", () => {
  it("shows the native confirmation boundary before a cloud run exists", () => {
    const progress = getAgentRunProgress({
      activeRunId: null,
      elapsedSeconds: 2,
      kernelState: "stopped",
      modelRuntimeState: "stopped",
      providerMode: "cloud-single",
    });

    expect(progress.title).toBe("等待原生窗口确认");
    expect(progress.description).toContain("不会发送任务内容");
    expect(progress.slow).toBe(false);
  });

  it("distinguishes local cold start from a running Agent kernel", () => {
    const coldStart = getAgentRunProgress({
      activeRunId: "run-1",
      elapsedSeconds: 12,
      kernelState: "starting",
      modelRuntimeState: "starting",
      providerMode: "local",
    });
    const executing = getAgentRunProgress({
      activeRunId: "run-1",
      elapsedSeconds: 18,
      kernelState: "running",
      modelRuntimeState: "running",
      providerMode: "local",
    });

    expect(coldStart.title).toContain("启动本地模型");
    expect(coldStart.description).toContain(`${AGENT_MODEL_START_TIMEOUT_SECONDS} 秒`);
    expect(executing.title).toContain("正在理解并执行任务");
  });

  it("turns a long execution into an honest slow-task message with its timeout boundary", () => {
    const progress = getAgentRunProgress({
      activeRunId: "run-2",
      elapsedSeconds: 67,
      kernelState: "running",
      modelRuntimeState: "running",
      providerMode: "local",
    });

    expect(progress.slow).toBe(true);
    expect(progress.title).toContain("复杂任务");
    expect(progress.description).toContain("已等待 67 秒");
    expect(progress.description).toContain(`${AGENT_RESPONSE_TIMEOUT_SECONDS / 60} 分钟`);
    expect(progress.description).toContain("随时取消");
  });
});
