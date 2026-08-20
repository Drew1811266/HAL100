# Hermes Agent 接入

本文描述用户独立安装的 Hermes Agent 如何连接 HAL100。当前实现以 Hermes Agent 0.18.2
（上游发布标签`v2026.7.7.2`）为兼容基线。

## 上游契约与前置条件

- 默认状态目录：`~/.hermes`。
- 默认Profile配置：`~/.hermes/config.yaml`。
- 凭据环境文件：`~/.hermes/.env`。
- Provider调用：`--provider custom:hal100 --model hal100-active`。
- HAL100已验收协议：Chat Completions。
- 当前最低上下文：64,000 Token。

Hermes 0.18.2会拒绝上下文不足64,000 Token的模型。HAL100因此根据真实模型契约故障关闭：
当前保守`hal100-active`契约只有4,096 Token时，界面显示“接入被阻止”，不生成配置计划。
切换到声明至少64,000 Token且真实可用的模型契约后，用户可重新检测并配置。这个门槛不是
任意代码量或功能限制，而是官方运行时兼容要求。

上游依据：

- [Hermes Agent v0.18.2 release](https://github.com/NousResearch/hermes-agent/releases/tag/v2026.7.7.2)
- [Provider配置](https://github.com/NousResearch/hermes-agent/blob/v2026.7.7.2/website/docs/integrations/providers.md)
- [Profile说明](https://github.com/NousResearch/hermes-agent/blob/v2026.7.7.2/website/docs/user-guide/profiles.md)

## 所有权边界

HAL100只拥有：

1. `config.yaml`中的`providers.hal100`。
2. `.env`中的`HAL100_HERMES_GATEWAY_KEY`单行变量。
3. SQLite与运行时凭据注册表中的`hermes-agent`专属Key摘要。

HAL100只配置`default` Profile，不改变Hermes的粘性活动Profile、默认模型、其他Provider、
其他环境变量、会话、规则、Skills或服务。检测到自定义`HERMES_HOME`或非default粘性Profile
时会提示；验证与推荐调用始终显式使用`-p default`。

## 配置与秘密处理

受管Provider等价于：

```yaml
providers:
  hal100:
    name: HAL100
    api: http://127.0.0.1:10100/v1
    key_env: HAL100_HERMES_GATEWAY_KEY
    transport: chat_completions
    default_model: hal100-active
    discover_models: false
    context_length: 65536
    models:
      hal100-active:
        context_length: 65536
        supports_vision: false
```

实际上下文和视觉能力来自当时的版本化模型契约。YAML使用语义解析和重写，因此既有注释、
锚点和排版可能标准化；预览会提示并在写入前备份不含密钥的原YAML。`.env`则只按精确变量
行补丁，其他字节逐字保留，并把权限收紧到`0600`。

`.env`可能包含用户的其他秘密，所以HAL100不会把整份文件复制成持久备份。事务期间原字节
只保存在内存中用于失败回滚；成功断开只删除受管变量。明文Key不进入YAML、SQLite、日志、
错误或审计详情。

## 验证、回滚与断开

计划阶段先在HAL100临时`HERMES_HOME`中写入候选配置，调用官方
`hermes -p default config show`验证。应用阶段复验版本、模型契约revision、配置和`.env`
摘要，完成原子写入后再用真实default Profile验证；任一步失败恢复YAML、`.env`、数据库和
内存凭据状态。计划5分钟有效且只能消费一次。

断开只移除`providers.hal100`、专属环境变量和`hermes-agent`凭据；其他三个外部Agent和
内置`hal100-agent`不受影响。

## 验收

单元测试覆盖64K门槛、YAML和`.env`保留、冲突、外部修改、版本、陈旧计划、回滚和断开。
忽略型真实验收用`uv`在隔离Python 3.12环境安装`hermes-agent==0.18.2`，使用65,536 Token
测试模型契约，经真实HAL100 Gateway完成官方CLI单次调用；Usage归属`hermes-agent`，
`hal100-agent`保持为零。
