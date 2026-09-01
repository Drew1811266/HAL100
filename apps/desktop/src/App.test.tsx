import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import App, { modelDownloadPollingInterval } from "./App";

describe("HAL100 application shell", () => {
  afterEach(cleanup);

  beforeEach(() => {
    window.localStorage.clear();
    window.history.replaceState(null, "", "/");
    document.documentElement.removeAttribute("data-theme");
  });

  it("polls downloads only while the window is active and work is progressing", () => {
    const activeDownload = {
      downloadId: "download-1",
      source: "huggingFace" as const,
      repository: "Qwen/Qwen3.5-2B",
      fileName: "model.gguf",
      state: "downloading" as const,
      downloadedBytes: 1,
      expectedSizeBytes: 2,
      errorCode: null,
      canResume: false,
      model: null,
    };

    expect(modelDownloadPollingInterval(true, [activeDownload])).toBe(500);
    expect(modelDownloadPollingInterval(false, [activeDownload])).toBe(false);
    expect(modelDownloadPollingInterval(true, [{ ...activeDownload, state: "ready" }])).toBe(false);
    expect(modelDownloadPollingInterval(true, [])).toBe(false);
  });

  it("renders the accepted overview information architecture", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    expect(await screen.findByRole("heading", { name: "今天需要处理什么" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "核心已就绪，尚未添加模型" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "添加模型" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "添加第一个模型" })).not.toBeInTheDocument();
    expect(screen.getByRole("region", { name: "当前状态" })).toBeInTheDocument();

    const mainNavigation = screen.getByRole("navigation", { name: "主导航" });
    expect(within(mainNavigation).getAllByRole("link")).toHaveLength(6);
    for (const name of ["首页", "模型与运行", "软件接入", "Agent", "活动", "运行方案"]) {
      expect(within(mainNavigation).getByRole("link", { name })).toBeInTheDocument();
    }
    expect(
      within(mainNavigation).queryByRole("link", { name: "Token 统计" }),
    ).not.toBeInTheDocument();
  });

  it("persists an explicit appearance choice", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={["/settings"]}>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "切换为深色" }));
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    expect(window.localStorage.getItem("hal100-theme")).toBe("dark");
    expect(screen.getByRole("button", { name: "切换为浅色" })).toBeInTheDocument();
  });

  it("exposes runtime profiles as a dedicated primary navigation destination", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={["/profiles"]}>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    expect(await screen.findByRole("heading", { name: "运行方案", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "保存当前方案" })).toBeDisabled();
    expect(screen.getByText("还没有保存运行方案")).toBeInTheDocument();
    expect(await screen.findByText("已识别外部运行身份")).toBeInTheDocument();
    expect(screen.getByText("1 个可验证候选")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "保存为方案" }));
    expect(screen.getByRole("dialog", { name: "保存为运行方案" })).toBeInTheDocument();
    expect(screen.getByText(/只保存后端身份、引擎版本和类型化模型证据/)).toBeInTheDocument();
    const mainNavigation = screen.getByRole("navigation", { name: "主导航" });
    expect(within(mainNavigation).getByRole("link", { name: "运行方案" })).toHaveClass("active");
    expect(within(mainNavigation).getByRole("link", { name: "模型与运行" })).not.toHaveClass(
      "active",
    );
    expect(screen.queryByRole("navigation", { name: "模型与运行" })).not.toBeInTheDocument();
  });

  it("requires an explicit confirmation after showing the OpenCode semantic diff", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={["/integrations"]}>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "配置接入" }));
    const openCodeDrawer = await screen.findByRole("dialog", { name: "OpenCode" });
    fireEvent.click(within(openCodeDrawer).getByRole("button", { name: "配置 OpenCode" }));
    expect(await screen.findByRole("dialog", { name: "配置 OpenCode" })).toBeInTheDocument();
    expect(screen.getByText("+ provider.hal100.options.baseURL")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "确认并应用配置" })).toBeDisabled();
    expect(screen.getByText("浏览器预览模式只能查看变更，不能应用。")).toBeInTheDocument();
  });

  it("distinguishes the built-in Agent runtime from external Agent integrations", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={["/integrations"]}>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "了解运行边界" }));
    expect(await screen.findByRole("dialog", { name: "内置与外部相互独立" })).toBeInTheDocument();
    expect(await screen.findByText("HAL100 Agent（内置）")).toBeInTheDocument();
    expect(await screen.findByText(/固定版本 Pi Agent Core/)).toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: "Pi Coding Agent" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "OpenClaw" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Hermes Agent" })).toBeInTheDocument();
    expect(screen.queryByText("规划接入")).not.toBeInTheDocument();
    const accessButtons = screen.getAllByRole("button", { name: "查看接入方式" });
    fireEvent.click(accessButtons[accessButtons.length - 1]);
    const hermesDrawer = await screen.findByRole("dialog", { name: "Hermes Agent" });
    expect(within(hermesDrawer).getByRole("button", { name: "配置 Hermes" })).toBeDisabled();
    expect(screen.getByText(/Hermes ≥ 0.18.2/)).toBeInTheDocument();
  });

  it("shows the enabled OpenAI Responses and Anthropic Messages endpoints", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={["/integrations"]}>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "管理客户端" }));
    const clientDrawer = await screen.findByRole("dialog", { name: "其他客户端" });
    expect(
      within(clientDrawer).getByText("/v1/chat/completions · /v1/responses"),
    ).toBeInTheDocument();
    expect(screen.getByText("/v1/messages")).toBeInTheDocument();
    expect(screen.getByText(/支持 x-api-key、SSE和缓存 Usage/)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "OpenAI / Anthropic 客户端" })).toBeInTheDocument();
    expect(
      await screen.findByText("尚未签发通用客户端 Key。OpenCode 专属凭据不会显示在这里。"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "生成独立 Key" })).toBeDisabled();
    expect(screen.queryByText("协议适配将在后续迭代启用。")).not.toBeInTheDocument();
  });

  it("renders real audit and settings pages without background polling controls", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    const { unmount } = render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={["/audit"]}>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    expect(await screen.findByRole("heading", { name: "操作记录" })).toBeInTheDocument();
    expect(screen.getByText(/查看最近 50 条/)).toBeInTheDocument();
    expect(screen.getByText("尚无受控操作记录")).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "操作记录筛选" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "筛选" })).toBeDisabled();
    expect(screen.getByRole("link", { name: "前往模型库" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "刷新" })).toBeEnabled();
    unmount();

    const settingsClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={settingsClient}>
        <MemoryRouter initialEntries={["/settings"]}>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    expect(await screen.findByRole("heading", { name: "设置" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "初始化配置中心" })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "下载与启动" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "HAL100" })).toBeInTheDocument();
    expect(await screen.findByText("v1.0.5")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "打开 Agent 诊断" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "已关闭" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "保存保留策略" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "按策略清理" })).toBeDisabled();
  });

  it("moves first-run setup into the global settings center", async () => {
    window.localStorage.setItem("hal100-preview-onboarding", "incomplete");
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    expect(await screen.findByRole("heading", { name: "今天需要处理什么" })).toBeInTheDocument();
    expect(screen.getByText("基础设置尚未完成")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("link", { name: "前往设置" }));
    expect(await screen.findByRole("heading", { name: "设置" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "初始化配置中心" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "模型与运行" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("link", { name: "首页" }));
    expect(await screen.findByRole("heading", { name: "今天需要处理什么" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("link", { name: "前往设置" }));
    expect(await screen.findByRole("heading", { name: "初始化配置中心" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "完成设置" })).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "ModelScope" }));
    expect(await screen.findByText(/Apple M1（浏览器预览）/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "保持关闭" }));
    expect(screen.getByLabelText("基础设置完成 2 / 2 项")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "完成设置" }));
    expect(
      await screen.findByRole("heading", { name: "核心已就绪，尚未添加模型" }),
    ).toBeInTheDocument();
    expect(window.localStorage.getItem("hal100-preview-onboarding")).toBeNull();
  });

  it("keeps model discovery and hardware guidance inside the add-model task", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={["/models"]}>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    expect(await screen.findByRole("heading", { name: "模型库" })).toBeInTheDocument();
    expect(screen.queryByText("Apple M1（浏览器预览）")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "添加模型" }));
    const addModelDrawer = await screen.findByRole("dialog", { name: "添加模型" });
    expect(addModelDrawer.parentElement?.parentElement).toBe(document.body);
    expect(within(addModelDrawer).getByText("Apple M1（浏览器预览）")).toBeInTheDocument();
    fireEvent.change(within(addModelDrawer).getByLabelText("搜索来源"), {
      target: { value: "modelScope" },
    });

    fireEvent.change(within(addModelDrawer).getByRole("searchbox", { name: "模型名称或仓库" }), {
      target: { value: "Qwen3 GGUF" },
    });
    fireEvent.click(within(addModelDrawer).getByRole("button", { name: "搜索模型" }));
    expect(await screen.findByText("Qwen3.5-2B-GGUF")).toBeInTheDocument();
    expect(screen.getByText("ModelScope 返回 1 个结果")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "查看 GGUF 文件" }));
    expect(await screen.findByText("Q4_K_M")).toBeInTheDocument();
    expect(screen.getAllByText("可校验 SHA-256")).toHaveLength(2);

    fireEvent.click(screen.getByRole("button", { name: "下载 Q4_K_M" }));
    const downloadDialog = await screen.findByRole("dialog", { name: "下载并安装模型" });
    expect(within(downloadDialog).getByText("当前可用")).toBeInTheDocument();
    expect(within(downloadDialog).getByText("发布仓库")).toBeInTheDocument();
    expect(within(downloadDialog).getByText("unsloth/Qwen3.5-2B-GGUF")).toBeInTheDocument();
    expect(within(downloadDialog).getByText(/不代表原模型作者官方发布/)).toBeInTheDocument();
    expect(within(downloadDialog).getByRole("button", { name: "确认下载并安装" })).toBeDisabled();
    fireEvent.click(within(downloadDialog).getByRole("button", { name: "关闭" }));

    fireEvent.click(within(addModelDrawer).getByRole("button", { name: "选择 GGUF 文件" }));
    expect(await screen.findByRole("dialog", { name: "导入外部 GGUF" })).toBeInTheDocument();
    expect(
      screen.getByText(
        "只在 HAL100 中建立外部模型索引；不复制、不移动、不删除源文件。确认时会再次检查路径、大小、修改时间、GGUF 头和完整 SHA-256。",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "确认导入外部模型" })).toBeDisabled();
  });

  it("opens an exact owner/repository without a broad catalog search", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={["/models"]}>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    const repository = "unsloth/Qwen3.5-2B-GGUF";
    fireEvent.click(await screen.findByRole("button", { name: "添加模型" }));
    const addModelDrawer = await screen.findByRole("dialog", { name: "添加模型" });
    fireEvent.change(within(addModelDrawer).getByRole("searchbox", { name: "模型名称或仓库" }), {
      target: { value: repository },
    });
    fireEvent.click(within(addModelDrawer).getByRole("button", { name: "搜索模型" }));

    expect(await screen.findByText(repository)).toBeInTheDocument();
    expect(screen.getByText("Qwen3.5-2B-Q4_K_M.gguf")).toBeInTheDocument();
    expect(screen.queryByText(/返回 1 个结果/)).not.toBeInTheDocument();
  });

  it("separates local runtime from inference-service configuration", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    const { unmount } = render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={["/workspace/runtime"]}>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    expect(await screen.findByText("llama.cpp")).toBeInTheDocument();
    expect(screen.getByText("未安装")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "运行" })).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "Gateway 路由与模型别名" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("强制操作")).not.toBeInTheDocument();
    expect(screen.getByText("先准备推理引擎")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "安装 llama.cpp" }));
    const dialog = await screen.findByRole("dialog", { name: "安装 llama.cpp" });
    expect(within(dialog).getByText(/ggml-org\/llama.cpp GitHub Releases/)).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "确认安装" })).toBeDisabled();
    expect(
      within(dialog).getByText("浏览器预览模式只能查看计划，不能执行操作。"),
    ).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "取消" }));

    unmount();
    const serviceClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={serviceClient}>
        <MemoryRouter initialEntries={["/workspace/services"]}>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    expect(await screen.findByRole("heading", { name: "推理服务" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "默认服务与模型名称映射" })).toBeInTheDocument();
    const serviceHeading = screen.getByRole("heading", { name: "已配置后端" });
    const routingHeading = screen.getByRole("heading", {
      name: "默认服务与模型名称映射",
    });
    expect(
      serviceHeading.compareDocumentPosition(routingHeading) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(screen.getByText(/`hal100-active` 始终指向当前活动后端/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "添加推理服务" }));
    const serviceDrawer = await screen.findByRole("dialog", { name: "添加推理服务" });
    fireEvent.click(within(serviceDrawer).getByRole("button", { name: "开始发现" }));
    const discovery = await screen.findByRole("region", { name: "本机后端发现结果" });
    expect(within(discovery).getByText("本机 Ollama（预览示例）")).toBeInTheDocument();
    expect(within(discovery).getByText(/不会常驻监测/)).toBeInTheDocument();

    fireEvent.click(within(serviceDrawer).getByRole("button", { name: "手动填写配置" }));
    const backendEditor = await screen.findByRole("dialog", { name: "添加外部后端" });
    expect(within(backendEditor).getByText(/API Key 只写入 macOS Keychain/)).toBeInTheDocument();
    expect(within(backendEditor).getByRole("button", { name: "保存后端" })).toBeDisabled();
  });

  it("renders the exact-usage dashboard without background polling", async () => {
    window.history.replaceState(null, "", "/?preview=usage");
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={["/usage"]}>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    expect(await screen.findByRole("heading", { name: "用量" })).toBeInTheDocument();
    const summary = screen.getByRole("region", { name: "当前范围用量摘要" });
    expect(within(summary).getByText("请求数")).toBeInTheDocument();
    expect(within(summary).getByRole("region", { name: "当前范围主要客户端" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Token 变化" })).toBeInTheDocument();
    const tokenLegend = screen.getByRole("group", {
      name: "点击可显示或隐藏 Token 类型",
    });
    expect(within(tokenLegend).getByText("输入（缓存命中）")).toBeInTheDocument();
    expect(within(tokenLegend).getByText("输入（缓存未命中）")).toBeInTheDocument();
    expect(within(tokenLegend).getByText("输出")).toBeInTheDocument();
    const rangeSwitch = screen.getByRole("group", { name: "用量时间粒度" });
    for (const range of ["年", "月", "周", "天"]) {
      expect(within(rangeSwitch).getByRole("button", { name: range })).toBeInTheDocument();
    }
    expect(within(rangeSwitch).getByRole("button", { name: "月" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    fireEvent.click(within(rangeSwitch).getByRole("button", { name: "年" }));
    expect((await screen.findAllByText(/按月汇总/)).length).toBeGreaterThan(0);

    expect(screen.getByRole("heading", { name: "每日请求活动" })).toBeInTheDocument();
    const heatmap = screen.getByRole("region", { name: "过去 365 天每日请求活动" });
    expect(
      within(heatmap).getByRole("table", { name: "过去 365 天每日请求格子" }),
    ).toBeInTheDocument();
    expect(within(heatmap).getAllByRole("button")).toHaveLength(365);
    const activeDate = within(heatmap)
      .getAllByRole("button")
      .find((button) => /· [1-9]\d* 次请求/.test(button.getAttribute("aria-label") ?? ""));
    expect(activeDate).toBeDefined();
    const activeDateLabel = (activeDate as HTMLButtonElement).getAttribute("aria-label") ?? "";
    fireEvent.click(activeDate as HTMLButtonElement);
    expect(screen.queryByText("正在读取本机 Token 统计…")).not.toBeInTheDocument();
    const dayRangeSwitch = await screen.findByRole("group", { name: "用量时间粒度" });
    expect(within(dayRangeSwitch).getByRole("button", { name: "天" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect((await screen.findAllByText(/按小时汇总/)).length).toBeGreaterThan(0);
    const selectedDate = within(
      screen.getByRole("region", { name: "过去 365 天每日请求活动" }),
    ).getByRole("button", { name: activeDateLabel });
    expect(selectedDate).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(selectedDate);
    expect(screen.queryByText("正在读取本机 Token 统计…")).not.toBeInTheDocument();
    const restoredRangeSwitch = await screen.findByRole("group", { name: "用量时间粒度" });
    expect(within(restoredRangeSwitch).getByRole("button", { name: "年" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(
      within(screen.getByRole("region", { name: "过去 365 天每日请求活动" })).getByRole("button", {
        name: activeDateLabel,
      }),
    ).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByText(/三类数据都来自后端 usage 精确值/)).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Token 构成" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByText("Token 构成与请求明细"));
    expect(await screen.findByRole("heading", { name: "Token 构成" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Token 构成比例" })).toBeInTheDocument();
    expect(screen.getByText(/未返回 usage 的请求仍计入请求数/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "刷新" })).toBeEnabled();
  });

  it("keeps the model test disabled until a real local model is running", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={["/test"]}>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    expect(await screen.findByRole("heading", { name: "运行" })).toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "测试当前模型" })).toBeInTheDocument();
    expect(screen.getByText("尚未启动模型")).toBeInTheDocument();
    expect(screen.getByLabelText("内容")).toBeDisabled();
    expect(screen.getByRole("button", { name: "发送测试" })).toBeDisabled();
    expect(
      screen.getByText("浏览器预览不会发送内容；请在 Tauri 开发版中测试。"),
    ).toBeInTheDocument();
    expect(screen.getByText("这里不会展示模拟响应。")).toBeInTheDocument();
  });

  it("renders the restricted Agent workspace without browser-side fake execution", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={["/agent"]}>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    expect(await screen.findByRole("heading", { name: "HAL100 Agent" })).toBeInTheDocument();
    expect(screen.getByText("受控执行：Pi 负责推理，Rust 负责授权")).toBeInTheDocument();
    expect(screen.getByText(/v0.84.2/)).toBeInTheDocument();
    expect(screen.getByText(/Qwen3.5-2B Q4_K_M/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "环境诊断" })).toBeInTheDocument();
    expect(document.querySelectorAll(".agent-context-recommendations button")).toHaveLength(3);
    fireEvent.click(screen.getByText("打开任务库"));
    expect(screen.getByRole("button", { name: "全面诊断环境" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "生成单项修复计划" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "分析近期失败" })).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("任务分类"), { target: { value: "models" } });
    expect(screen.getByRole("button", { name: "搜索并规划模型下载" })).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "模型与引擎状态" })).toHaveLength(2);
    expect(screen.getByRole("button", { name: "生成模型切换计划" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "生成引擎安装计划" })).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("任务分类"), { target: { value: "integrations" } });
    expect(screen.getByRole("button", { name: "生成 Pi 私有安装计划" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "生成 Pi 私有卸载计划" })).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "生成 OpenCode 配置计划" })).toHaveLength(2);
    expect(
      screen.getByText(/计划不会自动执行，下载、安装、卸载、删除和配置写入仍需原生确认/),
    ).toBeInTheDocument();
    expect(screen.getByText(/下载计划会绑定精确仓库、修订、文件与/)).toBeInTheDocument();
    expect(screen.getByLabelText("任务")).toHaveValue(
      "检测这台 Mac，并根据真实硬件给出适合的本地模型参数范围和量化建议。",
    );
    expect(screen.getByRole("button", { name: "运行本地任务" })).toBeDisabled();
    expect(
      screen.getByText(
        "浏览器预览不会运行 Agent、启动下载或执行任何写操作；请在 Tauri 开发版中运行。",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("等待一项 HAL100 管理任务")).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "云端" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "生成 Pi 私有卸载计划" }));
    expect(screen.getByLabelText("任务")).toHaveValue(
      "检查 HAL100 私有 Pi Coding Agent 是否存在；如存在，仅为私有运行时生成移入系统废纸篓的卸载计划，保留用户安装、配置和会话。",
    );

    fireEvent.click(screen.getByRole("button", { name: "环境诊断" }));
    expect(await screen.findByText("尚未检测到 OpenCode")).toBeInTheDocument();
    expect(screen.getByText(/不后台轮询/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("radio", { name: "云端" }));
    expect(screen.getByRole("radio", { name: "仅本次任务" })).toBeChecked();
    expect(screen.getByText(/暂无可用且已配置凭据/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "前往推理服务配置" })).toHaveAttribute(
      "href",
      "/workspace/services",
    );
  });

  it("shows only an eligible configured backend in the cloud Agent picker", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(["backend-catalog"], {
      activeBackendId: null,
      modelRoutes: [],
      backends: [
        {
          id: "cloud-openai",
          displayName: "团队 OpenAI",
          kind: "externalOpenAi",
          apiRoot: "https://api.example.test/v1/",
          authMethod: "bearer",
          credentialConfigured: true,
          enabled: true,
          runtimeAvailable: true,
          isActive: false,
          consecutiveFailures: 0,
          circuitOpen: false,
        },
        {
          id: "local-vllm",
          displayName: "本机 vLLM",
          kind: "externalVllm",
          apiRoot: "http://127.0.0.1:8000/v1/",
          authMethod: "none",
          credentialConfigured: false,
          enabled: true,
          runtimeAvailable: true,
          isActive: false,
          consecutiveFailures: 0,
          circuitOpen: false,
        },
      ],
    });

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={["/agent"]}>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    await screen.findByRole("heading", { name: "HAL100 Agent" });
    fireEvent.click(screen.getByRole("radio", { name: "云端" }));
    expect(screen.getByLabelText("已配置后端")).toHaveValue("cloud-openai");
    expect(screen.getByRole("option", { name: "团队 OpenAI · OpenAI" })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: /本机 vLLM/ })).not.toBeInTheDocument();
    expect(screen.getByText(/API Key 只由 Gateway 从 macOS Keychain 读取/)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("模型 ID"), { target: { value: "gpt-test" } });
    expect(screen.getByRole("button", { name: "预览单次云端发送" })).toBeDisabled();

    fireEvent.click(screen.getByRole("radio", { name: "当前会话" }));
    expect(screen.getByLabelText("已配置后端")).toHaveValue("cloud-openai");
    expect(screen.getByLabelText("模型 ID")).toHaveValue("gpt-test");
    expect(screen.getByRole("button", { name: "预览会话授权" })).toBeDisabled();
  });

  it("keeps an active cloud Agent session visible and locks provider switching", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(["agent-cloud-session"], {
      active: true,
      available: true,
      backendId: "cloud-anthropic",
      backendName: "团队 Anthropic",
      backendKind: "externalAnthropic",
      apiRoot: "https://api.anthropic.test/",
      model: "claude-test",
      providerProtocol: "cloudAnthropic",
      activatedAtMs: 1_700_000_000_000,
      lastErrorCode: null,
    });

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={["/agent"]}>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    expect(await screen.findByLabelText("当前云端 Agent 会话")).toHaveTextContent(
      "当前会话：团队 Anthropic · claude-test",
    );
    expect(screen.getByText(/退出或重启后恢复本地默认/)).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "本地" })).toBeDisabled();
    expect(screen.getByRole("radio", { name: "云端" })).toBeDisabled();
    expect(screen.getByRole("radio", { name: "当前会话" })).toBeChecked();
    expect(screen.getByRole("button", { name: "退出云端会话" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "运行云端会话任务" })).toBeDisabled();
  });
});
