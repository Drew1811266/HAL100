import { cleanup, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { installStartupRecovery, showStartupFailure } from "./startup-recovery";

describe("startup recovery", () => {
  beforeEach(() => {
    document.documentElement.removeAttribute("data-hal100-ready");
    document.body.innerHTML = '<div id="root"><p>正在启动</p></div>';
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("replaces the loading shell with an actionable failure instead of a blank page", () => {
    showStartupFailure("module_load_failed");

    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "HAL100 界面加载失败" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "重新加载界面" })).toBeEnabled();
    expect(screen.getByText("诊断代码：module_load_failed")).toBeInTheDocument();
  });

  it("shows a timeout diagnosis when the application never becomes ready", () => {
    vi.useFakeTimers();
    const recovery = installStartupRecovery(50);

    vi.advanceTimersByTime(50);

    expect(screen.getByRole("heading", { name: "HAL100 界面启动超时" })).toBeInTheDocument();
    recovery.dispose();
  });

  it("does not replace a renderer that has already declared readiness", () => {
    const recovery = installStartupRecovery();
    recovery.markReady();
    document.getElementById("root")?.replaceChildren(document.createTextNode("HAL100 已准备就绪"));

    showStartupFailure("module_load_failed");

    expect(screen.getByText("HAL100 已准备就绪")).toBeInTheDocument();
  });
});
