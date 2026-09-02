import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Drawer } from "./Drawer";
import { Modal } from "./Modal";

afterEach(() => {
  cleanup();
  document.body.innerHTML = "";
  document.body.style.overflow = "";
});

function renderInApplication(ui: React.ReactNode) {
  const applicationRoot = document.createElement("div");
  applicationRoot.id = "root";
  document.body.appendChild(applicationRoot);
  return {
    applicationRoot,
    ...render(ui, { container: applicationRoot }),
  };
}

describe("OverlayPortal", () => {
  it("locks the application, traps focus, closes with Escape, and restores focus", () => {
    function Harness() {
      const [open, setOpen] = useState(false);
      return (
        <>
          <button onClick={() => setOpen(true)} type="button">
            打开弹窗
          </button>
          {open && (
            <Modal onClose={() => setOpen(false)}>
              <section aria-label="确认操作" aria-modal="true" role="dialog">
                <button type="button">第一个操作</button>
                <button type="button">最后一个操作</button>
              </section>
            </Modal>
          )}
        </>
      );
    }

    const { applicationRoot } = renderInApplication(<Harness />);
    const opener = screen.getByRole("button", { name: "打开弹窗" });
    opener.focus();
    fireEvent.click(opener);

    const first = screen.getByRole("button", { name: "第一个操作" });
    const last = screen.getByRole("button", { name: "最后一个操作" });
    expect(applicationRoot).toHaveAttribute("inert");
    expect(document.body).toHaveStyle({ overflow: "hidden" });
    expect(first).toHaveFocus();

    first.focus();
    fireEvent.keyDown(window, { key: "Tab", shiftKey: true });
    expect(last).toHaveFocus();
    fireEvent.keyDown(window, { key: "Tab" });
    expect(first).toHaveFocus();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "确认操作" })).not.toBeInTheDocument();
    expect(applicationRoot).not.toHaveAttribute("inert");
    expect(document.body).not.toHaveStyle({ overflow: "hidden" });
    expect(opener).toHaveFocus();
  });

  it("allows only the topmost overlay to handle Escape and keeps the page locked", () => {
    const drawerClose = vi.fn();

    function Harness() {
      const [drawerOpen, setDrawerOpen] = useState(false);
      const [modalOpen, setModalOpen] = useState(false);
      return (
        <>
          <button onClick={() => setDrawerOpen(true)} type="button">
            打开抽屉
          </button>
          {drawerOpen && (
            <Drawer
              onClose={() => {
                drawerClose();
                setDrawerOpen(false);
              }}
              title="模型操作"
            >
              <button onClick={() => setModalOpen(true)} type="button">
                打开二级确认
              </button>
            </Drawer>
          )}
          {modalOpen && (
            <Modal onClose={() => setModalOpen(false)}>
              <section aria-label="二级确认" aria-modal="true" role="dialog">
                <button onClick={() => setModalOpen(false)} type="button">
                  取消
                </button>
              </section>
            </Modal>
          )}
        </>
      );
    }

    const { applicationRoot } = renderInApplication(<Harness />);
    const drawerOpener = screen.getByRole("button", { name: "打开抽屉" });
    drawerOpener.focus();
    fireEvent.click(drawerOpener);

    const modalOpener = screen.getByRole("button", { name: "打开二级确认" });
    modalOpener.focus();
    fireEvent.click(modalOpener);
    expect(screen.getByRole("dialog", { name: "二级确认" })).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "二级确认" })).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "模型操作" })).toBeInTheDocument();
    expect(drawerClose).not.toHaveBeenCalled();
    expect(applicationRoot).toHaveAttribute("inert");
    expect(modalOpener).toHaveFocus();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "模型操作" })).not.toBeInTheDocument();
    expect(drawerClose).toHaveBeenCalledTimes(1);
    expect(applicationRoot).not.toHaveAttribute("inert");
    expect(drawerOpener).toHaveFocus();
  });

  it("does not reset drawer focus when its parent rerenders with an inline onClose", () => {
    function Harness() {
      const [value, setValue] = useState("");
      return (
        <Drawer onClose={() => undefined} title="编辑配置">
          <label>
            名称
            <input onChange={(event) => setValue(event.target.value)} value={value} />
          </label>
        </Drawer>
      );
    }

    renderInApplication(<Harness />);
    const input = screen.getByRole("textbox", { name: "名称" });
    input.focus();
    fireEvent.change(input, { target: { value: "本地模型" } });

    expect(input).toHaveFocus();
    expect(input).toHaveValue("本地模型");
  });
});
