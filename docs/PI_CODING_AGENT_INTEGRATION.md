# Pi Coding Agent接入

本文描述用户独立安装的官方Pi Coding Agent如何连接HAL100。它不描述HAL100内置Agent
Runtime；内置Runtime只使用固定Pi Agent Core/Pi AI库，并拥有独立进程、临时HOME、会话和
`hal100-agent`身份。

## 所有权边界

HAL100只拥有两项资源：

1. `~/.pi/agent/models.json`中的`providers.hal100`对象。
2. HAL100应用数据目录中的`credentials/pi-coding-agent-gateway.key`。

用户的默认Provider/模型、`settings.json`、`auth.json`、会话、扩展、Skills、Prompt模板、
项目`.pi`资源和Pi安装本身均不属于HAL100。检测到`PI_CODING_AGENT_DIR`指向其他目录时只
显示提示，默认不会写入该目录。

## 配置契约

Provider固定使用：

- Base URL：当前HAL100 Gateway的`/v1`地址。
- API：`openai-completions`。
- 模型ID：`hal100-active`。
- 模型能力：来自HAL100版本化`managed-route-v3`合同，当前为text输入、Rust设备策略选择的
  16384/32768上下文、1024最大输出。该值与HAL100托管llama.cpp实际`--ctx-size`一致，不虚报
  活动路由容量。
- 凭据：`0600`独立文件；配置只保存固定`/bin/cat`读取命令，不保存明文Key。

HAL100不在Pi配置中声明上游没有定义的`supportsTools`字段。工具能力由Gateway模型契约和
真实CLI验收保证；未来能力变化通过模型契约revision触发配置刷新。

## 生命周期

```text
Detect → Strict JSON Parse → Ownership Check → Preview → Native Confirm
       → Digest Recheck → Backup → Atomic Patch → Strict Verify
       → Credential Hot Register → SQLite Transaction
       └──────────────────────────── Rollback on failure
```

配置和断开各自使用独立、5分钟、一次消费的计划。用户取消时计划立即丢弃；确认前文件或
凭据发生变化时拒绝写入。断开只移除`providers.hal100`和Pi专属Key，并保留配置备份。

## 共存保证

- 官方`pi`不会进入HAL100 Sidecar的命令发现或模块解析。
- 外部Pi不读取HAL100内置Agent的临时HOME、会话或短期Key。
- Gateway Usage使用`pi-coding-agent`归属；内置Agent始终使用`hal100-agent`。
- 配置或撤销Pi不会增加、删除或修改OpenCode、OpenClaw、Hermes或通用客户端凭据。

## 验收

自动单元测试覆盖严格JSON、用户字段保留、外部修改拒绝、路径Shell转义、0600凭据、计划
丢弃、配置回滚和精确断开。隔离端到端测试下载固定官方
`@earendil-works/pi-coding-agent@0.84.2`，在临时HOME运行JSON模式，通过真实HAL100
Gateway请求SSE模拟后端，并验证Usage只归属`pi-coding-agent`。
