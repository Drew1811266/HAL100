import { Activity, Bookmark, Bot, Boxes, Cable, CircleGauge, Settings } from "lucide-react";
import type { ComponentType, ReactNode } from "react";
import { NavLink, useLocation } from "react-router-dom";
import { isTauriRuntime } from "../../lib/desktop-api";

interface NavigationItem {
  label: string;
  path: string;
  icon: ComponentType<{ size?: number; strokeWidth?: number }>;
  activePrefixes?: string[];
}

const navigation: NavigationItem[] = [
  { label: "首页", path: "/", icon: CircleGauge },
  {
    label: "模型与运行",
    path: "/workspace/models",
    icon: Boxes,
    activePrefixes: ["/workspace", "/models", "/backends", "/test"],
  },
  { label: "软件接入", path: "/integrations", icon: Cable },
  { label: "Agent", path: "/agent", icon: Bot },
  {
    label: "活动",
    path: "/activity/usage",
    icon: Activity,
    activePrefixes: ["/activity", "/usage", "/audit"],
  },
  { label: "运行方案", path: "/profiles", icon: Bookmark },
];

function Sidebar({ setupRequired }: { setupRequired: boolean }) {
  const location = useLocation();

  return (
    <aside className="sidebar">
      <div className="traffic-lights" data-tauri-drag-region aria-hidden="true">
        {!isTauriRuntime() && (
          <>
            <span />
            <span />
            <span />
          </>
        )}
      </div>
      <div className="brand">
        <span className="brand-mark" aria-hidden="true">
          <img alt="" src="/hal100-logo.png" />
        </span>
        <div>
          <strong>HAL100</strong>
          <small>本地 AI 控制台</small>
        </div>
      </div>
      <nav className="navigation" aria-label="主导航">
        {navigation.map((item) => {
          const Icon = item.icon;
          const activePrefixes = item.activePrefixes ?? [item.path];
          const active = activePrefixes.some((prefix) =>
            prefix === "/"
              ? location.pathname === "/"
              : location.pathname === prefix || location.pathname.startsWith(`${prefix}/`),
          );
          return (
            <NavLink
              className={() => `nav-item${active ? " active" : ""}`}
              key={item.path}
              to={item.path}
            >
              <Icon size={17} strokeWidth={1.8} />
              <span>{item.label}</span>
            </NavLink>
          );
        })}
      </nav>
      <div className="sidebar-footer">
        <NavLink
          className={({ isActive }) => `nav-item${isActive ? " active" : ""}`}
          to="/settings"
        >
          <Settings size={17} strokeWidth={1.8} />
          <span>设置</span>
          {setupRequired && <i aria-hidden="true" className="nav-notice-dot" />}
        </NavLink>
      </div>
    </aside>
  );
}

export function AppShell({
  children,
  setupRequired,
}: {
  children: ReactNode;
  setupRequired: boolean;
}) {
  return (
    <div className="app-shell">
      <Sidebar setupRequired={setupRequired} />
      <main className="main-area">
        <div className="window-drag-region" data-tauri-drag-region aria-hidden="true" />
        {children}
      </main>
    </div>
  );
}
