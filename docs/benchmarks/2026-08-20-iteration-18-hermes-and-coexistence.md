# 迭代18：Hermes接入与四客户端共存验收

日期：2026-08-20

## 交付范围

- Hermes Agent 0.18.2固定兼容基线、默认Profile检测与官方CLI验证。
- `providers.hal100` YAML分片和`.env`专属变量的不同秘密处理策略。
- 64,000 Token真实上下文门槛与Blocked产品状态。
- 配置、刷新、原生确认、失败回滚、精确断开和桌面控制。
- OpenCode、Pi Coding Agent、OpenClaw、Hermes Agent四客户端同时标记为可用。

## 自动验收结果

| 验收项 | 结果 |
| --- | --- |
| Hermes适配器与配置单元测试 | 9/9通过 |
| 固定官方`hermes-agent==0.18.2`隔离端到端 | 通过 |
| 官方CLI测试模型契约 | 65,536上下文、4,096最大输出 |
| 当前保守4,096上下文契约 | 正确显示Blocked并拒绝计划 |
| Usage身份隔离 | `hermes-agent`至少1条，`hal100-agent`为0 |
| Agent注册表共存约束 | 四客户端身份、凭据和受管片段唯一 |
| React接入页回归 | 16/16通过 |
| TypeScript生产构建 | 通过 |
| Rust workspace全目标检查 | 通过 |
| 121秒后台快速稳定性 | 通过 |

官方测试使用`uv`创建隔离Python 3.12环境并安装精确PyPI版本。HAL100适配器先在临时
`HERMES_HOME`调用官方`config show`验证候选配置，再应用到隔离HOME；随后执行
`hermes -p default -z ... --provider custom:hal100 --model hal100-active --ignore-rules`，
经真实Gateway取得模拟回答和精确Usage。

最终连续开发版后台观察12次采样：平均/最高CPU均为0.0%，物理占用起止均为43.1 MiB、峰值
43.2 MiB，线程和TCP连接无增长，文件描述符减少1；Gateway 12/12健康，没有Agent子进程、
遗留会话目录、不安全审计字段或秘密匹配。原始报告位于被Git忽略的
`output/stability/20260820T104514Z/summary.json`。

## 共存矩阵

| 客户端 | 配置所有权 | 凭据身份 | 已验收协议 | 默认模型 |
| --- | --- | --- | --- | --- |
| OpenCode | `provider.hal100` | `opencode` | Chat | 保留 |
| Pi Coding Agent | `providers.hal100` | `pi-coding-agent` | Chat | 保留 |
| OpenClaw | 模型Provider + SecretRef | `openclaw` | Chat / Responses / Messages | 保留 |
| Hermes Agent | YAML Provider + `.env`变量 | `hermes-agent` | Chat | 保留 |

四个外部客户端和内置`hal100-agent`的Integration ID、Gateway客户端ID与凭据ID均唯一。
每个适配器拥有独立计划存储、文件路径、配置语法、验证器和回滚事务；任一客户端断开不会
撤销其他客户端Key。真实官方端到端分别证明外部Usage不会误归属内置Runtime。

## 安全结论

- YAML允许持久化非敏感原文件备份；可能包含其他秘密的`.env`不创建持久明文副本。
- `.env`只有HAL100变量行发生变化，其他内容逐字保留，最终权限为`0600`。
- 4K模型不会伪装成64K；兼容性不足在任何文件写入前阻止。
- 本迭代不涉及安装包、签名、公证或发售工作。
