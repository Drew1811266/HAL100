# ADR-0015：内置 Agent Runtime 与外部 Agent 集成边界

- 状态：已接受
- 日期：2026-08-20
- 关系：补充 ADR-0006、ADR-0008、ADR-0010 与 ADR-0013；不改变 Gateway 数据面、Agent RPC v4 或 SQLite schema v7

## 背景

HAL100 Agent 的按需 Kernel Sidecar 使用固定版本 `@earendil-works/pi-agent-core` 与
`@earendil-works/pi-ai`。Pi 同时存在可由用户独立安装的完整 Pi Coding Agent 产品。HAL100
还计划在已有 OpenCode 接入后继续适配 Pi Coding Agent、OpenClaw 与 Hermes Agent。

如果把“内置内核依赖”和“外部软件接入”视为同一个生命周期，后续可能错误地读取用户
Pi 配置、调用全局 `pi`、共享会话或凭据，甚至把升级或卸载外部软件误认为 HAL100 的
管理权。另一方面，为四个已知客户端提前建设通用插件系统会引入不必要的动态代码、权限
和兼容面。

## 决策

### 1. 两类 Agent 身份永久分离

内置组件使用稳定身份：

- Runtime ID：`hal100-agent-runtime`
- Gateway客户端 ID：`hal100-agent`
- 产品名称：`HAL100 Agent`
- 编排实现：固定版本 Pi Agent Core与 Pi AI

外部客户端使用独立稳定身份：

| 软件 | Integration ID / Gateway客户端 ID |
| --- | --- |
| OpenCode | `opencode` |
| Pi Coding Agent | `pi-coding-agent` |
| OpenClaw | `openclaw` |
| Hermes Agent | `hermes-agent` |

“Pi Coding Agent”只指用户独立安装的软件，不是 HAL100内置 Runtime的别名。界面必须明确
展示二者区别。

### 2. 外部软件只作为 Gateway客户端接入

外部 Agent不得复用 HAL100 Agent Kernel、Sidecar进程、临时 HOME、会话目录、内部模型
路由或任务凭据。每个客户端通过 HAL100 Gateway支持的明确协议调用模型，并拥有独立
Gateway Key、Usage归属与撤销生命周期。

HAL100不负责安装、升级、启动、停止或卸载这些外部软件。检测和配置只在用户打开软件
接入页面、主动刷新或显式生成计划时按需执行。

### 3. 共享稳定策略，保留专用适配器

`hal100-core`拥有外部 Agent身份注册表以及以下跨客户端不变量：

- Integration ID、Gateway客户端 ID和凭据 ID唯一且不与内置 Runtime冲突。
- 只管理 HAL100拥有的配置片段。
- 每个客户端使用独立凭据。
- 自动接入不改变用户默认模型。
- 至少显式声明一种 HAL100 Gateway协议。

每个专用适配器拥有自己的安装检测、配置路径、JSON/JSON5/YAML解析、版本范围、语义
补丁、验证、重载与回滚细节。现有 `OpenCodeIntegrationAdapter`是首个实现；不通过无类型
JSON、动态库或用户脚本把不同配置格式伪装成同一种插件。

### 4. 配置写入继续采用受控计划

未来 Pi Coding Agent、OpenClaw和 Hermes Agent接入必须复用 OpenCode已经验证的控制面
语义：只读检测、语义预览、短期一次性计划、Rust原生确认、应用前文件指纹复验、托管片段
所有权、备份、原子更新、写后验证与失败回滚。

适配器只添加 `hal100`命名 Provider或等价托管条目。除非用户单独发起并确认“设为默认”，
不得修改默认 Provider、默认模型、会话、扩展、Skills或项目配置。

## 迭代14范围

- 建立内置 Runtime和四个外部客户端的稳定注册表。
- 让 OpenCode适配器从注册表读取身份与凭据边界，保持现有IPC、数据库和配置行为兼容。
- 软件接入页展示运行边界及后续专用适配器，不提供尚未实现的写操作。
- 增加官方 Pi配置目录、环境变量、命令发现和完整 Pi Coding Agent依赖均不进入内置
  Sidecar的回归测试。
- 本迭代不写入 Pi、OpenClaw或 Hermes配置，不新增数据库迁移，不建设插件框架。

## 后果

- 用户可以独立安装、升级或卸载官方 Pi Coding Agent而不影响 HAL100 Agent。
- 后续每增加一个外部 Agent都需要一个小型专用适配器和真实官方客户端验收，但共享身份、
  凭据、计划和所有权规则不会漂移。
- 代码量会随客户端能力增加；架构质量以职责、所有权和兼容性衡量，不以适配器数量或文件
  行数衡量。
- 当前 OpenCode API名称继续兼容；`OpenCodeManager`作为过渡类型别名指向明确命名的
  `OpenCodeIntegrationAdapter`，可在调用方自然触达时渐进迁移。

## 实施结果（迭代15–18）

决策已经由四个可用专用适配器落实。共享层只承载模型契约、多资源所有权、短期一次性计划、
受限命令和通用文件事务；配置语法仍由客户端适配器拥有：OpenCode为JSONC、Pi为严格JSON、
OpenClaw为官方CLI驱动的JSON5、Hermes为YAML和`.env`精确变量补丁。

所有客户端均通过固定官方版本的隔离端到端验收，拥有不同Gateway客户端ID、凭据ID、配置
路径和Usage归属。Hermes额外遵守官方64,000 Token上下文前置条件，模型契约不足时阻止其
自身接入而不限制其他客户端。实现没有采用文件行数上限，也没有引入动态插件加载。
