# 2026-08-25：迭代35结构化任务路由受控接管验收

## 结论

通过。经过Rust校验和双路裁决的`AgentTaskSpec`现在能在默认`controlled`模式下派生规范
`requiredTools`并进入真实运行链。Pi仍只提出任务语义；工具、前置关系、目标绑定、计划预算、
原生确认和确定性执行全部由Rust拥有。

这次接管不会直接执行系统变更。现有写能力仍只生成一个短期一次性计划，用户确认前没有写入。

## 接管策略

| 裁决结果 | 行为 |
| --- | --- |
| 可信确定性任务 | 从Rust任务工作流派生叶能力和全部前置能力 |
| 可信Pi任务 | 使用相同Rust工作流派生能力；不采纳模型工具 |
| 确定性澄清/拒绝 | Rust固定回答；不启动Kernel或模型 |
| Pi澄清/拒绝 | 意图回合后Rust固定回答；不进入工具运行 |
| 未解析且旧能力非空 | 故障关闭；不调用工具或生成计划 |
| 未解析且旧能力为空 | 保留零工具产品说明兼容路径 |
| `safe-legacy`模式 | 仅确定性任务可回到旧能力；Pi独有任务故障关闭 |

结构化能力不会和旧工具集合求并集，避免扩大数据范围、重复工具或产生多个写计划。

## v4固定合同

合同：`contracts/agent-evals/v4-controlled-routing.json`。

13个场景覆盖确定性只读、确定性计划、4类Pi长尾任务、2类安全拒绝、未解析故障关闭、零工具
兼容、确定性Shell守卫以及两类安全回退。结果：

| 指标 | 结果 |
| --- | ---: |
| 精确路由决策 | 13/13 |
| 精确`requiredTools`集合 | 13/13 |
| 未授权系统变更 | 0 |

Core还逐项验证18类任务的叶能力效果、数据范围、原生确认要求与工作流约束一致；能力注册表继续
负责前置闭包和规范顺序。

## 真实Qwen纵向验收

显式运行：

```bash
cargo test -p hal100-desktop real_qwen_controlled_long_tail_inspection_uses_structured_route -- --ignored --nocapture
```

输入使用v3中的长尾Hermes检查场景。真实本地Qwen先通过零工具意图回合提出
`inspect_external_agent/hermes-agent`，Rust复验后只下发`hal100.inspect_external_agent`。最终：

- 真实工具调用：1个，只读；
- 操作计划：0；
- 路由指标：`structuredPi`；
- 未授权系统变更：0；
- 整项测试耗时：约16.69秒，包含本地Runtime、意图和回答回合，不作为产品延迟承诺。

确定性Shell越权测试同时证明守卫回答不会启动Kernel或模型，工具和计划均为0。

## 脱敏观测与回退

`AgentTaskRoutingMetrics`只聚合结构化确定性接管、结构化Pi接管、守卫回答、安全旧路由、零工具
兼容和故障关闭计数。它不保存提示词、回答、任务类型、目标、run ID、路径或凭据，且不写
SQLite、审计正文或日志；应用重启后清空。

开发期需要快速回退时，可在启动环境显式设置：

```bash
HAL100_AGENT_TASK_ROUTING_MODE=safe-legacy pnpm desktop:open
```

状态会返回`safeLegacy`，防止回退模式不可见。该模式不会激活确定性入口未识别、仅由Pi提出的
任务，因此回退不等于恢复不安全的长尾关键词执行。

## 自动验证

- Rust Core：38/38；
- Rust Protocol：21/21，包含路由指标无敏感字段序列化检查；
- Rust Desktop：58项默认测试通过、9项重型测试忽略；
- Agent Kernel：31/31；
- 云端Gateway→Sidecar闭环：通过，零工具解释只进入显式兼容路径。

完整`pnpm check`通过：文档一致性、Biome 86文件、TypeScript、Desktop前端25项、Agent Kernel
31项、Cargo fmt、Clippy `-D warnings`和默认Rust workspace 261项测试全部通过。Rust分项为
Core 38、Desktop 58、Infra 122、Platform 8、Protocol 21、Gateway E2E 10和协议夹具4项。
`pnpm build`同时完成Desktop生产前端、Agent Kernel Sidecar和Rust workspace开发构建。

## 限制与下一步

- 当前接管的是任务到能力集合，不是模型到执行器；系统写入仍与Agent运行分离。
- `AgentTaskState`和脱敏语义检查点尚未接入真实运行、确认与复验生命周期。
- v4是13个固定场景，真实纵向验收当前只有一个Pi长尾只读场景，不能外推为开放输入完全覆盖。
- 下一步应接入任务阶段和检查点，不能扩大Pi工具权限或持久化提示词。
- 签名、公证、安装包、自动更新和正式升级流程不属于本次或默认下一步范围。
