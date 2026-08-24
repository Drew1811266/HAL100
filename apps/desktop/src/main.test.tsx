import { cleanup, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mountApplication } from "./main";

vi.mock("./App", () => ({
  default: () => <main>HAL100 测试界面</main>,
}));

describe("application mount", () => {
  beforeEach(() => {
    document.body.innerHTML = '<div id="root"></div>';
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("declares startup readiness after React commits without waiting for animation frames", async () => {
    const onReady = vi.fn();
    const animationFrame = vi.spyOn(window, "requestAnimationFrame");
    const root = mountApplication(onReady);

    expect(await screen.findByText("HAL100 测试界面")).toBeInTheDocument();
    await waitFor(() => expect(onReady).toHaveBeenCalled());
    expect(animationFrame).not.toHaveBeenCalled();

    root.unmount();
  });
});
