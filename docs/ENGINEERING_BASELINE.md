# HAL100 工程基线

- 状态：已接受
- 版本：1.0
- 日期：2026-08-17

## 1. 目标

本基线把已经确认的产品和架构约束转成可执行的工程规则。首版只构建 Apple Silicon macOS 开发版，不制作安装包；Windows 10/11 只保留可替换的平台边界。

## 2. 平台与工具链

| 项目 | 基线 |
| --- | --- |
| 最低系统 | macOS 13 Ventura |
| CPU | Apple Silicon / `aarch64-apple-darwin` |
| 桌面框架 | Tauri 2.11 系列 |
| Rust | 1.97.1，项目内固定工具链 |
| Node.js | 24 LTS，固定 24.18.0 |
| 包管理 | pnpm 11，提交锁文件 |
| 前端 | React 19.2、TypeScript 6.0、Vite 8 |
| 数据库 | SQLite WAL、显式迁移、单写入路径 |

依赖必须使用精确 JavaScript 版本和已提交的 `pnpm-lock.yaml`；Rust 使用兼容版本声明并提交 `Cargo.lock`。依赖更新是独立、可审计任务，不能在功能开发中顺便升级。

## 3. 运行边界

HAL100首版使用一个长期驻留的 Tauri/Rust主进程，承载系统托盘、Gateway、应用服务和数据库协调。登录后台启动时主 WebView可延迟创建；一旦创建，关闭主窗口时隐藏并复用，Rust主进程继续运行。首版不引入独立系统守护进程。

以下进程按需启动：

- Agent Kernel Sidecar：Pi Agent Core 与 HAL100 适配层，空闲两分钟退出。
- Agent Model Runtime：内置小模型的独立 llama.cpp 运行时，空闲两分钟退出。
- 托管用户推理引擎：由运行时策略启动和退出。

Sidecar 和推理引擎崩溃不得带崩 Rust 主进程。Rust Core 是唯一能够执行安装、卸载、删除、配置修改和进程管理的权限主体。

Agent Kernel进程必须由平台启动器创建：运行时、入口和工作目录先规范化，入口与工作目录必须位于受控 Workspace；清空父进程环境，只注入独立 HOME/TMPDIR、语言、无颜色、Node警告策略和 RPC版本。启动器不得继承 PATH、代理、SSH Agent、云端密钥或用户 Shell变量。macOS开发沙箱是显式测试选项，不可用时返回错误，不得静默回退。

## 4. 代码结构

```text
apps/desktop/            React UI 与薄 Tauri 装配层
crates/hal100-core/      领域规则、用例、平台端口
crates/hal100-infra/     Gateway、SQLite、下载和进程适配
crates/hal100-platform/  macOS 实现；未来增加 Windows 实现
crates/hal100-protocol/  IPC、Gateway 和 Agent RPC 类型
sidecars/agent-kernel/   Pi Agent Core 宿主
contracts/agent-rpc/     协议 Schema、样例和版本说明
tests/fixtures/          跨语言测试样例
```

`apps/desktop/src-tauri` 只负责生命周期、依赖装配和 Tauri 命令；业务规则不得写入命令处理函数。领域核心不得依赖 Tauri、WebView、macOS API 或 Pi 类型。

## 5. 通信约束

- React 与 Rust：小而明确的 Tauri IPC 命令和事件。
- 外部 AI 客户端与 HAL100：回环地址上的 HTTP/SSE Gateway。
- Rust 与 Agent Kernel：带版本的长度前缀 JSON RPC。

Agent RPC 每帧使用四字节大端无符号长度加 UTF-8 JSON，单帧默认上限 1 MiB。stdout 只传协议帧，stderr 只传脱敏日志。协议包含版本、请求 ID、消息类型和载荷；不兼容变更必须提升协议版本。

前端不获得 Shell 权限。Sidecar 不获得用户云端 API Key，也不执行 HAL100 工具；工具请求必须返回 Rust Tool Broker 重新校验和执行。

迭代1模拟工具闭环固定使用 `agent.simulation.start → tool.call.request → tool.call.result → agent.simulation.completed`。Sidecar只负责 Pi编排与关联等待，Rust按自己的协议类型、白名单和精确参数执行二次校验。跨语言 fixture、TypeScript故障关闭测试及真实 Rust↔Sidecar子进程测试共同覆盖该边界。

## 6. 前端基线

- React Router 负责页面结构。
- TanStack Query 管理 IPC 查询缓存和事件失效。
- React Hook Form 与 Zod 用于表单和不可信输入。
- CSS Modules/全局设计令牌承载已确认的 HAL100 视觉规范，不引入 Tailwind。
- 首版不引入 Redux 或 Zustand；只有出现无法由局部状态和查询缓存解决的问题时才新增状态库。
- React Server Components 不进入桌面 SPA。

## 7. Rust 基线

- Tokio 负责异步任务和取消边界。
- Axum 承载本地 Gateway。
- rusqlite 使用 bundled SQLite、WAL 和显式迁移。
- serde 定义稳定的协议 DTO。
- thiserror 定义分层错误；tracing 输出结构化日志。
- 默认日志为异步 JSONL，不写入提示词、回答、凭据和完整本机路径；活动文件上限5 MiB，最多保留6个归档；同一稳定错误码按60秒窗口聚合且不增加定时器。
- 数据库写入不得发生在流式转发热路径中；Usage字段先做小尺寸约束，再通过不丢弃已接收记录的专用单写入线程写入。

## 8. 后台性能规则

- 空闲 CPU 目标不高于 0.3%。
- 主 WebView隐藏或尚未创建时，主进程物理占用目标不高于80 MB。
- 不使用固定高频轮询；优先使用进程退出、文件通知和请求事件。
- 外部后端健康检查只在界面可见、请求失败、用户刷新或低频必要维护时触发。
- 流式代理使用有界缓冲、活动流许可、总字节限制、读取/总时限、背压和客户端断开取消，不缓存完整响应。
- Usage 事件按批次提交，图表优先查询聚合表。

## 9. 质量门槛

统一检查包含：

- Biome 格式和静态检查。
- TypeScript 严格类型检查。
- Vitest 与 React Testing Library。
- Rust fmt、Clippy、单元测试和集成测试。
- Agent RPC Rust/TypeScript 契约样例。
- Gateway 模拟后端契约测试。
- macOS原生启动、窗口隐藏复用、托盘恢复、强制销毁诊断和睡眠唤醒冒烟测试。

开发构建必须保持可运行。任何数据库变更都需要迁移测试；任何新系统能力都需要安全边界和性能预算复核。

## 10. Sidecar 开发运行策略

当前开发阶段使用固定 Node 24运行 Agent Kernel，并以精确版本、受限环境、临时HOME和进程
回收测试约束该边界。项目目前不规划安装包或可分发构建，因此自包含 Sidecar、Node单文件
可执行和Tauri外部二进制不属于当前完成条件；只有未来明确重新打开分发范围时，才建立独立
决策和验收任务，不能从当前文档自动推定为下一阶段。
