# OpenClaw 接入

本文描述用户独立安装的 OpenClaw 如何连接 HAL100。HAL100 不安装、升级、启动、停止或
重启 OpenClaw，也不读取它的会话、Skills 或其他工作区数据。

## 兼容基线

- 当前自动验收下限：`2026.7.1`。
- 固定官方端到端版本：`openclaw@2026.7.1-2`。
- 配置文件：默认实例的 `~/.openclaw/openclaw.json`。
- 已验收协议：Chat Completions、OpenAI Responses、Anthropic Messages。

检测只检查固定常用安装位置。存在 `OPENCLAW_HOME`、`OPENCLAW_STATE_DIR`、
`OPENCLAW_CONFIG_PATH`或自定义 Profile 时，HAL100 会提示并阻止自动写入，不猜测用户想管理
哪个实例。

## 所有权边界

HAL100只拥有三项资源：

1. `models.providers.hal100`。
2. `secrets.providers.hal100_gateway`。
3. HAL100应用数据目录中的`credentials/openclaw-gateway.key`。

Provider使用固定模型`hal100-active`，能力来自版本化模型契约。Secret使用OpenClaw文件型
`SecretRef`引用`0600`专属Key；配置和SQLite都不保存明文Key。HAL100不修改默认模型、默认
Provider、其他模型Provider、其他Secret、会话或运行中的服务。

## 官方配置工具边界

OpenClaw配置允许JSON5，HAL100不自行假设完整格式。计划阶段和应用阶段均调用固定候选路径中
的官方CLI执行`config patch`；命令运行在清空环境、固定安全PATH、20秒超时和16 KiB输出上限
内。写入前先通过官方`--dry-run --json`验证补丁。

配置或断开流程为：

```text
Detect → Parse → Ownership Check → Official Dry Run → Preview → Native Confirm
       → Digest Recheck → Backup → Official Patch → Semantic Verify
       → Credential Hot Register → SQLite Transaction
       └──────────────────────────── Rollback on failure
```

如果原文件是JSON5，官方工具可能把注释和排版标准化；预览会明确提示并先保存原字节备份。
配置、CLI或凭据在预览后变化时拒绝执行。断开只移除两项受管分片并吊销OpenClaw专属Key。

## 协议切换

界面允许用户显式选择三种已验收协议。切换只替换`models.providers.hal100`协议字段：

- Chat Completions：`openai-completions`，Gateway `/v1` Base URL。
- Responses：`openai-responses`，Gateway `/v1` Base URL。
- Anthropic Messages：`anthropic-messages`，Gateway根URL。

协议切换不会更改OpenClaw默认模型；每次仍需新的短期预览和原生确认。

## 验收

单元测试覆盖JSON5、三协议补丁、用户字段保留、自定义实例阻止、CLI失败、陈旧计划、凭据
权限、回滚和精确断开。忽略型真实验收下载固定官方包，在隔离HOME中连续切换三种协议，
经真实HAL100 Gateway请求模拟后端，并确认Usage全部归属`openclaw`且`hal100-agent`为零。
