import { useQuery } from "@tanstack/react-query";
import { Activity, Bot, Boxes, Cable, CircleGauge, Settings } from "lucide-react";
import { type ComponentType, type ReactNode, useEffect, useRef } from "react";
import { NavLink, useLocation } from "react-router-dom";
import {
  getAppOverview,
  getBackendCatalog,
  getLlamaCppStatus,
  isTauriRuntime,
} from "../../lib/desktop-api";

interface NavigationItem {
  label: string;
  path: string;
  icon: ComponentType<{ size?: number; strokeWidth?: number }>;
  activePrefixes?: string[];
}

interface ServiceState {
  label: string;
  tone: "attention" | "ready" | "neutral" | "warning";
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
];

const pageLabels: Array<{ prefix: string; label: string }> = [
  { prefix: "/workspace/models", label: "模型与运行 / 模型库" },
  { prefix: "/workspace/runtime", label: "模型与运行 / 本地运行" },
  { prefix: "/workspace/services", label: "模型与运行 / 连接服务" },
  { prefix: "/workspace/profiles", label: "模型与运行 / 快捷切换" },
  { prefix: "/integrations", label: "软件接入" },
  { prefix: "/agent", label: "Agent" },
  { prefix: "/activity/operations", label: "活动 / 操作记录" },
  { prefix: "/activity", label: "活动 / 用量" },
  { prefix: "/settings", label: "设置" },
  { prefix: "/", label: "首页" },
];

function Sidebar({
  serviceState,
  setupRequired,
}: {
  serviceState: ServiceState;
  setupRequired: boolean;
}) {
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
          <small>LOCAL AI CONTROL</small>
        </div>
      </div>
      <p className="navigation-label">工作区</p>
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
      <section className={`sidebar-state ${serviceState.tone}`} aria-label="HAL100 当前状态">
        <strong>
          <i aria-hidden="true" />
          {serviceState.label}
        </strong>
        <span>
          {setupRequired
            ? "选择一种使用方式即可开始"
            : serviceState.tone === "ready"
              ? "当前推理服务可以使用"
              : "打开首页查看推荐下一步"}
        </span>
      </section>
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
  const location = useLocation();
  const mainAreaRef = useRef<HTMLElement>(null);
  const overview = useQuery({
    queryKey: ["app-overview"],
    queryFn: getAppOverview,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  const runtime = useQuery({
    queryKey: ["llama-cpp-status"],
    queryFn: getLlamaCppStatus,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  const backends = useQuery({
    queryKey: ["backend-catalog"],
    queryFn: getBackendCatalog,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  const pageLabel =
    pageLabels.find(({ prefix }) =>
      prefix === "/" ? location.pathname === "/" : location.pathname.startsWith(prefix),
    )?.label ?? "HAL100";
  const activeBackend = backends.data?.backends.find((backend) => backend.isActive);
  const inferenceReady = Boolean(
    runtime.data?.runtimeState === "running" ||
      (activeBackend?.enabled &&
        activeBackend.runtimeAvailable &&
        !activeBackend.circuitOpen &&
        (activeBackend.authMethod === "none" || activeBackend.credentialConfigured)),
  );
  const coreReady =
    overview.data?.gatewayState === "运行中" && overview.data.databaseState === "已就绪";
  const serviceState: ServiceState = setupRequired
    ? { label: "等待首次设置", tone: "attention" }
    : inferenceReady
      ? { label: "推理服务可用", tone: "ready" }
      : coreReady
        ? { label: "HAL100 运行正常", tone: "neutral" }
        : overview.isPending
          ? { label: "正在读取状态", tone: "neutral" }
          : { label: "本地服务需检查", tone: "warning" };

  useEffect(() => {
    if (location.pathname) {
      if (typeof mainAreaRef.current?.scrollTo === "function") {
        mainAreaRef.current.scrollTo({ top: 0, left: 0 });
      }
    }
  }, [location.pathname]);

  return (
    <div className="app-shell">
      <Sidebar serviceState={serviceState} setupRequired={setupRequired} />
      <header className="app-topbar" data-tauri-drag-region>
        <span className="app-breadcrumb">{pageLabel}</span>
        <span className={`app-service-state ${serviceState.tone}`}>
          <i aria-hidden="true" />
          {serviceState.label}
        </span>
      </header>
      <main className="main-area" ref={mainAreaRef}>
        {children}
      </main>
    </div>
  );
}
