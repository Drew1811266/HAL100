# HAL100 Agent RPC

该目录是 Rust Core 与 Agent Kernel Sidecar 的稳定私有协议说明。当前v12使用四字节大端长度前缀加UTF-8 JSON；单帧最大1 MiB。v12保留v11的按需Pi结构化意图和任务级效率指标，当前扩展到19项工具，并在意图/执行请求中增加Rust已选容量档案；协议版本精确匹配，不兼容v11。

## stdout / stderr

- stdin：Rust Core 发送的协议帧。
- stdout：Sidecar 返回的协议帧，禁止写日志和普通文本。
- stderr：脱敏诊断日志，禁止输出提示词、回答、凭据和完整用户路径。

## v12 envelope

```json
{
  "protocolVersion": 12,
  "id": "request-id",
  "kind": "system.ping",
  "payload": {}
}
```

不兼容字段或语义变更必须增加协议版本。Rust 与 TypeScript 测试共同读取 `tests/fixtures/agent-rpc` 下的样例。

## 工具调用边界

产品任务使用以下消息序列；迭代1的`agent.simulation.*`确定性Faux模型路径继续只用于边界回归：

```text
Rust → system.ping
Pi   → system.pong
Rust → agent.intent.start       # 仅确定性路由为 Unresolved 时
Pi   → agent.intent.completed   # 规范化提案、固定无效状态或固定失败码
Rust → agent.run.start
Pi   → tool.call.request
Rust → tool.call.result
Pi   → agent.run.completed
Rust → system.shutdown
Pi   → system.shutdown.ack
```

`tool.call.request`包含 `runId`、`toolCallId`、`toolName`和 `arguments`。`tool.call.result`复用请求 envelope的 `id`，同时必须携带相同的 `toolCallId`。Sidecar对未知结果、错误关联和超时全部故障关闭；Rust不信任 Sidecar中的 Typebox校验，仍按自己的 DTO、白名单和参数规则重新校验。

`agent.intent.start`只携带提示词和当前任务的Gateway路由信息，不携带工具列表。Pi在独立、零工具调用中只能生成`contracts/agent-intent/v1-schema.json`允许的任务、澄清、拒绝或未解析提案。Sidecar解析后只发送规范化对象；Markdown、额外字段、未知枚举、超过2 KiB的结果和Provider原始错误均不会进入Rust。Rust再次验证schema、19类任务、目标类型、四个外部Agent ID和Provider，再与确定性结果裁决。迭代35起，可信任务由Rust工作流映射叶能力并通过能力注册表闭合前置关系，生成后续`agent.run.start.requiredTools`；Pi仍不能提交或扩大工具。澄清/拒绝由Rust固定回答，含工具但未解析的请求故障关闭，只有零工具解释保留兼容路径。

当前产品Sidecar注册19个工具。`v12-tools.json`共同固定协议级任务预算、精确顺序、读/计划效果、前置关系、原生确认要求和参数正反例；Rust能力注册表、Rust Tool Policy与TypeScript TypeBox测试分别读取并验证同一清单：

