# HAL100 软件架构

## 1. 架构目标

- 长期后台运行时低资源占用。
- 本地网关转发时低延迟、真流式、可取消。
- UI、Agent和用户推理进程相互隔离。
- 系统操作只通过受控 Rust服务执行。
- 首版专注 Apple Silicon，同时避免将 macOS细节泄漏到领域核心。
- 数据和协议均可迁移，未来增加 Windows 10/11实现时不重写业务规则。

## 2. 技术栈

| 层级 | 技术选择 |
| --- | --- |
| 桌面容器 | Tauri 2 |
| 前端 | React + TypeScript + Vite |
| 领域与系统核心 | Rust |
| 异步运行时 | Rust异步生态，具体库在骨架阶段锁定 |
| 本地数据库 | SQLite + WAL +显式迁移 |
| 本地推理 | llama.cpp系列运行时 |
| Agent编排 | Pi Agent Core + Pi AI，封装于按需 Sidecar |
| 控制通信 | Tauri IPC |
| Agent私有通信 | HAL100自有版本化 RPC，通过 Sidecar标准输入/输出传输 |
| 推理数据面 | 本机 HTTP + SSE/流式响应 |
| 凭据 | macOS Keychain；未来 Windows Credential Manager |

## 3. 运行拓扑

```text
OpenCode / Pi Coding Agent / OpenClaw / Hermes Agent / 通用客户端
                            │
                            ▼
HAL100 Gateway (127.0.0.1)
        │
        ▼
Router ──────────────── Usage Collector ── SQLite
  │
  ├── HAL100托管 llama.cpp
  ├── 外部 llama.cpp
  ├── 外部 Ollama
  ├── 外部 vLLM
  └── 局域网/自定义后端

React WebView ── Tauri IPC ── Rust Core
                                  │
                                  ├── Runtime Manager
                                  ├── Model Registry
                                  ├── Integration Manager
                                  └── Agent Manager / Tool Broker
                                           │
                                           ├── Agent Kernel Sidecar
                                           │     └── Pi Agent Core + Pi AI
                                           └── 本地 Agent Model Runtime

Agent Kernel Sidecar ── 临时本地凭据 ── HAL100 Gateway
                                             │
                                             ├── 本地 Agent Model Runtime
                                             └── 用户主动启用的云端 API
```

所有本地和云端 Agent模型请求都必须经过 HAL100 Gateway。Sidecar不直接持有云端 API Key，也不直接连接推理后端。Sidecar提出的工具调用经私有 RPC返回 Rust Tool Broker，不能在 Sidecar内直接执行系统操作。

## 4. 进程模型

### 4.1 长期常驻

Tauri Core进程长期常驻并承载：

- 系统托盘和单实例控制。
- Rust Core状态。
- 本地 Gateway。
- SQLite连接和单写入器。
- 低频健康检查。

登录后台启动且界面从未打开时可以不创建 React WebView。主 WebView一旦创建，窗口关闭时隐藏并暂停全部前端周期活动；托盘重新打开时复用。该策略来自迭代1的100轮生命周期回归，可避免 macOS WebKit反复销毁/重建导致的进程内资源增长。Rust Core和 SQLite仍是权威状态源，临时窗口仍按需销毁。

### 4.2 按需进程

- Agent Kernel Sidecar：包含 Pi Agent Core、Pi AI和 HAL100薄适配层；每个 Agent任务启动一个进程，完成版本化 RPC shutdown后立即退出。
- Agent Model Runtime：本地 Agent模式下按需加载内置小模型；云端模式不启动；最后一次 Agent任务完成后空闲两分钟退出。
- 托管用户 llama.cpp：请求到达或用户手动启动时启动，默认空闲15分钟后退出。
- 下载/元数据工作进程：只在有任务时运行，CPU密集工作不得阻塞网关异步线程。

Kernel Sidecar与 Model Runtime崩溃时不得带崩 Rust Core，也不得中断面向 OpenCode和通用客户端的 Gateway。Rust Core负责子进程状态机、超时、取消、退出和孤儿进程清理。

### 4.3 外部进程

HAL100可以监测和连接外部 Ollama、vLLM和 llama.cpp，但不得假定拥有其进程、文件或安装环境。外部后端的停止、升级和卸载默认不属于 HAL100的管理权。

### 4.4 内置 Runtime与外部 Agent

