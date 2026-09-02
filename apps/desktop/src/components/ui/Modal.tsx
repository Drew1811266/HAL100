import type { ReactNode } from "react";
import { OverlayPortal } from "./OverlayPortal";

export function Modal({
  children,
  closeDisabled = false,
  onClose,
}: {
  children: ReactNode;
  closeDisabled?: boolean;
  onClose: () => void;
}) {
  return (
    <OverlayPortal className="dialog-backdrop" closeDisabled={closeDisabled} onClose={onClose}>
      {children}
    </OverlayPortal>
  );
}
