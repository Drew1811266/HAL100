import { X } from "lucide-react";
import { type ReactNode, useEffect, useId, useRef } from "react";
import { createPortal } from "react-dom";

export function Drawer({
  children,
  description,
  eyebrow,
  onClose,
  title,
}: {
  children: ReactNode;
  description?: string;
  eyebrow?: string;
  onClose: () => void;
  title: string;
}) {
  const titleId = useId();
  const closeButton = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLElement>(null);

  useEffect(() => {
    const previousOverflow = document.body.style.overflow;
    const applicationRoot = document.getElementById("root");
    const rootWasInert = applicationRoot?.hasAttribute("inert") ?? false;
    const previouslyFocused =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const keepFocusInside = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
        return;
      }
      if (event.key !== "Tab" || !panel.current) return;
      const focusable = [
        ...panel.current.querySelectorAll<HTMLElement>(
          'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), details > summary, [tabindex]:not([tabindex="-1"])',
        ),
      ].filter((element) => !element.hasAttribute("hidden") && element.getClientRects().length > 0);
      if (focusable.length === 0) {
        event.preventDefault();
        panel.current.focus();
        return;
      }
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.body.style.overflow = "hidden";
    applicationRoot?.setAttribute("inert", "");
    window.addEventListener("keydown", keepFocusInside);
    closeButton.current?.focus();
    return () => {
      document.body.style.overflow = previousOverflow;
      if (!rootWasInert) applicationRoot?.removeAttribute("inert");
      window.removeEventListener("keydown", keepFocusInside);
      previouslyFocused?.focus();
    };
  }, [onClose]);

  return createPortal(
    <div className="drawer-backdrop">
      <button
        aria-hidden="true"
        className="drawer-scrim"
        onClick={onClose}
        tabIndex={-1}
        type="button"
      />
      <section
        aria-labelledby={titleId}
        aria-modal="true"
        className="drawer-panel"
        ref={panel}
        role="dialog"
        tabIndex={-1}
      >
        <header className="drawer-header">
          <div>
            {eyebrow && <p className="eyebrow">{eyebrow}</p>}
            <h2 id={titleId}>{title}</h2>
            {description && <p>{description}</p>}
          </div>
          <button
            aria-label={`关闭${title}`}
            className="icon-button"
            onClick={onClose}
            ref={closeButton}
            type="button"
          >
            <X size={17} />
          </button>
        </header>
        <div className="drawer-content">{children}</div>
      </section>
    </div>,
    document.body,
  );
}
