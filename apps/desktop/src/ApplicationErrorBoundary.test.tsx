import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ApplicationErrorBoundary } from "./ApplicationErrorBoundary";

function BrokenRenderer(): never {
  throw new Error("test renderer failure");
}

describe("ApplicationErrorBoundary", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("keeps renderer failures visible and offers a reload action", () => {
    const onReload = vi.fn();
    vi.spyOn(console, "error").mockImplementation(() => undefined);

    render(
      <ApplicationErrorBoundary onReload={onReload}>
        <BrokenRenderer />
      </ApplicationErrorBoundary>,
    );

    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "HAL100 界面发生异常" })).toBeInTheDocument();
    expect(screen.getByText("诊断代码：renderer_runtime_error")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "重新加载界面" }));
    expect(onReload).toHaveBeenCalledOnce();
  });
});