`HAL100 Agent`是产品内置 Runtime，使用`hal100-agent-runtime`身份和固定Workspace中的 Pi
Agent Core/Pi AI依赖；每个任务运行在独立Sidecar进程、临时HOME、会话目录和短期凭据中。
它不执行全局`pi`、不读取`~/.pi/agent`，也不拥有用户安装的 Pi Coding Agent。

OpenCode、Pi Coding Agent、OpenClaw和 Hermes Agent属于外部软件接入。它们独立安装、运行、
升级和保存会话，只通过各自的Gateway客户端身份、专属Key和受管Provider片段连接HAL100。
任一外部Agent撤销或卸载不得影响内置Runtime和其他客户端。稳定身份与跨客户端不变量由
`hal100-core::ExternalAgentIntegrationRegistry`拥有，JSON/JSON5/YAML、版本检测、重载和
回滚由各专用适配器拥有；不引入动态插件或用户代码加载。

通用控制面提供版本化`hal100-active`模型契约、每适配器独立的一次性计划、受限命令运行器
以及多资源所有权记录。它不抽象上游配置语法：OpenCode使用JSONC保留式补丁，Pi使用严格
JSON，OpenClaw通过官方CLI管理JSON5，Hermes使用YAML Provider与精确`.env`变量补丁。

迭代18后四个适配器均已可用。OpenClaw已经以固定官方版本真实验收Chat Completions、
Responses与Anthropic Messages；Hermes以0.18.2验收Chat Completions，并在模型契约低于官方
64,000 Token上下文门槛时返回Blocked。该兼容门槛属于适配器能力判断，不会反向限制Gateway、
其他客户端或未来软件规模。Hermes的YAML可以保存不含Key的原字节备份；`.env`可能含其他
用户秘密，因此只在事务内存中保留回滚副本，不持久复制整个文件。

## 5. Rust模块边界

```text
hal100-core
├── system_probe       硬件、系统、进程、磁盘和能力检测
├── model_registry     模型元数据、文件所有权和索引
├── downloads          下载源、断点续传、校验和安装事务
├── runtime_manager    托管进程状态机、健康检查和恢复
├── backend_adapters   各推理后端能力和协议适配
├── gateway            认证、限流、流式转发和协议入口
├── router             模型别名、活动路由和请求排空
├── usage              Usage标准化、聚合和保留策略
├── agent              Sidecar生命周期、私有 RPC、会话和本地/云端模型调度
├── tools              Agent白名单工具定义
├── policy             参数校验、风险等级和确认令牌
├── integrations       外部Agent稳定身份、专用适配器、配置所有权和回滚
├── credentials        系统凭据抽象和本地 Key哈希
├── audit              操作审计、结构化日志和脱敏
└── platform           平台接口及 macOS/Windows实现边界
```

前端不得直接访问数据库、凭据库、任意文件系统或系统 Shell。前端只能通过小而明确的 Tauri命令调用 Rust应用服务。

迭代12开始按业务上下文渐进组织React：根`App`只负责应用壳、全局设置和路由，首条`features/agent`切片拥有Agent页面与环境诊断展示；`lib/desktop-api`继续作为Tauri和只读浏览器预览的单一适配入口。该组织以职责和变化原因划分，不设置文件行数上限，也不要求一次性迁移其他成熟页面或全局样式。

迭代4模型管理能力已经沿这些边界落地：`hal100-platform`按需读取固定 Apple Silicon系统字段；`hal100-protocol`定义硬件画像、远端目录、下载、引擎、测试与 Usage DTO；`hal100-infra`实现 Hugging Face/ModelScope官方 API适配、可恢复下载、GGUF校验、SQLite schema v5目录以及固定供应链的`LlamaCppManager`。迭代5把Usage来源语义迁移到schema v6，并在同一Gateway数据面加入Responses与Messages协议解析；schema v7增加非敏感后端和模型别名，平台凭据抽象由macOS Keychain实现。路由活动对象持有可旋转的请求代次取消令牌，使强制切换无需轮询即可中断旧代。迭代6在既有通用`settings`和审计表之上增加类型化桌面设置、可恢复向导、登录项状态、保留策略和通用客户端生命周期，不需要增加schema版本。每个通用客户端明文Key只在创建命令的单次响应中出现，运行时注册表与SQLite只保存摘要；撤销时数据库与运行时注册表以补偿事务保持一致。数据保留不会运行后台清理任务，只有用户查看精确数量并通过Rust原生确认后才按固定截止时间删除过期记录。审计DTO只返回固定安全字段白名单。协议、路由与桌面设置扩展没有新增常驻轮询或计时器。目录 API的运营方与模型仓库发布者是两层独立信任边界，确认计划保留实际仓库、修订、许可证和文件哈希。Tauri只暴露窄命令：只读查询、一次性计划、受控生命周期、桌面设置、通用客户端和脱敏审计操作。Dialog插件与登录项插件只从Rust调用，WebView capability不含通用对话框、自动启动或文件权限。硬件、数据库、Keychain、安装和进程操作不会进入每个Gateway请求的热路径；页面隐藏后没有探测、统计或审计定时器。

