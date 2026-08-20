# 迭代15–16：外部Agent控制面与Pi接入验收

日期：2026-08-20

## 交付范围

- 外部Agent支持协议与已验收协议分层。
- 版本化`hal100-active`模型契约。
- SQLite schema v8多资源所有权与秘密资源约束。
- 每适配器独立的一次性配置/断开计划、受限命令运行器和通用安全文件事务。
- OpenCode受管分片精确断开、凭据吊销与回滚。
- Pi Coding Agent检测、严格JSON配置、独立凭据、配置刷新、断开和桌面控制。
- 官方Pi CLI隔离端到端与内置Runtime共存归属验证。

## 自动验收结果

| 验收项 | 结果 |
| --- | --- |
| Pi适配器单元测试 | 8/8通过 |
| 通用受管文件测试 | 2/2通过 |
| 官方Pi CLI 0.84.2隔离端到端 | 通过 |
| Rust workspace检查 | 通过 |
| hal100-desktop Rust测试 | 37通过，7项本机大模型测试按设计忽略 |
| React测试 | 16/16通过 |
| TypeScript类型检查 | 通过 |

官方CLI测试在随机临时HOME中运行`@earendil-works/pi-coding-agent@0.84.2`，读取受管
`models.json`与0600凭据，经真实Gateway访问SSE模拟后端。请求模型为`hal100-active`，
Usage至少一条归属`pi-coding-agent`，`hal100-agent`归属为零；没有读取或修改真实用户Pi配置。

## 故障与回滚覆盖

- 严格JSON拒绝JSONC注释和尾随逗号。
- 非HAL100拥有的`providers.hal100`拒绝覆盖。
- 预览后配置或凭据变化拒绝执行。
- 受管分片变化标记为外部修改，不自动修复。
- 模型契约revision变化标记为需要刷新。
- 失败路径恢复原配置并删除新建凭据；秘密凭据没有明文备份。
- 断开保留其他Provider和用户默认字段，只吊销Pi专属Key。
