import type { ReactNode } from "react";
import { NavLink } from "react-router-dom";
import { PageHeader } from "../../components/ui/PageHeader";

const activityTabs = [
  { label: "用量", path: "/activity/usage" },
  { label: "操作记录", path: "/activity/operations" },
];

export function ActivityPageShell({
  action,
  children,
  description,
  title,
}: {
  action?: ReactNode;
  children: ReactNode;
  description: string;
  title: string;
}) {
  return (
    <div className="page-content activity-page">
      <PageHeader action={action} description={description} title={title} />
      <nav aria-label="活动" className="section-tabs">
        {activityTabs.map((tab) => (
          <NavLink
            className={({ isActive }) => (isActive ? "active" : undefined)}
            key={tab.path}
            to={tab.path}
          >
            {tab.label}
          </NavLink>
        ))}
      </nav>
      {children}
    </div>
  );
}
