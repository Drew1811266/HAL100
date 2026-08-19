# ADR-0006：采用 Pi Agent Core 作为 HAL100 Agent 内核

- 状态：已接受
- 日期：2026-08-17

## 决策

HAL100采用 Pi项目的 `@earendil-works/pi-agent-core` 作为首选 Agent编排内核，并使用其 `@earendil-works/pi-ai` 模型协议层连接 HAL100 Gateway。

HAL100不嵌入完整的 `pi-coding-agent` 产品，不加载 Pi TUI、Coding Tools、用户扩展、项目扩展、Skills、Prompt Templates或上下文文件自动发现。HAL100只向 Pi Agent Core注册由 HAL100定义的结构化白名单工具。

Pi Agent Core运行在按需启动的 HAL100 Agent Kernel Sidecar中。Sidecar与 Rust Core通过 HAL100自有、带版本的私有 RPC协议通信；不把 Pi CLI的私有 RPC协议作为 HAL100内部长期契约。

Rust Core仍是唯一的权限与执行权威：Sidecar只能提出工具调用，所有参数、风险、确认令牌、文件所有权和实际操作都由 Rust Policy Engine与 Deterministic Executor处理。

## 原因

- Pi Agent Core提供成熟的多轮工具循环、流式事件、参数校验、取消、状态和工具执行前后钩子。
- Pi AI可以适配 OpenAI、Anthropic及自定义 OpenAI兼容端点，适合统一连接 HAL100 Gateway。
- 复用通用 Agent内核可以减少自研工具循环和多 Provider差异处理，但不会替代 HAL100领域知识、工具设计和确定性校验。
- 完整 Coding Agent默认面向编程任务，包含任意文件和 Shell能力，不符合 HAL100领域受限和最小权限原则。
- Pi采用 MIT许可证，与 HAL100的 MIT许可证兼容。

## 运行边界

- `Agent Kernel Sidecar`：包含 Pi Agent Core、Pi AI和 HAL100薄适配层，负责会话、模型调用和工具编排。
- `Agent Model Runtime`：仅在本地 Agent模式下加载内置小模型的独立精简 llama.cpp进程。
- Rust Core：管理 Sidecar和 Model Runtime生命周期，处理工具 RPC、确认、执行、审计和凭据。
- HAL100 Gateway：Sidecar访问本地或云端模型的唯一网络入口，并负责 Agent Token统计。

HAL100只向 Sidecar传入短生命周期的 HAL100 Gateway会话凭据，不传入用户云端 API Key。云端 Key仍由 Rust Core从系统凭据库读取并在 Gateway转发时使用。

禁用工具和动态资源约束的是模型能力，不等同于操作系统沙箱。未启用平台沙箱时，Sidecar及其固定依赖仍是以当前用户身份运行的受信任代码，必须作为供应链风险审计；HAL100不能宣称其在操作系统层面无法访问用户文件或网络。

## 安全约束

- 不注册 `bash`、任意文件读写、任意进程启动或任意网络请求工具。
- 禁止扫描 `~/.pi`、项目 `.pi`、`AGENTS.md`和第三方 Pi包。
- Pi自己的确认交互不能替代 HAL100确认令牌。
- Rust必须在工具执行前重新验证名称、Schema、目标、所有权、风险和确认状态。
- Sidecar崩溃或被终止不得影响 Gateway、桌面管理功能和 Usage采集。
- Agent会话默认不使用 Pi文件会话存储；持久化策略由 HAL100统一控制。
- Sidecar使用最小环境变量、固定工作目录、固定代码和依赖锁，禁止运行时安装包和动态加载用户代码。
- macOS进程沙箱或等效限制需要技术验证；在其落实前，必须明确记录 Sidecar同用户权限的残余风险。

## 生命周期与性能

- Sidecar只在用户使用 HAL100 Agent时启动。
- 本地模式按需启动 Agent Model Runtime；云端模式不启动本地模型运行时。
- Sidecar每个任务结束后立即退出；本地 Agent Model Runtime在最后一次任务后空闲2分钟释放模型与 KV Cache。
- Sidecar不计入后台空闲常驻预算，因为空闲状态下进程必须不存在。
- 引入前必须测量冷启动、活动内存、退出回收和反复启动后的泄漏情况。

## 依赖与升级

- 实现时固定 Pi包的精确版本和完整依赖锁，不使用浮动版本范围作为可复现构建依据。
- 记录来源、版本、许可证、版权声明和依赖清单。
- Pi升级必须通过协议、工具调用、安全、取消、流式、性能和跨平台回归测试。
- HAL100自有适配层隔离 Pi API变化，Rust Core不得直接依赖 Pi内部类型。

## 技术验证门槛

在正式实现 Agent功能前，技术验证必须覆盖：

1. 通过 HAL100 Gateway调用本地 OpenAI兼容模型和用户主动选择的云端模型。
2. 只注册 HAL100自定义工具，确认所有 Pi内置 Coding Tools和资源自动发现均未启用。
3. 工具参数 Schema校验、工具错误回传、重试、取消和 Sidecar崩溃恢复。
4. 安装、卸载、删除和强制切换无法绕过 Rust确认。
5. 中文领域任务集上的工具选择、参数正确率和最终完成率。
6. Sidecar冷启动、峰值内存、2分钟空闲退出及反复启停后的资源回收。
7. Apple Silicon开发运行和未来 Windows 10/11 Sidecar构建路径。
8. macOS与未来 Windows可用的 Sidecar进程权限收缩方案；若暂不能实施，记录残余风险和补偿控制。

若 Pi Agent Core无法达到安全、可嵌入、性能或跨平台门槛，应新建 ADR记录替代方案，不得通过放宽 HAL100权限边界来迁就依赖。

## 上游依据

以下上游资料于2026-08-17复核；实现时必须重新检查目标固定版本：

- [Pi仓库与包结构](https://github.com/earendil-works/pi)
- [Pi Agent Core API与工具循环](https://github.com/earendil-works/pi/blob/main/packages/agent/README.md)
- [Pi AI Provider与 OpenAI兼容接口](https://github.com/earendil-works/pi/blob/main/packages/ai/README.md)
- [Pi Coding Agent SDK及内置工具边界](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/sdk.md)
- [Pi安全策略](https://github.com/earendil-works/pi/blob/main/SECURITY.md)
- [Pi MIT许可证](https://github.com/earendil-works/pi/blob/main/LICENSE)
