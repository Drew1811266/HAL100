# 迭代12：应用边界与 Agent能力架构验收

- 日期：2026-08-20
- 平台：Apple Silicon macOS开发机
- 形态：未签名连续开发版，不制作安装包或发行物
- 兼容基线：Agent RPC v3、10个工具、Tauri IPC、SQLite schema v7、既有Gateway数据面

## 验收目标

- 证明本次改造按变化原因拆分职责，而不是用文件行数限制未来功能增长。
- 证明Agent能力策略可以脱离Tauri、SQLite、Node和真实模型独立测试。
- 证明任务协调、Kernel/RPC、工具执行、一次性操作计划和Provider会话不再由单一服务隐式共享状态。
- 证明前端可以按业务切片渐进迁移，同时保留同一桌面API和浏览器只读语义。
- 证明Gateway热路径、数据库schema和跨进程协议没有被架构改造改变。

## 结构结果

| 边界 | 最终职责 |
| --- | --- |
| `hal100-core::agent_capability` | 10项能力身份、风险、数据范围、前置关系和原生确认元数据的事实状态源 |
| `agent_coordinator` | 职责门禁、能力需求、RPC v3适配、完成校验、活动任务与取消租约 |
| `agent_kernel` | 固定Node发现、Sidecar进程、0700会话目录、RPC帧、超时、取消与回收 |
| `agent_tools` | Rust工具二次授权、前置顺序、只读快照、Manager计划编排和计划审计 |
| `agent_action` | 单一待确认计划、精确ID、有效期、禁止替换、一次性取用与丢弃 |
| `agent_provider` | 本地/云端目标解析、内存会话、可用性、审计和无本地回退 |
| `AgentService` | 稳定兼容外观、运行互斥、临时凭据/路由、确定性执行、审计与错误映射 |
| `features/agent` | Agent页面业务切片与独立环境诊断展示组件 |

具体引擎、模型移除和OpenCode Manager继续拥有现实状态复验与最终执行，没有为每个Manager方法增加机械接口。React根组件只引用Agent路由，不复制桌面API；其他成熟页面和全局样式没有进行大爆炸迁移。

## 自动验收结果

`pnpm check`完整通过：

| 范围 | 结果 |
| --- | --- |
| Biome | 37个文件通过 |
| TypeScript | Desktop与Agent Kernel均通过 |
| React | 15/15通过 |
| Agent Kernel | 23/23通过；新增RPC v3精确10工具目录回归 |
| Core | 11/11通过；包含能力唯一性、前置闭包、风险/确认和未知能力故障关闭 |
| Desktop | 32通过、7个真机大模型/规模探针按设计忽略 |
| Infra | 76通过、5个联网/规模探针按设计忽略 |
| Gateway E2E | 10通过、1个显式性能探针忽略 |
| Platform | 7/7通过 |
| Protocol与共享契约 | 11个单元测试和3个共享fixture测试通过 |
| Clippy与Rustfmt | 全Workspace、全部target、warnings-as-errors通过 |

`pnpm build`通过。生产前端构建为411.15 kB JavaScript（gzip 120.54 kB）和71.67 kB CSS（gzip 12.72 kB）；Agent Kernel与Rust全Workspace构建同时通过。本迭代没有创建安装包，也没有执行签名、公证或发售流程。

## 兼容性与安全结果

- Sidecar由`ToolBrokerBridge.createAgentTools`单点构造注册集合，测试锁定RPC v3的10个精确工具名；Rust完成校验仍要求注册数量为10。
- `AgentRunRequirements::to_rpc_v3`继续产生原有10个逐工具布尔字段，Provider协议字段和错误码保持不变。
- SQLite迁移测试继续确认schema版本为7，数据库没有新增表或迁移。
- Gateway 10项非忽略E2E覆盖Chat/Responses/Messages、SSE、取消、限流、路由、Usage和推理POST不重放；Agent边界没有进入数据面热路径。
- 一次性计划测试覆盖伪造、超长、过期、缺少确认标记、重复消费、静默替换和精确丢弃拒绝。
- Kernel测试覆盖取消感知、静默/无效协议有界失败、0700会话目录和析构清理。
- Provider与真实本机模拟Gateway测试覆盖内存会话重启失效、凭据隔离、临时身份归属、云端失效不回退本地。
- React回归确认浏览器预览中的Agent运行、环境修复和原生写操作保持禁用，不生成模拟成功结果。

## 120秒后台快速检查

主窗口通过开发单实例参数隐藏，对当前debug连续开发版观察121秒；原始报告位于被Git忽略的`output/stability/20260820T040025Z/`。

| 指标 | 结果 |
| --- | --- |
| 样本 | 12个 |
| CPU | 平均0.0000%，最大0.0000% |
| 物理内存 | 42.9→40.9 MiB，增长-2.0 MiB，最大42.9 MiB |
| 文件项 / TCP / 线程增长 | 0 / 0 / -4 |
| Gateway健康失败 | 0 |
| Agent子进程最大值 | 0 |
| Agent会话目录最大值 | 0 |
| 不安全审计行 / 疑似密钥日志 | 0 / 0 |
| Usage / 审计行变化 | 121→121 / 182→182 |

快速检查结论为`passed=true`。验收完成后已停止为本次检查启动的开发进程，没有遗留HAL100或Vite进程。

## 结论

迭代12完成。软件仍是模块化单体，代码量和文件数量可以随功能增长；后续是否继续拆分，以变化原因、依赖方向、事实状态所有者和独立可测试性判断，不以行数作为硬门槛。新增模型搜索/下载或其他Agent能力时，应先进入能力目录与现有应用边界，再单独决定是否升级RPC或提取新的crate。
