import type { ReactNode } from "react";

export function PageHeader({
  action,
  className,
  description,
  eyebrow,
  title,
}: {
  action?: ReactNode;
  className?: string;
  description: string;
  eyebrow?: string;
  title: string;
}) {
  return (
    <header className={`page-header${className ? ` ${className}` : ""}`}>
      <div>
        {eyebrow && <p className="eyebrow">{eyebrow}</p>}
        <h1>{title}</h1>
        <p>{description}</p>
      </div>
      {action}
    </header>
  );
}