## 6. 网关请求生命周期

1. 在连接和请求大小限制内接收请求。
2. 校验本地客户端凭据并识别来源。
3. 解析协议、模型别名和路由策略。
4. 建立请求级 Usage累加器和取消令牌。
5. 选择后端并进行必要的最小协议转换。
6. 以有界缓冲和背压方式转发流式响应。
7. 客户端断开时立即取消上游。
8. 使用后端最终 Usage；缺失时明确标记`unavailable`，当前版本不伪造估算值。
9. 请求完成后向数据库写入一条 Usage事件。
10. 统一返回错误格式并保存脱敏错误类别。

同协议转发应尽量直接传递响应字节，不缓存完整正文。跨协议转换只解析完成转换所必需的字段。

迭代2已实现第一条OpenAI兼容纵向闭环：桌面后台绑定`127.0.0.1:10100`，Models与Chat入口先使用本地Key摘要认证，再以独立后端凭据转发。非流式响应有16 MiB上限；SSE由下游轮询驱动上游读取，Drop下游响应即可取消上游。最终Usage经容量1024的专用单写入线程进入SQLite schema v2，每个请求最多写一条。当前后端和Key使用明确标记为开发期的进程配置，正式配置界面及系统凭据存储在后续迭代接入。详见[Gateway开发说明](GATEWAY_DEVELOPMENT.md)。

迭代4把 Gateway后端改为共享动态配置：托管`llama-server`通过健康检查后，`hal100-active`被路由到当前会话随机选择且独立认证的回环端口；停止或切换时恢复/替换该配置。内置模型测试使用只存在于当前桌面进程内的随机客户端Key，响应体按数据块限制为2 MiB，提示词和回答不落库。Usage统计读取聚合总数和最近50条请求，没有前端轮询。

迭代5协议子阶段将Chat Completions、Responses和Messages收敛到同一个请求生命周期。请求只解析模型名和流式标志，正文与工具结构保持同协议透明；Responses最终事件与Anthropic`message_start/message_delta`累计Usage被标准化后写入schema v6。Messages入口接受Anthropic SDK的`x-api-key`，但Gateway只使用后端配置中的独立认证信息访问上游。

路由核心使用单个内存快照保存活动后端、可路由后端和模型别名。请求以短时读锁完成选择，并在释放锁前原子增加对应后端的活动请求计数；响应或流式Body被丢弃时由请求租约递减。安全切换串行化管理操作，标记旧后端进入排空、拒绝新请求、等待计数归零，再以短时写锁替换活动后端。超时会移除排空标记并保留旧配置。托管llama.cpp切换复用同一路径；激活失败时回收新进程，停止失败时保留仍在服务的进程和路由。

强制切换与安全切换共用同一管理操作串行锁，但不等待活动计数归零。每个请求在路由读锁内取得当前代次的取消令牌并增加租约计数；强制替换在写锁内取消旧代、创建新代并替换活动后端。旧请求在发送、收集或流式轮询点终止并标记`forced_route_switch`，新请求绑定新代，不受旧令牌影响。外部后端、托管引擎切换与强制停止都从Rust原生确认后进入该路径。

每个后端活动对象还保存两个原子故障字段：连续失败数和熔断截止时刻。请求热路径只增加一次原子读取；达到阈值后以相对Gateway启动时刻打开15秒熔断，下一次访问惰性清除过期状态，没有定时器或维护任务。只有幂等GET可以在请求作用域内进行一次有限重试；推理POST不重试。Keychain、SQLite和发现探测均不进入Gateway热路径。

