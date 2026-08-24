import { useQuery } from "@tanstack/react-query";
import { lazy, Suspense, useEffect, useState } from "react";
import { Navigate, Route, Routes } from "react-router-dom";
import { AppShell } from "./components/layout/AppShell";
import { getDesktopSettings, type ModelDownloadSnapshot } from "./lib/desktop-api";

const OverviewPage = lazy(() =>
  import("./features/home/OverviewPage").then((module) => ({ default: module.OverviewPage })),
);
const ModelsPage = lazy(() =>
  import("./features/workspace/ModelsPage").then((module) => ({ default: module.ModelsPage })),
);
const BackendsPage = lazy(() =>
  import("./features/workspace/BackendsPage").then((module) => ({
    default: module.BackendsPage,
  })),
);
const ModelTestPage = lazy(() =>
  import("./features/workspace/BackendsPage").then((module) => ({
    default: module.ModelTestPage,
  })),
);
const IntegrationsPage = lazy(() =>
  import("./features/integrations/IntegrationsPage").then((module) => ({
    default: module.IntegrationsPage,
  })),
);
const AgentPage = lazy(() =>
  import("./features/agent/AgentPage").then((module) => ({ default: module.AgentPage })),
);
const UsagePage = lazy(() => import("./features/activity/UsagePage"));
const AuditPage = lazy(() => import("./features/activity/AuditPage"));
const SettingsPage = lazy(() =>
  import("./features/settings/SettingsPage").then((module) => ({
    default: module.SettingsPage,
  })),
);

const activeDownloadStates = new Set(["pending", "downloading", "verifying", "installing"]);

export function modelDownloadPollingInterval(
  windowActive: boolean,
  downloads?: ModelDownloadSnapshot[],
): number | false {
  return windowActive && downloads?.some((download) => activeDownloadStates.has(download.state))
    ? 500
    : false;
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

function getInitialDarkMode(): boolean {
  const storedTheme = window.localStorage.getItem("hal100-theme");
  if (storedTheme === "dark") return true;
  if (storedTheme === "light") return false;
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false;
}

export default function App() {
  const [darkMode, setDarkMode] = useState(getInitialDarkMode);
  const desktopSettings = useQuery({
    queryKey: ["desktop-settings"],
    queryFn: getDesktopSettings,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });

  useEffect(() => {
    document.documentElement.dataset.theme = darkMode ? "dark" : "light";
    window.localStorage.setItem("hal100-theme", darkMode ? "dark" : "light");
  }, [darkMode]);

  if (desktopSettings.isPending) {
    return <div className="app-bootstrap-state">正在连接 HAL100 Core…</div>;
  }
  if (desktopSettings.isError) {
    return <div className="app-bootstrap-state error">{errorMessage(desktopSettings.error)}</div>;
  }

  return (
    <AppShell setupRequired={!desktopSettings.data.onboardingCompleted}>
      <Suspense fallback={<div className="route-loading-state">正在打开页面…</div>}>
        <Routes>
          <Route
            path="/"
            element={<OverviewPage setupRequired={!desktopSettings.data.onboardingCompleted} />}
          />
          <Route path="/workspace" element={<Navigate replace to="/workspace/models" />} />
          <Route path="/workspace/models" element={<ModelsPage />} />
          <Route path="/workspace/runtime" element={<BackendsPage view="runtime" />} />
          <Route path="/workspace/services" element={<BackendsPage view="services" />} />
          <Route path="/workspace/test" element={<ModelTestPage />} />
          <Route path="/integrations" element={<IntegrationsPage />} />
          <Route path="/agent" element={<AgentPage />} />
          <Route path="/activity" element={<Navigate replace to="/activity/usage" />} />
          <Route path="/activity/usage" element={<UsagePage />} />
          <Route path="/activity/operations" element={<AuditPage />} />
          <Route path="/models" element={<Navigate replace to="/workspace/models" />} />
          <Route path="/backends" element={<Navigate replace to="/workspace/runtime" />} />
          <Route path="/test" element={<Navigate replace to="/workspace/test" />} />
          <Route path="/usage" element={<Navigate replace to="/activity/usage" />} />
          <Route path="/audit" element={<Navigate replace to="/activity/operations" />} />
          <Route
            path="/settings"
            element={
              <SettingsPage
                darkMode={darkMode}
                onToggleTheme={() => setDarkMode((value) => !value)}
              />
            }
          />
        </Routes>
      </Suspense>
    </AppShell>
  );
}
