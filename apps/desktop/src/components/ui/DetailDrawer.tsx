import type { ReactNode } from "react";

export function DetailDrawer({
  children,
  className,
  summary,
}: {
  children: ReactNode;
  className?: string;
  summary: string;
}) {
  return (
    <details className={className}>
      <summary>{summary}</summary>
      {children}
    </details>
  );
}
