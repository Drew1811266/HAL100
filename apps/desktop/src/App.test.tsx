import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import App, { modelDownloadPollingInterval } from "./App";

describe("HAL100 application shell", () => {
  afterEach(cleanup);

  beforeEach(() => {
    window.localStorage.clear();
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

    expect(await screen.findByText("HAL100 已准备就绪")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "模型库" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "接下来做什么" })).toBeInTheDocument();
  });

  it("persists an explicit appearance choice", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "使用深色外观" }));
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    expect(window.localStorage.getItem("hal100-theme")).toBe("dark");
    expect(screen.getByRole("button", { name: "使用浅色外观" })).toBeInTheDocument();
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

    fireEvent.click(await screen.findByRole("button", { name: "配置 OpenCode" }));
    expect(await screen.findByRole("dialog", { name: "配置 OpenCode" })).toBeInTheDocument();
    expect(screen.getByText("+ provider.hal100.options.baseURL")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "确认并应用配置" })).toBeDisabled();
    expect(screen.getByText("浏览器预览模式只能查看变更，不能应用。")).toBeInTheDocument();
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

    expect(await screen.findByText("/v1/chat/completions · /v1/responses")).toBeInTheDocument();
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

    expect(await screen.findByRole("heading", { name: "审计记录" })).toBeInTheDocument();
    expect(screen.getByText("尚无受控操作记录")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "刷新记录" })).toBeEnabled();
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
    expect(screen.getByRole("heading", { name: "初始化配置中心" })).toBeInTheDocument();
    expect(screen.getByText("基础设置已完成")).toBeInTheDocument();
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

    expect(await screen.findByRole("heading", { name: "HAL100 已准备就绪" })).toBeInTheDocument();
    expect(screen.getByText("基础设置尚未完成")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("link", { name: "前往设置" }));
    expect(await screen.findByRole("heading", { name: "设置" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "初始化配置中心" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "模型库" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("link", { name: "总览" }));
    expect(await screen.findByRole("heading", { name: "HAL100 已准备就绪" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("link", { name: "前往设置" }));
    expect(await screen.findByRole("heading", { name: "初始化配置中心" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "完成设置" })).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "ModelScope" }));
    expect(await screen.findByText(/Apple M1（浏览器预览）/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "保持关闭" }));
    expect(screen.getByLabelText("基础设置完成 2 / 2 项")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "完成设置" }));
    expect(await screen.findByText("HAL100 已准备就绪")).toBeInTheDocument();
    expect(window.localStorage.getItem("hal100-preview-onboarding")).toBeNull();
  });

  it("shows on-demand hardware data and lets the user choose a default model source", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={["/models"]}>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    expect(await screen.findByText("Apple M1（浏览器预览）")).toBeInTheDocument();
    expect(screen.getByText("尚未选择，HAL100 不会替你指定来源")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "ModelScope" }));
    expect(await screen.findByText("当前默认：ModelScope")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "ModelScope" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );

    fireEvent.change(screen.getByRole("searchbox", { name: "模型名称或仓库" }), {
      target: { value: "Qwen3 GGUF" },
    });
    fireEvent.click(screen.getByRole("button", { name: "搜索模型" }));
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

    fireEvent.click(screen.getByRole("button", { name: "导入 GGUF" }));
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
    fireEvent.change(await screen.findByRole("searchbox", { name: "模型名称或仓库" }), {
      target: { value: repository },
    });
    fireEvent.click(screen.getByRole("button", { name: "搜索模型" }));

    expect(await screen.findByText(repository)).toBeInTheDocument();
    expect(screen.getByText("Qwen3.5-2B-Q4_K_M.gguf")).toBeInTheDocument();
    expect(screen.queryByText(/返回 1 个结果/)).not.toBeInTheDocument();
  });

  it("requires confirmation before installing the managed llama.cpp engine", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={["/backends"]}>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    expect(await screen.findByText("llama.cpp")).toBeInTheDocument();
    expect(screen.getByText("未安装")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Gateway 路由与模型别名" })).toBeInTheDocument();
    expect(screen.getByText(/`hal100-active` 始终指向当前活动后端/)).toBeInTheDocument();
    fireEvent.click(screen.getByText("强制操作"));
    expect(screen.getByRole("button", { name: "强制切换" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "强制停止" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "安装 llama.cpp" }));
    const dialog = await screen.findByRole("dialog", { name: "安装 llama.cpp" });
    expect(within(dialog).getByText(/ggml-org\/llama.cpp GitHub Releases/)).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "确认安装" })).toBeDisabled();
    expect(
      within(dialog).getByText("浏览器预览模式只能查看计划，不能执行操作。"),
    ).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "取消" }));

    fireEvent.click(screen.getByRole("button", { name: "发现本机服务" }));
    const discovery = await screen.findByRole("region", { name: "本机后端发现结果" });
    expect(within(discovery).getByText("本机 Ollama（预览示例）")).toBeInTheDocument();
    expect(within(discovery).getByText(/不扫描局域网/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "添加外部后端" }));
    const backendEditor = await screen.findByRole("dialog", { name: "添加外部后端" });
    expect(within(backendEditor).getByText(/API Key 只写入 macOS Keychain/)).toBeInTheDocument();
    expect(within(backendEditor).getByRole("button", { name: "保存后端" })).toBeDisabled();
  });

  it("renders the exact-usage dashboard without background polling", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={["/usage"]}>
          <App />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    expect(await screen.findByRole("heading", { name: "Token 统计" })).toBeInTheDocument();
    const summary = screen.getByRole("region", { name: "Token 汇总" });
    expect(within(summary).getByText("请求数")).toBeInTheDocument();
    expect(within(summary).getByText(/不会按字符数估算/)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "用量趋势" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Token 构成" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "累计 Token 构成环形图" })).toBeInTheDocument();
    expect(screen.getByText("尚无 Token 用量记录")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "刷新统计" })).toBeEnabled();
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

    expect(await screen.findByRole("heading", { name: "测试模型" })).toBeInTheDocument();
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
    expect(screen.getByText("v0.84.2")).toBeInTheDocument();
    expect(screen.getByText("Qwen3.5-2B Q4_K_M")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "环境诊断" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "全面诊断环境" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "生成单项修复计划" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "模型与引擎状态" })).toBeInTheDocument();
    fireEvent.click(screen.getByText("更多计划模板"));
    expect(screen.getByRole("button", { name: "生成模型切换计划" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "生成引擎安装计划" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "生成 OpenCode 配置计划" })).toBeInTheDocument();
    expect(
      screen.getByText(/计划不会自动执行，安装、卸载、删除和配置写入仍需原生确认/),
    ).toBeInTheDocument();
    expect(screen.getByText(/模型搜索与下载仍未开放给 Agent/)).toBeInTheDocument();
    expect(screen.getByLabelText("任务")).toHaveValue(
      "检测这台 Mac，并根据真实硬件给出适合的本地模型参数范围和量化建议。",
    );
    expect(screen.getByRole("button", { name: "运行本地任务" })).toBeDisabled();
    expect(
      screen.getByText("浏览器预览不会启动模型或生成模拟回答；请在 Tauri 开发版中运行。"),
    ).toBeInTheDocument();
    expect(screen.getByText("等待一项 HAL100 管理任务")).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /当前会话使用云端/ })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "环境诊断" }));
    expect(await screen.findByText("尚未检测到 OpenCode")).toBeInTheDocument();
    expect(screen.getByText(/不后台轮询/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("radio", { name: /云端单次增强/ }));
    expect(screen.getByText(/暂无可用且已配置凭据/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "前往推理后端配置" })).toHaveAttribute(
      "href",
      "/backends",
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
    fireEvent.click(screen.getByRole("radio", { name: /云端单次增强/ }));
    expect(screen.getByLabelText("已配置后端")).toHaveValue("cloud-openai");
    expect(screen.getByRole("option", { name: "团队 OpenAI · OpenAI" })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: /本机 vLLM/ })).not.toBeInTheDocument();
    expect(screen.getByText(/API Key 只由 Gateway 从 macOS Keychain 读取/)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("模型 ID"), { target: { value: "gpt-test" } });
    expect(screen.getByRole("button", { name: "预览单次云端发送" })).toBeDisabled();

    fireEvent.click(screen.getByRole("radio", { name: /当前会话使用云端/ }));
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
    expect(screen.getByRole("radio", { name: /本地 Qwen/ })).toBeDisabled();
    expect(screen.getByRole("radio", { name: /云端单次增强/ })).toBeDisabled();
    expect(screen.getByRole("radio", { name: /当前会话使用云端/ })).toBeChecked();
    expect(screen.getByRole("button", { name: "退出云端会话" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "运行云端会话任务" })).toBeDisabled();
  });
});