| 工具 | 参数 | Rust结果与限制 |
| --- | --- | --- |
| `hal100.inspect_system_summary` | 精确`{"detail":"summary"}` | Apple Silicon硬件与磁盘摘要；不返回路径 |
| `hal100.inspect_runtime_catalog` | 精确`{"detail":"summary"}` | 引擎、活动路由和本地模型脱敏目录；不返回路径或凭据 |
| `hal100.plan_model_start` | 仅`{"modelId":"<1—128字符>"}` | 只有同一任务先完成目录读取后，才生成一次性计划；不执行模型操作 |
| `hal100.plan_model_stop` | 仅`{"modelId":"<1—128字符>"}` | 只接受本次目录中的当前活动模型；生成一次性停止计划，不删除模型或索引，仍需原生确认 |
| `hal100.plan_model_removal` | 仅`{"modelId":"<1—128字符>"}` | 先读取目录；Rust按所有权生成废纸篓或仅移除索引计划；内置Agent模型拒绝 |
| `hal100.inspect_environment_diagnostics` | 精确`{"target":"full"}` | Rust按需生成有界脱敏快照，覆盖Gateway、引擎、模型库和四个外部Agent；不读取原始日志、不执行完整模型哈希、不返回路径 |
| `hal100.plan_diagnostic_repair` | 仅`{"reportId":"<1—128字符>","findingId":"<1—128字符>"}` | 只接受同一任务刚生成报告中带`repairKind`的一项；生成计划，不执行修复 |
| `hal100.plan_engine_install` | 精确`{"target":"llama.cpp"}` | 先读取目录；只为固定官方构建生成安装计划 |
| `hal100.plan_engine_remove` | 精确`{"target":"llama.cpp"}` | 先读取目录；只为HAL100托管引擎生成卸载计划，不涉及模型 |
| `hal100.inspect_external_agent` | 仅`{"integrationId":"<固定枚举>"}` | Rust检查安装、受管配置和所有权，并明确返回是否存在HAL100私有运行时；只返回脱敏状态，不返回二进制或配置路径 |
| `hal100.plan_external_agent_configuration` | 仅`{"integrationId":"<固定枚举>"}` | 先完成同一Agent状态检查；生成带快照校验、独立凭据、备份、写后验证与失败回滚的配置事务计划 |
| `hal100.plan_external_agent_disconnection` | 仅`{"integrationId":"<固定枚举>"}` | 先完成同一Agent状态检查；只移除HAL100受管片段和专属凭据，不删除用户配置 |
| `hal100.search_model_catalog` | 仅`{"query":"<2—100字符>"}` | 使用用户在HAL100选择的默认来源；最多返回8个公开、非gated仓库摘要 |
| `hal100.inspect_model_repository` | 仅`{"repository":"owner/name"}` | 必须精确引用同任务搜索结果；最多返回12个带可信SHA-256的GGUF文件 |
| `hal100.plan_model_download` | 仅`{"remotePath":"<相对路径>"}` | 必须精确引用同任务仓库快照；Rust重新拉取元数据并复验来源、仓库、修订、文件、哈希、重复项与空间，只生成一次性计划 |
| `hal100.inspect_operational_history` | 精确`{"target":"recent"}` | 最多返回24条事件类型、目标类型、时间和固定错误/动作标识；删除目标ID、提示词、回答、路径、配置与凭据 |
| `hal100.observe_operational_health` | 精确`{"target":"deployment","sampleCount":3}` | Rust复用全面诊断并在固定短窗口内采集3次引擎、活动路由、后端数量和熔断计数；只返回聚合状态与固定故障码，不读取原始日志或创建后台监控 |
| `hal100.plan_external_agent_installation` | 仅`{"integrationId":"<固定枚举>"}` | 先完成同一Agent状态检查；当前只为未安装的Pi Coding Agent核对固定官方包、版本、Registry、SRI与完整依赖闭包并生成HAL100私有安装计划；不执行安装 |
| `hal100.plan_managed_external_agent_removal` | 仅`{"integrationId":"<固定枚举>"}` | 先完成同一Agent状态检查且`managedInstallation=true`；只为HAL100私有Pi运行时生成移入系统废纸篓的计划，不触碰用户安装、配置或会话 |

Rust同时验证run ID、tool call ID、重复调用、调用顺序和RPC v12当前每任务最多4次工具调用。`agent.run.start.requiredTools`必须是注册表规范顺序、无重复、包含全部前置能力且最多含一个写计划能力；`agent.run.completed`必须报告固定的19个注册工具、实际完成工具名与数量。每个成功工具结果的序列化载荷最多128 KiB，明显低于1 MiB帧上限；Rust发送前和Sidecar接收后分别校验。必需工具缺失、计划类型错误、结果/回答过长或任何关联不一致均拒绝整个任务。4项是RPC v12的版本化单任务复杂度预算，不是源文件、模块数量或产品能力上限；后续合法工作流需要更多步骤时必须显式评审并更新共享策略。

