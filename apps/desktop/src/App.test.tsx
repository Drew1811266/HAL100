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

    expect(
      await screen.findByRole("heading", { name: "你好，今天可以从这里开始" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "还没有可用的模型或服务" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "连接服务" })).toBeInTheDocument();
    expect(screen.getAllByText("HAL100 运行正常").length).toBeGreaterThan(0);
    expect(screen.queryByRole("heading", { name: "添加第一个模型" })).not.toBeInTheDocument();
    expect(screen.getByRole("region", { name: "当前状态" })).toBeInTheDocument();

    const mainNavigation = screen.getByRole("navigation", { name: "主导航" });
    expect(within(mainNavigation).getAllByRole("link")).toHaveLength(5);
    for (const name of ["首页", "模型与运行", "软件接入", "Agent", "活动"]) {
      expect(within(mainNavigation).getByRole("link", { name })).toBeInTheDocument();
    }
    expect(
      within(mainNavigation).queryByRole("link", { name: "运行方案" }),
    ).not.toBeInTheDocument();
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

    const appearanceToggle = await screen.findByRole("button", { name: "切换深色外观" });
    fireEvent.click(appearanceToggle);
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    expect(window.localStorage.getItem("hal100-theme")).toBe("dark");
    expect(appearanceToggle).toHaveAttribute("aria-pressed", "true");
  });

  it("nests runtime profiles under the model and runtime workspace", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={["/profiles"]}>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    expect(
      await screen.findByRole("heading", { name: "模型与运行", level: 1 }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "保存当前环境" })).toBeDisabled();
    expect(screen.getByText("还没有保存快捷环境")).toBeInTheDocument();
    fireEvent.click(screen.getByText("可保存的外部环境与兼容信息"));
    expect(await screen.findByText("已识别外部运行身份")).toBeInTheDocument();
    expect(screen.getByText("1 个可验证候选")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "保存为快捷环境" }));
    expect(screen.getByRole("dialog", { name: "保存为快捷环境" })).toBeInTheDocument();
    expect(screen.getByText(/只保存后端身份、引擎版本和类型化模型证据/)).toBeInTheDocument();
    const mainNavigation = screen.getByRole("navigation", { name: "主导航" });
    expect(within(mainNavigation).getByRole("link", { name: "模型与运行" })).toHaveClass("active");
    expect(
      within(mainNavigation).queryByRole("link", { name: "运行方案" }),
    ).not.toBeInTheDocument();
    const workspaceNavigation = screen.getByRole("navigation", { name: "模型与运行" });
    expect(within(workspaceNavigation).getAllByRole("link")).toHaveLength(4);
    expect(within(workspaceNavigation).getByRole("link", { name: "快捷切换" })).toHaveClass(
      "active",
    );
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

    fireEvent.click(await screen.findByRole("button", { name: "查看接入方式" }));
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

    fireEvent.click(await screen.findByRole("button", { name: "了解接入边界" }));
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

    expect(await screen.findByRole("heading", { name: "活动" })).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "活动" })).toBeInTheDocument();
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
    expect(screen.getByRole("heading", { name: "通用" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "模型来源" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "数据保留" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "关于" })).toBeInTheDocument();
    expect(await screen.findByText("HAL100 1.0.6")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "切换登录启动" })).toBeDisabled();
    expect(screen.getByLabelText("默认模型下载源")).toBeDisabled();
    expect(screen.getByRole("button", { name: "预览并清理" })).toBeDisabled();
  });

  it("starts first-run setup from the user's existing AI environment", async () => {
    window.localStorage.setItem("hal100-preview-onboarding", "incomplete");
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    expect(
      await screen.findByRole("heading", { name: "从你已有的 AI 环境开始" }),
    ).toBeInTheDocument();
    expect(screen.getAllByText("等待首次设置").length).toBeGreaterThan(0);
    expect(screen.getByRole("region", { name: "首次使用方式" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /添加本地模型/ })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /连接云端服务/ })).toBeInTheDocument();

    const localService = await screen.findByRole("button", { name: /使用已有本地服务/ });
    expect(within(localService).getByText("已发现 · 推荐")).toBeInTheDocument();
    expect(within(localService).getByText(/本机 Ollama（预览示例）/)).toBeInTheDocument();
    fireEvent.click(localService);

    const candidateDrawer = await screen.findByRole("dialog", {
      name: "连接 本机 Ollama（预览示例）",
    });
    expect(
      within(candidateDrawer).getByText("浏览器预览不会保存或启用真实服务连接。"),
    ).toBeInTheDocument();
    expect(within(candidateDrawer).getByRole("button", { name: "连接并继续" })).toBeDisabled();
    fireEvent.click(
      within(candidateDrawer).getByRole("button", {
        name: "关闭连接 本机 Ollama（预览示例）",
      }),
    );

    fireEvent.click(screen.getByRole("link", { name: "设置" }));
    expect(await screen.findByRole("heading", { name: "设置" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "初始化配置中心" })).not.toBeInTheDocument();
    expect(screen.getByText("首次使用从首页开始")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /返回首页/ })).toBeInTheDocument();
    expect(window.localStorage.getItem("hal100-preview-onboarding")).toBe("incomplete");
  });

  it("opens each first-use choice at the exact model or service task", async () => {
    window.localStorage.setItem("hal100-preview-onboarding", "incomplete");
    const modelClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { unmount } = render(
      <QueryClientProvider client={modelClient}>
        <MemoryRouter initialEntries={["/workspace/models?setup=1"]}>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    expect(await screen.findByRole("dialog", { name: "添加模型" })).toBeInTheDocument();
    unmount();

    const localServiceClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const localServiceView = render(
      <QueryClientProvider client={localServiceClient}>
        <MemoryRouter initialEntries={["/workspace/services?setup=1"]}>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    expect(await screen.findByRole("dialog", { name: "添加推理服务" })).toBeInTheDocument();
    const discovery = await screen.findByRole("region", { name: "本机后端发现结果" });
    expect(within(discovery).getByText("本机 Ollama（预览示例）")).toBeInTheDocument();
    localServiceView.unmount();

    const cloudServiceClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={cloudServiceClient}>
        <MemoryRouter initialEntries={["/workspace/services?setup=cloud"]}>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    const cloudEditor = await screen.findByRole("dialog", { name: "添加推理服务" });
    expect(within(cloudEditor).getByDisplayValue("https://api.openai.com/v1/")).toBeInTheDocument();
    expect(within(cloudEditor).getByRole("button", { name: "保存并验证" })).toBeDisabled();
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

    expect(await screen.findByRole("heading", { name: "模型与运行" })).toBeInTheDocument();
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
    expect(screen.getByRole("heading", { name: "模型与运行" })).toBeInTheDocument();
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

    expect(await screen.findByRole("heading", { name: "模型与运行" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "默认服务与模型名称映射" })).toBeInTheDocument();
    const serviceHeading = screen.getByRole("heading", { name: "已连接服务" });
    const routingHeading = screen.getByRole("heading", {
      name: "默认服务与模型名称映射",
    });
    expect(
      serviceHeading.compareDocumentPosition(routingHeading) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(screen.getByText(/`hal100-active` 始终指向当前服务/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "添加推理服务" }));
    const serviceDrawer = await screen.findByRole("dialog", { name: "添加推理服务" });
    fireEvent.click(within(serviceDrawer).getByRole("button", { name: "开始发现" }));
    const discovery = await screen.findByRole("region", { name: "本机后端发现结果" });
    expect(within(discovery).getByText("本机 Ollama（预览示例）")).toBeInTheDocument();
    expect(within(discovery).getByText(/不会常驻监测/)).toBeInTheDocument();

    fireEvent.click(within(serviceDrawer).getByRole("button", { name: "手动填写配置" }));
    const backendEditor = await screen.findByRole("dialog", { name: "添加推理服务" });
    expect(within(backendEditor).getByText(/API Key 只写入 macOS Keychain/)).toBeInTheDocument();
    expect(within(backendEditor).getByRole("button", { name: "保存连接" })).toBeDisabled();
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

    expect(await screen.findByRole("heading", { name: "活动" })).toBeInTheDocument();
    const summary = screen.getByRole("region", { name: "最近十四天用量摘要" });
    expect(within(summary).getByText("总 Token")).toBeInTheDocument();
    expect(within(summary).getByText("今天")).toBeInTheDocument();
    expect(within(summary).getByText("缓存节省")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: /Token 用量趋势/ })).toBeInTheDocument();
    expect(screen.getByRole("table", { name: "过去 365 天每日请求格子" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "最近客户端" })).toBeInTheDocument();

    const advancedDetails = screen.getByText("筛选、时间范围与请求明细").closest("details");
    expect(advancedDetails).not.toHaveAttribute("open");
    fireEvent.click(screen.getByText("筛选、时间范围与请求明细"));
    expect(advancedDetails).toHaveAttribute("open");
    const rangeSwitch = screen.getByRole("group", { name: "用量时间粒度" });
    for (const range of ["年", "月", "周", "天"]) {
      expect(within(rangeSwitch).getByRole("button", { name: range })).toBeInTheDocument();
    }
    expect(within(rangeSwitch).getByRole("button", { name: "月" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    fireEvent.click(within(rangeSwitch).getByRole("button", { name: "年" }));
    expect(within(rangeSwitch).getByRole("button", { name: "年" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(await screen.findByRole("heading", { name: "Token 构成" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Token 构成比例" })).toBeInTheDocument();
    expect(screen.getByText("当前请求中有精确计量值的比例")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "请求明细" })).toBeInTheDocument();
    expect(screen.getAllByRole("table")).toHaveLength(2);
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

    expect(await screen.findByRole("heading", { name: "模型与运行" })).toBeInTheDocument();
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
    expect(screen.getByText("所有系统更改都由你确认")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "你想让 Agent 完成什么？" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "结果与待确认操作" })).toBeInTheDocument();
    expect(screen.getByLabelText("多步骤任务")).not.toHaveAttribute("open");
    expect(screen.getByText(/v0.84.2/)).toBeInTheDocument();
    expect(screen.getByText(/Qwen3.5-2B Q4_K_M/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "检查环境" })).toBeInTheDocument();
    expect(document.querySelectorAll(".agent-context-recommendations button")).toHaveLength(3);
    expect(screen.getByLabelText("用自然语言描述目标")).toHaveValue("");
    fireEvent.click(screen.getByText("浏览全部任务"));
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
    expect(screen.getByLabelText("用自然语言描述目标")).toHaveValue("");
    expect(screen.getByRole("button", { name: "开始分析" })).toBeDisabled();
    expect(
      screen.getByText(
        "浏览器预览不会运行 Agent、启动下载或执行任何写操作；请在桌面开发版中运行。",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("等待你的任务")).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "云端" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "生成 Pi 私有卸载计划" }));
    expect(screen.getByLabelText("用自然语言描述目标")).toHaveValue(
      "检查 HAL100 私有 Pi Coding Agent 是否存在；如存在，仅为私有运行时生成移入系统废纸篓的卸载计划，保留用户安装、配置和会话。",
    );

    fireEvent.click(screen.getByRole("button", { name: "检查环境" }));
    expect(await screen.findByText("尚未检测到 OpenCode")).toBeInTheDocument();
    expect(screen.getByText(/不后台轮询/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("radio", { name: "云端" }));
    expect(screen.getByRole("radio", { name: "仅本次任务" })).toBeChecked();
    expect(screen.getByText(/暂无可用且已配置凭据/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "前往连接服务" })).toHaveAttribute(
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
    expect(screen.getByLabelText("已连接服务")).toHaveValue("cloud-openai");
    expect(screen.getByRole("option", { name: "团队 OpenAI · OpenAI" })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: /本机 vLLM/ })).not.toBeInTheDocument();
    expect(screen.getByText(/API Key 只由 Gateway 从 macOS Keychain 读取/)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("模型 ID"), { target: { value: "gpt-test" } });
    expect(screen.getByRole("button", { name: "预览单次云端发送" })).toBeDisabled();

    fireEvent.click(screen.getByRole("radio", { name: "当前会话" }));
    expect(screen.getByLabelText("已连接服务")).toHaveValue("cloud-openai");
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