## 7. 协议面

首版网关目标：

```text
GET  /v1/models
POST /v1/chat/completions
POST /v1/responses
POST /v1/messages
```

必须覆盖：

- 流式和非流式响应。
- Tool Calling。
- 请求取消。
- Usage和缓存 Token。
- 统一错误映射。
- 后端能力发现。

Embedding、Reranking和音视频协议不属于初始闭环，后续依据实际客户端需求增加。

## 8. 路由模型

- `hal100-active` 指向当前活动路由。
- 显式模型别名可以指向不同后端。
- 客户端/API Key可拥有默认路由，但不绕过显式模型选择。
- 切换路由采用原子更新。
- 默认等待旧路由活动请求排空。
- 强制切换必须产生用户确认并标记被中断请求。

以上路由要求均已实现。外部后端在保存或重启恢复时进入同一原子路由表，API Key由平台`SecretStore`提供，路由热路径不访问Keychain或SQLite。

本机发现不是常驻服务。用户点击后才创建3个固定回环请求，分别检查Ollama默认端口及vLLM、llama.cpp Server常用端口；结果是“候选”而非所有权或确定进程身份。局域网服务继续依靠用户明确输入URL，避免后台广播、端口扫描和隐私暴露。

## 9. 数据架构

SQLite是唯一持久化事实源，但不保存云端 API Key和明文客户端密钥。概念实体包括：

- `settings`
- `models`
- `model_locations`
- `backends`
- `routes`
- `client_apps`
- `api_key_hashes`
- `usage_requests`
- `usage_daily`
- `downloads`
- `integrations`
- `operations`
- `audit_events`
- `agent_sessions`

当前schema v7中的实际表名为`backends`与`model_routes`；活动外部后端ID使用`settings.gateway.active_backend_id`保存。`backends.credential_id`只是系统凭据引用，不是密钥。启动恢复先读取非敏感元数据，再从Keychain解析凭据；凭据缺失时不加载对应运行态后端和别名。向导步骤、登录启动询问状态、下载源和保留天数都写入类型化`settings`键；保留天数仅允许30、90、180、365或永久。通用客户端复用`client_apps`和`api_key_hashes`，只存客户端ID、显示名、Key前缀和SHA-256摘要。

数据库使用显式版本迁移。Usage通过单写入队列进入 WAL事务；不得按流式 Token逐条写入。

## 10. 模型与下载源

下载抽象必须让 Hugging Face和ModelScope使用同一任务状态机：

```text
Pending → Downloading → Verifying → Installing → Ready
              ├→ Paused
              └→ Failed/Cancelled
```

下载任务需记录来源、仓库、修订版本、文件、许可证、预期大小、哈希、目标路径和可恢复状态。默认源由用户设置，但模型记录必须保留实际来源。

Alpha只接受公开且具有确定SHA-256的GGUF。下载计划重新解析权威元数据并检查“文件大小 + 512 MiB”空间；分片和目标位于同卷，Range恢复会严格验证`Content-Range`，最终通过哈希、GGUF和原子重命名后才在事务中标记`ready`。应用重启把活动任务暂停，不会静默恢复网络传输。

## 11. HAL100 Agent

Agent由六层组成：

1. Model Provider：本地小模型或用户主动启用的云端模型。
2. HAL100 Gateway：统一代理模型请求、隔离云端凭据并记录 Agent Usage。
3. Agent Kernel Sidecar：使用 Pi Agent Core管理多轮工具调用、流式事件、取消和上下文；使用 Pi AI适配模型协议。
4. Tool Broker：把 Sidecar工具请求转换为 HAL100内部命令，并把进度和结果返回 Agent。
5. Policy Engine：验证工具、参数、风险等级、所有权和确认状态。
6. Deterministic Executor：执行明确、可测试、可审计的 Rust操作。

模型输出始终视为不可信输入。Agent不能直接调用任意 Shell、删除任意路径或读取任意文件。

### 11.1 Pi集成边界