v12的`agent.run.completed.efficiency`只包含数值：上下文/输出预算、意图与执行模型回合、固定纠偏次数、Provider报告的输入/输出与峰值、装配估算峰值、裁剪轮次，以及发送/重复工具结果的字节和Token估算。Rust按当前Provider复核固定容量、回合上限、Usage关系和重复量不大于发送量；任何越界都拒绝整项任务。指标不包含提示词、回答、工具参数或原始工具结果。工具结果Token使用`ceil(可见字符数/4)`，只做同场景上下文比较，不替代Gateway精确Usage。

诊断报告只在当前Rust调用栈和前端显式查询结果中存在，不作为长期授权。当前可自动生成计划的修复只有三类确定性动作：安装缺失的固定llama.cpp构建、为已安装且未配置或需要刷新的四类外部Agent生成专用配置事务、清理文件仍缺失的非内置模型索引。引擎校验失败、模型变化/校验失败、配置冲突、版本不兼容和后端熔断只报告，不自动修复。Rust在计划生成前重新检查现实状态；用户原生确认执行后再运行一次诊断，复检失败不会回滚或伪装已经成功的确定性操作。

`agent.run.start`显式包含`requiredTools`、`providerProtocol`、`modelId`、`contextWindowTokens`和`maxOutputTokens`；`agent.intent.start`携带同一容量对。两项数值只能来自Rust启动时选定的版本化设备档案。本地仅接受16K基线或已验收32K标准档，云端仍精确固定为128K/2048；Pi和Sidecar不能请求更高档，也不能用用户输入覆盖。协议值仅允许`localOpenAi`、`cloudOpenAi`、`cloudAnthropic`；本地模型ID必须精确等于`hal100-agent`，云端模型ID必须是Rust生成的`hal100-agent-cloud-`临时别名。RPC中的`apiKey`始终是当前任务的短生命周期本机Gateway Key，不是云端后端Key；消息中不存在Keychain引用或上游凭据字段。OpenAI路径由Pi适配到Gateway `/v1/chat/completions`，Anthropic路径适配到Gateway `/v1/messages`。

单次云端和当前内存会话使用同一逐任务RPC合同。会话授权只存在于Rust `AgentService`内存，不进入Sidecar会话；即使当前Provider范围连续使用云端，每项任务仍得到新的随机模型别名和Gateway Key，Sidecar不会继承上一项任务的提示词、回答或凭据。

Provider每一回合只获得`requiredTools`中当前缺失的一个工具定义并使用`tool_choice: "required"`；完成后才暴露下一工具。系统指令只装配本任务需要的能力；消息只保留原始目标、最新直接工具调用/结果对和最新固定纠偏。工具授权仍读取完整Rust任务状态，不能从裁剪后的消息推导。若模型在成功工具结果后提前结束，Sidecar最多在同一Pi会话追加两次固定纠偏提示；最终计划工具成功后立即以固定“尚未执行、等待原生确认”说明收口，不再增加解释回合。该机制不执行工具、不能扩大白名单；固定认证、路由和连接错误不会进入纠偏重试。

可写计划不会通过RPC执行。公开计划只存在于Rust内存，绑定生成run、精确目标、当前状态、5分钟到期时间和原生确认要求，只保留最新且只能消费一次；底层管理器计划ID不发送给Pi。Tauri执行命令必须先由Rust显示原生确认，之后再次取走并校验同一计划。新任务、取消、失败或取消确认会同步废弃底层计划。

取消由Rust进程外控制：活动run持有原子取消标记，模型哈希每1 MiB、健康等待每50 ms、RPC接收、远端目录工具和短时观测等待每100 ms检查；目录HTTP future在取消时直接丢弃，不再等待15秒请求超时。取消后父进程终止Sidecar并回收模型、未使用下载计划和临时资源，不向不可信Sidecar请求“同意取消”。

确定性模拟Broker仍只执行`hal100.inspect_system_summary`并返回`simulated=true`固定结果；其余工具只验证策略授权，不在模拟器中触发产品状态或操作。
