# 迭代 8：云端 Agent 双范围纵向闭环验收

日期：2026-08-18
平台：Apple Silicon macOS开发机
范围：单次云端选择；当前内存会话使用云端

## 1. 交付链路

```text
用户选择云端单次增强
→ 选择已加载且带Keychain凭据引用的OpenAI/Anthropic兼容后端
→ 填写模型ID
→ Rust生成脱敏发送预览
→ 用户点击明确的单次发送按钮
→ Rust显示macOS原生确认并重新校验目标
→ Rust创建随机临时内部模型路由
→ Sidecar使用短生命周期本机Gateway Key请求模型
→ Gateway注入Keychain恢复的上游凭据并记录Usage
→ 工具调用返回同一Rust Tool Broker
→ 任务结束后回收路由、Key、会话目录和Sidecar
```

本地Qwen始终是默认。云端分支不启动本地Agent Model Runtime，认证、路由、连接或模型失败都直接失败关闭，不存在本地回退。

当前会话链路：

```text
用户选择当前会话使用云端
→ Rust生成后续任务发送范围预览
→ Rust显示macOS原生确认并重新校验目标
→ AgentService只在内存保存backendId、model、启用时间和固定错误码
→ 后续任务在取得统一运行锁后解析该目标
→ 每项任务仍创建独立临时路由、Gateway Key、Sidecar和会话目录
→ 页面持续显示目标与健康/故障状态
→ 用户明确退出或应用重启后恢复本地Qwen默认
```

该“会话”只代表Provider授权上下文，不是多轮聊天历史；HAL100仍不保存提示词或回答。

## 2. 安全与协议结果

- 前端和`AgentPromptRequest`只有`backendId + model`，没有API Key字段。
- 云端Key继续由现有BackendManager写入macOS Keychain；SQLite只保存`credential_id`。
- Agent RPC显式区分`localOpenAi`、`cloudOpenAi`和`cloudAnthropic`，但`apiKey`只代表本机临时Gateway Key。
- OpenAI Sidecar合同固定请求`/v1/chat/completions`；Anthropic合同固定请求`/v1/messages`。
- Anthropic必需工具通过精确`tool_choice: {type:"tool", name}`约束；完成必需工具后移除工具列表。
- 临时路由使用`hal100-agent-cloud-<随机ID>`，不写数据库、不改变`hal100-active`，RAII覆盖成功、错误和取消。
- 云端任务用量以`hal100-agent-cloud`归类；本地任务继续使用`hal100-agent`。
- 审计详情只允许Provider、后端ID、模型、工具数量和固定错误码等白名单字段；不保存提示词、回答、Key和路径。
- 会话状态只在Rust内存保存，不写SQLite；启用、退出和任务启动共用运行锁，退出成功后不会再进入旧目标。
- 新`AgentService`实例在同一持久化数据库上仍默认本地；窗口隐藏/显示保留进程内状态，应用重启不恢复云端会话。
- 后端删除、失载或认证/Provider失败会保留可见会话并记录固定错误码，任务故障关闭但仍允许明确退出。

## 3. 无网模拟验收

真实纵向测试启动本机模拟OpenAI上游、真实HAL100 Gateway、真实Node/Pi Sidecar和Rust AgentService，全程不访问互联网、无需真实云端Key。结果：

- 云端任务返回预期中文答案。
- Gateway把随机内部别名解析为目标模型`cloud-test`。
- 模拟上游只看到Gateway注入的上游Bearer凭据，没有看到Sidecar本机会话Key。
- Usage精确记录17 Token，客户端为`hal100-agent-cloud`，后端为`cloud-e2e`。
- 运行前后本地Agent Model Runtime状态完全不变。
- 任务完成后临时路由和本机Agent客户端凭据均不存在。

当前会话测试单次执行约0.34秒。Sidecar协议合同套件共16项通过，其中同时覆盖OpenAI与Anthropic路径、认证头、模型别名、最大输出和必需工具约束。

## 4. 后台性能边界

云端功能没有新增轮询、常驻Sidecar、常驻模型或数据库定时任务。活动云端会话只是一个小型内存状态；后端目录和状态只在进入Agent页或用户刷新时读取，发送范围预览、启用、退出与运行均由用户动作触发。若本地Qwen正在空闲倒计时，云端任务不会旋转该计时代次；计时到期时如任务锁正被占用，只以100毫秒低频等待到锁释放并立即停止本地运行时，避免模型意外长期驻留。

最终生产构建后的5次、每次间隔2秒CPU采样为0.0%、0.0%、0.0%、0.5%、0.1%；`vmmap`物理占用42.2 MiB、峰值43.1 MiB。采样时不存在Agent Sidecar、独立Agent llama-server或临时Agent进程，Gateway `/healthz`正常，继续满足后台80 MiB预算。Agent会话目录和测试临时目录均为0；真实开发数据库审计白名单扫描未发现提示词、回答、Authorization、API Key或临时Gateway Key字段。

## 5. 自动审查结论

迭代8两条云端纵向闭环达到完成条件：未主动选择并确认时不会产生云端请求；会话授权不持久化，退出或重启恢复本地；云端密钥不进入前端、Agent RPC上游字段、Sidecar环境、SQLite、审计或日志；云端模型没有比本地模型更高的工具权限；后端失载、认证失败、请求失败和状态竞态都不回退本地。会话状态机、真实Gateway/Pi无网纵向测试、前端三态交互和1280×720无横向溢出检查均已通过。

最终全仓质量门通过：前端15项、Sidecar16项、Desktop Rust 16项（另2项真实1.28 GB模型测试按设计忽略）、Infra 66项（另2项联网目录测试忽略）、Gateway 9项（另1项独立性能探针忽略）、Protocol 14项、Core与Platform各7项；类型检查、Biome、Rustfmt、Clippy `-D warnings`和生产构建均通过。