- 使用 `@earendil-works/pi-agent-core` 和 `@earendil-works/pi-ai`，不嵌入完整 `pi-coding-agent`。
- 不加载 Pi TUI、内置 `bash`/`read`/`write`/`edit`等 Coding Tools。
- 不发现 `~/.pi`、项目 `.pi`、`AGENTS.md`、第三方扩展、Skills、Prompt Templates或主题。
- HAL100系统提示、工具 Schema和会话策略由 HAL100提供。
- Sidecar通过 HAL100自有版本化 RPC与 Rust通信，不把 Pi CLI RPC作为稳定内部协议。
- Pi API变化被限制在 Sidecar适配层内，Rust领域类型不依赖 Pi类型。
- Sidecar使用短生命周期 Gateway凭据；云端密钥只存在于系统凭据库和 Rust代理路径。
- Pi会话文件存储默认禁用；需要持久化的会话状态由 HAL100数据策略统一管理。
- Sidecar使用固定代码、精确依赖锁、最小环境变量和固定工作目录，禁止运行时安装包或动态加载用户代码。

禁用 Pi工具只限制模型可以触发的能力。未落实平台进程沙箱时，Sidecar本身仍是以当前用户身份运行的受信任第三方代码；该供应链残余风险必须保留在威胁模型中，不能表述为操作系统级文件或网络隔离。

迭代1的可移植启动基线会规范化运行时和入口路径，要求入口与工作目录位于 HAL100受控根目录，清空父进程环境，并为 Sidecar创建独立 HOME/TMPDIR。macOS未签名开发版可显式启用弃用的 `sandbox-exec`配置做拒绝路径回归，但不能作为发布安全边界，也不能在不可用时静默降级。未来进入签名阶段后，优先验证具有独立权限的 XPC服务或 helper app；单纯让命令行 helper继承主应用沙箱无法获得比主应用更窄的权限集合。详细结论见 [Agent Sidecar隔离验证](SIDECAR_ISOLATION.md)。

### 11.2 工具调用生命周期

```text
模型提出工具调用
→ Pi Agent Core进行基础 Schema解析
→ Sidecar发送 tool.request
→ Rust Tool Broker按 HAL100 Schema重新解析
→ Policy Engine检查工具白名单、风险、参数、目标和所有权
→ 需要确认时由 Rust/UI创建绑定精确参数的一次性确认令牌
→ Deterministic Executor执行受控操作
→ 审计并返回结构化进度/结果/错误
→ Pi Agent Core决定解释、修正参数或进行下一步
```

Pi的执行前后钩子只能作为额外防线，不能替代 Rust复验。Sidecar内不存在可绕过 Tool Broker的通用执行工具。

迭代1先用确定性 Faux模型完成模拟边界验证。迭代7已将同一协议升级为真实产品链，迭代10升级为RPC v2，迭代11再升级为RPC v3；迭代12把桌面Agent拆为稳定外观与五个变化边界：`agent_coordinator`负责能力需求、完成校验和任务取消生命周期，`agent_kernel`负责固定Node/Sidecar与RPC传输，`agent_tools`负责工具授权和确定性计划编排，`agent_action`负责一次性待确认计划状态，`agent_provider`负责本地/云端Provider与内存会话。迭代13将私有协议升级为RPC v4，`agent.run.start.requiredTools`直接承载由能力注册表生成的规范有序能力集合，不再为每项工具扩展布尔字段。`AgentService`继续负责运行互斥、临时凭据/路由装配、原生确认后的确定性执行、审计和错误兼容。Sidecar通过临时客户端Key向Gateway请求内部保留模型别名`hal100-agent`。独立`AgentModelRuntime`复用已校验的Qwen3.5-2B Q4_K_M权重，但使用单独的llama-server、随机回环端口、临时后端Key、内部后端和模型路由，不修改用户活动后端。运行时使用6144上下文、最多768输出Token、parallel 1和reasoning off；空闲2分钟后通过一次性generation timer停止，没有轮询。

