export type StartupFailureCode = "module_load_failed" | "startup_timeout";

const STARTUP_TIMEOUT_MS = 12_000;

const failureCopy: Record<StartupFailureCode, { title: string; detail: string }> = {
  module_load_failed: {
    title: "HAL100 界面加载失败",
    detail: "界面资源未能完成加载。本地核心不会因此执行额外操作，可以重新加载界面后再试。",
  },
  startup_timeout: {
    title: "HAL100 界面启动超时",
    detail: "界面长时间没有完成初始化。本地核心仍保持独立运行，可以重新加载界面后再试。",
  },
};

function recoveryView(code: StartupFailureCode) {
  const copy = failureCopy[code];
  const shell = document.createElement("main");
  shell.className = "startup-failure";
  shell.setAttribute("role", "alert");

  const card = document.createElement("section");
  card.className = "startup-failure-card";

  const badge = document.createElement("span");
  badge.className = "startup-failure-badge";
  badge.textContent = "!";
  badge.setAttribute("aria-hidden", "true");

  const content = document.createElement("div");
  const title = document.createElement("h1");
  title.textContent = copy.title;
  const detail = document.createElement("p");
  detail.textContent = copy.detail;
  const diagnostic = document.createElement("code");
  diagnostic.textContent = `诊断代码：${code}`;
  const reload = document.createElement("button");
  reload.type = "button";
  reload.textContent = "重新加载界面";
  reload.addEventListener("click", () => window.location.reload());

  content.append(title, detail, diagnostic, reload);
  card.append(badge, content);
  shell.append(card);
  return shell;
}

export function showStartupFailure(code: StartupFailureCode) {
  if (document.documentElement.dataset.hal100Ready === "true") {
    return;
  }
  const root = document.getElementById("root");
  root?.replaceChildren(recoveryView(code));
}

export function installStartupRecovery(timeoutMs = STARTUP_TIMEOUT_MS) {
  const fail = (code: StartupFailureCode) => showStartupFailure(code);
  const onError = () => fail("module_load_failed");
  const onUnhandledRejection = () => fail("module_load_failed");
  window.addEventListener("error", onError);
  window.addEventListener("unhandledrejection", onUnhandledRejection);
  const timeout = window.setTimeout(() => fail("startup_timeout"), timeoutMs);

  return {
    fail,
    markReady() {
      document.documentElement.dataset.hal100Ready = "true";
      window.clearTimeout(timeout);
      window.removeEventListener("error", onError);
      window.removeEventListener("unhandledrejection", onUnhandledRejection);
    },
    dispose() {
      window.clearTimeout(timeout);
      window.removeEventListener("error", onError);
      window.removeEventListener("unhandledrejection", onUnhandledRejection);
    },
  };
}
