import { X } from "lucide-react";
import { type ReactNode, useId, useRef } from "react";
import { OverlayPortal } from "./OverlayPortal";

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

  return (
    <OverlayPortal className="drawer-backdrop" initialFocusRef={closeButton} onClose={onClose}>
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
    </OverlayPortal>
  );
}