产品Sidecar当前注册13个HAL100代理工具：既有10项工具保持语义，新增公开模型目录搜索、仓库GGUF检查和下载计划。搜索使用数据库中的用户默认来源并只返回最多8个公开仓库；仓库工具只能引用同任务搜索结果并返回最多12个带可信SHA-256的GGUF；下载工具只能引用该快照中的精确相对路径。`ModelDownloadManager`仍是远端复验、空间、重复项、目标路径、底层计划、下载、哈希/GGUF校验和原子安装的唯一事实状态源，Agent不实现第二套下载逻辑。模型与引擎计划必须先完成运行目录读取；OpenCode配置计划必须先完成OpenCode状态检查；诊断修复必须先在同一任务取得Rust报告并复制精确`reportId/findingId`。每个模型回合只暴露`requiredTools`中的唯一下一工具并使用`required`；如果模型提前给文字答案，Pi会话最多追加三次固定纠偏提示，Provider固定错误立即失败。Rust Tool Broker复验工具名、精确参数、run ID、tool call ID、唯一性、规范顺序、前置闭包和RPC v4当前每任务最多4次调用；每项任务最多产生一个可写计划。共享工具策略还固定读/计划效果、前置关系、原生确认、参数正反例和128 KiB结果预算，Rust与TypeScript分别验证。4项是协议版本的单任务预算而非软件规模上限。Sidecar筛选只提高小模型可靠性，不是权限边界。完成结果如果缺少必需工具、工具关联不匹配、回答/结果过长、协议异常或进程超时，整个任务失败关闭。通用聊天由Rust领域门禁在模型启动前拒绝。

`EnvironmentDiagnostics`属于Infra层的同步按需服务：只刷新模型文件存在性/廉价快照，读取引擎状态、Gateway路由与熔断快照、OpenCode检测结果，不读原始日志、不做完整模型哈希、发现数量上限64。直接桌面诊断不启动Pi或Qwen；Agent诊断只把脱敏DTO交给模型。当前只有引擎未安装、OpenCode已安装但未配置、非内置模型文件缺失三类发现带`repairKind`。修复工具不信任旧报告：Rust在计划生成前重新检查引擎安装态、OpenCode所有权或模型仍为`Missing`，再复用既有确定性计划。用户原生确认执行成功后运行一次新的诊断并返回界面；复检失败只写固定错误码，不泄漏底层路径，也不把已成功操作改判为失败。

Agent计划是Rust内存中的一次性能力对象：绑定生成任务、精确目标、当前状态、内部确定性管理器计划、5分钟到期时间和`requires_native_confirmation`，且只保留最新一项；底层管理器计划ID不发送给Pi。新任务、取消、失败或用户取消原生确认会同时废弃外层与底层计划；伪造、超长、过期、已消费或缺少原生确认标记的ID均拒绝。WebView只能请求Rust显示原生确认；确认后Rust再次取走并校验同一计划，再调用确定性管理器。模型移除还在引擎生命周期锁内复核活动模型，托管文件只能进入系统废纸篓，外部文件只能移除索引，内置Agent模型直接拒绝。因此“模型说已执行”、Pi工具成功或聊天中的同意都不是授权。

活动任务由`agent_coordinator::AgentRunRegistry`绑定独立取消原子标记和精确run租约。模型资源SHA-256每读取1 MiB检查，健康等待每50 ms、RPC接收与远端目录future每100 ms检查，不等待15/90/180秒超时；取消时丢弃HTTP future，随后Rust终止Sidecar、移除临时Gateway凭据和会话目录、停止独立Agent模型并写固定审计。`active_run_id`和`cancellation_requested`只作为状态展示，不授予额外能力。

Gateway会话Key仅存在于运行内存和RPC请求，使用RAII在任务结束后从认证注册表移除；Sidecar的单次HOME/TMP目录也在进程结束后删除。数据库只保存Agent开始、完成、失败、取消、模型运行时和操作计划的固定白名单元数据，不保存提示词、回答、Key或完整路径。用户配置层不能创建、删除或从数据库恢复内部保留别名`hal100-agent`。

迭代8云端闭环复用现有外部后端目录与Keychain恢复路径，不建立第二套凭据存储。UI的云端目标只有`backendId + model`；Rust确认后重新验证后端已启用、类型为OpenAI/Anthropic兼容、存在Keychain引用且已载入Gateway，再为每项任务创建`hal100-agent-cloud-<随机ID>`临时内部路由。Sidecar的RPC显式携带`localOpenAi`、`cloudOpenAi`或`cloudAnthropic`协议标记，但仍只携带短生命周期本机Gateway Key。OpenAI请求进入Gateway的`/v1/chat/completions`，Anthropic请求进入`/v1/messages`；Gateway才从内存后端配置注入真正的上游认证。临时路由由RAII删除，不改变`hal100-active`或用户持久化别名。

