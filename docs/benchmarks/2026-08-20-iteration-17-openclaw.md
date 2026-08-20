# 迭代17：OpenClaw多协议接入验收

日期：2026-08-20

## 交付范围

- OpenClaw固定实例检测、版本下限与自定义实例故障关闭。
- JSON5读取和官方`config patch` dry-run/应用边界。
- 独立文件型SecretRef、受管模型Provider、配置刷新和精确断开。
- Chat Completions、Responses、Anthropic Messages三协议显式切换。
- 桌面端检测、语义预览、原生确认、应用、状态刷新和断开。

## 自动验收结果

| 验收项 | 结果 |
| --- | --- |
| OpenClaw适配器单元测试 | 6/6通过 |
| 固定官方OpenClaw CLI `2026.7.1-2`隔离端到端 | 三协议全部通过 |
| 配置与凭据回滚 | 通过 |
| Usage身份隔离 | `openclaw`至少3条，`hal100-agent`为0 |
| React接入页回归 | 通过 |
| Rust workspace检查 | 通过 |

官方测试在随机临时HOME中安装精确npm包，使用官方配置工具应用HAL100补丁，再连续以三种
协议调用真实HAL100 Gateway。模拟后端分别收到Chat、Responses与Anthropic请求，模型均为
`hal100-active`。测试不修改开发机真实OpenClaw配置，不启动或管理OpenClaw常驻服务。

## 故障与共存覆盖

- 低版本、自定义HOME/STATE/CONFIG/Profile、未知Provider所有权和不安全凭据均故障关闭。
- 预览后配置或凭据变化拒绝执行。
- 官方工具dry-run、应用或写后语义验证失败会恢复配置、凭据、数据库和运行时注册表。
- 协议切换不修改默认模型，断开只删除OpenClaw受管资源。
- OpenCode、Pi、Hermes与内置Runtime拥有不同路径、凭据ID和Usage身份。
