# HAL100 Agent RPC

该目录是 Rust Core 与 Agent Kernel Sidecar 的稳定私有协议说明。当前v4使用四字节大端长度前缀加UTF-8 JSON；单帧最大1 MiB。v4以有序`requiredTools`能力集合替代v3逐工具布尔字段，并增加模型目录搜索、仓库检查与下载计划，不兼容v3。

## stdout / stderr

- stdin：Rust Core 发送的协议帧。
- stdout：Sidecar 返回的协议帧，禁止写日志和普通文本。
- stderr：脱敏诊断日志，禁止输出提示词、回答、凭据和完整用户路径。

## v4 envelope

```json
{
  "protocolVersion": 4,
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
Rust → agent.run.start
Pi   → tool.call.request
Rust → tool.call.result
Pi   → agent.run.completed
Rust → system.shutdown
Pi   → system.shutdown.ack
```

`tool.call.request`包含 `runId`、`toolCallId`、`toolName`和 `arguments`。`tool.call.result`复用请求 envelope的 `id`，同时必须携带相同的 `toolCallId`。Sidecar对未知结果、错误关联和超时全部故障关闭；Rust不信任 Sidecar中的 Typebox校验，仍按自己的 DTO、白名单和参数规则重新校验。

当前产品Sidecar注册13个工具。`v4-tools.json`共同固定协议级任务预算、精确顺序、读/计划效果、前置关系、原生确认要求和参数正反例；Rust能力注册表、Rust Tool Policy与TypeScript TypeBox测试分别读取并验证同一清单：

| 工具 | 参数 | Rust结果与限制 |
| --- | --- | --- |
| `hal100.inspect_system_summary` | 精确`{"detail":"summary"}` | Apple Silicon硬件与磁盘摘要；不返回路径 |
| `hal100.inspect_runtime_catalog` | 精确`{"detail":"summary"}` | 引擎、活动路由和本地模型脱敏目录；不返回路径或凭据 |
| `hal100.plan_model_start` | 仅`{"modelId":"<1—128字符>"}` | 只有同一任务先完成目录读取后，才生成一次性计划；不执行模型操作 |
| `hal100.plan_model_removal` | 仅`{"modelId":"<1—128字符>"}` | 先读取目录；Rust按所有权生成废纸篓或仅移除索引计划；内置Agent模型拒绝 |
| `hal100.inspect_environment_diagnostics` | 精确`{"target":"full"}` | Rust按需生成有界脱敏快照；不读取原始日志、不执行完整模型哈希、不返回路径 |
| `hal100.plan_diagnostic_repair` | 仅`{"reportId":"<1—128字符>","findingId":"<1—128字符>"}` | 只接受同一任务刚生成报告中带`repairKind`的一项；生成计划，不执行修复 |
| `hal100.plan_engine_install` | 精确`{"target":"llama.cpp"}` | 先读取目录；只为固定官方构建生成安装计划 |
| `hal100.plan_engine_remove` | 精确`{"target":"llama.cpp"}` | 先读取目录；只为HAL100托管引擎生成卸载计划，不涉及模型 |
| `hal100.inspect_opencode_status` | 精确`{"target":"opencode"}` | Rust检查安装、全局配置和Provider所有权；Sidecar不读配置文件 |
| `hal100.plan_opencode_configuration` | 精确`{"target":"opencode"}` | 先完成OpenCode状态检查；生成保留用户默认设置并拒绝冲突Provider的配置计划 |
| `hal100.search_model_catalog` | 仅`{"query":"<2—100字符>"}` | 使用用户在HAL100选择的默认来源；最多返回8个公开、非gated仓库摘要 |
| `hal100.inspect_model_repository` | 仅`{"repository":"owner/name"}` | 必须精确引用同任务搜索结果；最多返回12个带可信SHA-256的GGUF文件 |
| `hal100.plan_model_download` | 仅`{"remotePath":"<相对路径>"}` | 必须精确引用同任务仓库快照；Rust重新拉取元数据并复验来源、仓库、修订、文件、哈希、重复项与空间，只生成一次性计划 |

Rust同时验证run ID、tool call ID、重复调用、调用顺序和RPC v4当前每任务最多4次工具调用。`agent.run.start.requiredTools`必须是注册表规范顺序、无重复、包含全部前置能力且最多含一个写计划能力；`agent.run.completed`必须报告固定的13个注册工具、实际完成工具名与数量。每个成功工具结果的序列化载荷最多128 KiB，明显低于1 MiB帧上限；Rust发送前和Sidecar接收后分别校验。必需工具缺失、计划类型错误、结果/回答过长或任何关联不一致均拒绝整个任务。4项是RPC v4的版本化单任务复杂度预算，不是源文件、模块数量或产品能力上限；后续合法工作流需要更多步骤时必须显式评审并更新共享策略。

诊断报告只在当前Rust调用栈和前端显式查询结果中存在，不作为长期授权。当前可自动生成计划的修复只有三类确定性动作：安装缺失的固定llama.cpp构建、为已安装且未配置的OpenCode生成配置计划、清理文件仍缺失的非内置模型索引。引擎校验失败、模型变化/校验失败、配置冲突和后端熔断只报告，不自动修复。Rust在计划生成前重新检查现实状态；用户原生确认执行后再运行一次诊断，复检失败不会回滚或伪装已经成功的确定性操作。

`agent.run.start`显式包含`requiredTools`、`providerProtocol`和`modelId`。协议值仅允许`localOpenAi`、`cloudOpenAi`、`cloudAnthropic`；本地模型ID必须精确等于`hal100-agent`，云端模型ID必须是Rust生成的`hal100-agent-cloud-`临时别名。RPC中的`apiKey`始终是当前任务的短生命周期本机Gateway Key，不是云端后端Key；消息中不存在Keychain引用或上游凭据字段。OpenAI路径由Pi适配到Gateway `/v1/chat/completions`，Anthropic路径适配到Gateway `/v1/messages`。

单次云端和当前内存会话使用同一逐任务RPC合同。会话授权只存在于Rust `AgentService`内存，不进入Sidecar会话；即使当前Provider范围连续使用云端，每项任务仍得到新的随机模型别名和Gateway Key，Sidecar不会继承上一项任务的提示词、回答或凭据。

Provider每一回合只获得`requiredTools`中当前缺失的一个工具定义并使用`tool_choice: "required"`；完成后才暴露下一工具。若模型在成功工具结果后提前结束，Sidecar最多在同一Pi会话追加三次固定纠偏提示。该机制不执行工具、不能扩大白名单；固定认证、路由和连接错误不会进入纠偏重试。

可写计划不会通过RPC执行。公开计划只存在于Rust内存，绑定生成run、精确目标、当前状态、5分钟到期时间和原生确认要求，只保留最新且只能消费一次；底层管理器计划ID不发送给Pi。Tauri执行命令必须先由Rust显示原生确认，之后再次取走并校验同一计划。新任务、取消、失败或取消确认会同步废弃底层计划。

取消由Rust进程外控制：活动run持有原子取消标记，模型哈希每1 MiB、健康等待每50 ms、RPC接收和远端目录工具等待每100 ms检查；目录HTTP future在取消时直接丢弃，不再等待15秒请求超时。取消后父进程终止Sidecar并回收模型、未使用下载计划和临时资源，不向不可信Sidecar请求“同意取消”。

确定性模拟Broker仍只执行`hal100.inspect_system_summary`并返回`simulated=true`固定结果；其余工具只验证策略授权，不在模拟器中触发产品状态或操作。
