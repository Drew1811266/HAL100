import { type ReactNode, type RefObject, useEffect, useRef } from "react";
import { createPortal } from "react-dom";

const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "details > summary",
  '[tabindex]:not([tabindex="-1"])',
].join(", ");

const overlayStack: HTMLElement[] = [];
let pageLockCount = 0;
let previousBodyOverflow = "";
let rootWasInert = false;

function lockApplication() {
  const applicationRoot = document.getElementById("root");
  if (pageLockCount === 0) {
    previousBodyOverflow = document.body.style.overflow;
    rootWasInert = applicationRoot?.hasAttribute("inert") ?? false;
    document.body.style.overflow = "hidden";
    applicationRoot?.setAttribute("inert", "");
  }
  pageLockCount += 1;
}

function unlockApplication() {
  pageLockCount = Math.max(0, pageLockCount - 1);
  if (pageLockCount > 0) return;

  document.body.style.overflow = previousBodyOverflow;
  const applicationRoot = document.getElementById("root");
  if (!rootWasInert) applicationRoot?.removeAttribute("inert");
}

function isFocusable(element: HTMLElement) {
  if (element.hidden || element.closest('[hidden], [aria-hidden="true"]')) return false;
  const style = window.getComputedStyle(element);
  if (style.display === "none" || style.visibility === "hidden") return false;
  return element.getClientRects().length > 0 || import.meta.env.MODE === "test";
}

function getFocusableElements(container: HTMLElement) {
  return [...container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)].filter(isFocusable);
}

export function OverlayPortal({
  children,
  className,
  closeDisabled = false,
  initialFocusRef,
  onClose,
}: {
  children: ReactNode;
  className: string;
  closeDisabled?: boolean;
  initialFocusRef?: RefObject<HTMLElement | null>;
  onClose: () => void;
}) {
  const overlay = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  const closeDisabledRef = useRef(closeDisabled);
  const initialFocusRefRef = useRef(initialFocusRef);

  onCloseRef.current = onClose;
  closeDisabledRef.current = closeDisabled;
  initialFocusRefRef.current = initialFocusRef;

  useEffect(() => {
    const currentOverlay = overlay.current;
    if (!currentOverlay) return;

    const previouslyFocused =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    overlayStack.push(currentOverlay);
    lockApplication();

    const handleKeyDown = (event: KeyboardEvent) => {
      if (overlayStack.at(-1) !== currentOverlay) return;

      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        if (!closeDisabledRef.current) onCloseRef.current();
        return;
      }

      if (event.key !== "Tab") return;
      const focusable = getFocusableElements(currentOverlay);
      if (focusable.length === 0) {
        event.preventDefault();
        currentOverlay.focus();
        return;
      }

      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (
        event.shiftKey &&
        (document.activeElement === first || document.activeElement === currentOverlay)
      ) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };

    window.addEventListener("keydown", handleKeyDown, true);
    const initialFocus =
      initialFocusRefRef.current?.current ??
      getFocusableElements(currentOverlay)[0] ??
      currentOverlay;
    initialFocus.focus();

    return () => {
      window.removeEventListener("keydown", handleKeyDown, true);
      const overlayIndex = overlayStack.lastIndexOf(currentOverlay);
      if (overlayIndex >= 0) overlayStack.splice(overlayIndex, 1);
      unlockApplication();
      if (previouslyFocused?.isConnected) previouslyFocused.focus();
    };
  }, []);

  return createPortal(
    <div className={className} ref={overlay} tabIndex={-1}>
      {children}
    </div>,
    document.body,
  );
}