当前会话模式是Provider授权上下文，不是聊天历史：`AgentService`只在内存保存云端目标、启用时间和最后固定错误码。启动、退出与任务启动共用一个非阻塞运行锁；任务取得锁后才解析内存目标，保证成功退出后不会有旧会话请求越过边界。新服务实例的字段固定初始化为空，所以隐藏/显示窗口保留同一进程状态，而明确退出或应用重启恢复本地默认。状态查询会重新验证数据库后端与Gateway载入状态；删除、禁用、失载或认证失败只把会话标为不可用/上次失败，不切换Provider。单次模式仍逐任务原生确认；会话模式只在启用范围时原生确认，后续每项任务仍创建独立路由、Key、Sidecar和会话目录。

本地模式才会启动独立Qwen运行时；云端模式不调用本地运行时启动路径，任何认证、路由、连接或模型错误均原样失败关闭，不存在本地回退。云端任务使用独立客户端归属`hal100-agent-cloud`写入Usage。若本地Qwen正在空闲倒计时，云端任务不会取消该代次；释放任务在单次云端任务持有运行锁时以100毫秒低频等待，锁释放后立即停止本地运行时，避免云端任务造成模型长期驻留。

### 11.3 依赖隔离

实现时固定 Pi精确版本并提交依赖锁。升级必须通过工具协议、Provider、流式、取消、安全、性能和 Apple Silicon回归测试。未来 Windows 10/11使用同一 HAL100 RPC契约，平台差异只存在于 Sidecar构建和 Rust平台实现。

## 12. OpenCode集成

OpenCode适配器必须按版本处理配置格式差异。修改流程：

```text
Detect → Parse → Validate → Preview → Confirm → Backup → Atomic Patch → Verify
                                                   └→ Rollback on failure
```

全局配置是默认目标；项目级配置只诊断不自动修改。HAL100只管理带有自己标记和安装记录的配置片段。

实现使用5分钟有效的一次性计划将预览与执行分离。预览只包含HAL100将写入的语义字段，不把现有配置正文或凭据发送给WebView。确认后Rust Core重新比较原文件SHA-256，再创建原始字节备份并通过同目录临时文件、`fsync`和原子替换提交。OpenCode专属Key位于HAL100应用数据目录的`0600`文件，OpenCode配置只保存`{file:...}`引用。

SQLite schema v8的`integrations`记录主配置路径、凭据路径和受管分片语义哈希；
`integration_resources`登记配置、凭据和辅助配置资源，`api_key_hashes`只保存Key摘要。资源、
接入与凭据在同一事务中写入，成功后共享CredentialRegistry热更新，Gateway无需重启。
OpenCode检测和配置只由界面或Agent受控工具按需触发，不存在常驻轮询任务。

### 12.1 Pi Coding Agent集成

Pi适配器只管理`~/.pi/agent/models.json`中的`providers.hal100`。该文件按官方契约使用严格
JSON；HAL100不读取或修改`settings.json`、`auth.json`、会话、扩展、Skills、Prompt模板或
项目`.pi`资源。用户独立安装和升级官方`pi`，HAL100不运行安装器，也不把全局Pi模块加入
内置Sidecar解析路径。

Pi Provider使用`openai-completions`和版本化`hal100-active`模型描述。专属Gateway Key位于
HAL100应用数据目录的`0600`文件，`models.json`只保存经过Shell单引号转义的固定
`!/bin/cat '<absolute-path>'`读取命令；适配器不接受任意命令或用户脚本。配置和断开都使用
独立的一次性计划、摘要复验、备份、同目录原子替换、严格解析验证、事务提交和失败回滚。
外部Pi的Gateway身份固定为`pi-coding-agent`，不会与内置Runtime的`hal100-agent`共享Usage。

## 13. 平台抽象

领域核心只依赖平台接口：

- 凭据存储。
- 废纸篓。
- 登录启动。
- 进程和硬件检测。
- 文件系统通知。
- 模型与应用数据目录。
- OpenCode配置路径。

首版只实现 macOS/Apple Silicon。未来 Windows 10/11实现同一接口，不在核心业务代码中散落条件编译。

## 14. 首版不引入独立系统守护进程

当前由无可见窗口的 Tauri Core长期驻留；主 WebView可能尚未创建或处于隐藏状态。`hal100-core` 必须保持为独立 Rust库，以便未来在可靠性需求成立时提取为用户级后台进程，而无需重写业务逻辑。
