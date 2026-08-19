# ADR-0007：工程基线、代码布局与通信协议

- 状态：已接受
- 日期：2026-08-17

## 决策

HAL100 首版最低支持 macOS 13，仅构建 Apple Silicon。项目固定 Node.js 24 LTS、pnpm 11、Rust 1.97.1、Tauri 2、React 19、TypeScript 6 和 Vite 8。

代码采用 Rust Workspace 与 pnpm Workspace。领域、基础设施、平台和协议分别放入 `hal100-core`、`hal100-infra`、`hal100-platform` 与 `hal100-protocol`；Tauri 层仅进行生命周期管理和依赖装配。Pi Agent Kernel 位于独立的按需 Sidecar。

首版不引入独立守护进程。Gateway 与后台协调由 Rust主进程承载。原定的 WebView关闭即销毁策略已由 [ADR-0009](0009-main-webview-reuse.md)替代为主 WebView隐藏复用。

Rust 与 Agent Sidecar 使用 HAL100 自有的版本化二进制帧：四字节大端长度加 UTF-8 JSON。每帧有严格大小上限，stdout 不允许混入日志。Tauri IPC、Agent RPC 和 Gateway 数据面不能互相替代。

## 原因

- macOS 13 提供现代 `SMAppService` 登录项接口，同时覆盖全部 Apple Silicon 代际。
- Node 24 是长期支持线，并满足 Pi 当前 Node `>=22.19.0` 的要求。
- TypeScript 6 保留成熟的编程式 API和工具链兼容性；TypeScript 7.0 的编程式 API 尚未稳定。
- 单个 Rust 后台进程比主应用加独立守护进程更节省内存，也避免首版增加安装、升级和 IPC 故障面。
- 分层 Workspace 保证未来 Windows 平台实现和可能的后台进程拆分不要求重写领域逻辑。
- 长度前缀协议能明确处理分片、粘包、换行和最大消息大小，适合流式 Agent 事件。

## 后果

- 当前开发机的 Rust 1.95.0 需要由项目固定工具链安装 1.97.1。
- 开发阶段可使用本机 Node，但内部测试分发前必须完成自包含 Sidecar 验证。
- `Cargo.lock` 和 `pnpm-lock.yaml` 是可复现构建的一部分，必须提交。
- Windows 代码当前只保留接口；任何平台专用 API不得进入领域库。
- 如果后续可靠性数据证明需要独立守护进程，应新建 ADR，并复用现有 Core/Infra 边界实施。

## 验证

1. Rust Workspace 和 pnpm Workspace 可独立构建、检查与测试。
2. 浏览器模式可使用受控开发 Bridge 预览界面，Tauri 模式只调用白名单命令。
3. Agent RPC 对分片、粘包、超大帧和非法 JSON 有双语言测试。
4. 隐藏主 WebView后测量空闲 CPU和内存，并对隐藏/显示及强制销毁两种路径做回归，结果记录在 `docs/benchmarks`。
5. Pi Sidecar 技术验证只加载精确固定的 `pi-agent-core` 和 `pi-ai`，不加载 Coding Agent。
