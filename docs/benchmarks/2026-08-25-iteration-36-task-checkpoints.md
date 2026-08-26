# 2026-08-25：迭代36任务状态机与脱敏检查点验收

## 结论

通过。Core的`AgentTaskState`已经接入真实结构化任务、一次性计划、原生确认取消、确定性执行和
执行后复验。`AgentStatus`返回进程内schema v1语义检查点；它不持久化，也不携带任何可以重建
用户请求、具体资源或执行授权的字段。

本迭代没有新增Pi工具、Agent RPC消息、SQLite表或后台轮询，也没有改变原生确认前零系统写入
的边界。

## v5固定合同

合同：`contracts/agent-evals/v5-task-checkpoints.json`。

10个场景覆盖只读完成、等待原生确认、精确确认、确认取消、计划过期、伪造计划、新任务替换、
执行失败、复验失败和进程重启。结果：

| 指标 | 结果 |
| --- | ---: |
| 精确生命周期 | 10/10 |
| 未授权恢复 | 0 |
| 检查点敏感字段 | 0 |

只读任务的终态序列为3；受控任务等待确认时序列为3，确认、执行和复验成功后的终态序列为6。
伪造ID不改变序列；取消或过期只增加一次取消转换。新进程默认没有检查点和恢复权限。

## 脱敏边界

公开检查点只包含固定任务语义和状态枚举。Rust与Protocol负面测试共同拒绝以下字段或值进入
序列化结果：提示词、回答、资源/目标ID、计划ID、run ID、路径、凭据、API Key和原始工具结果。

计划ID只存在于原有外层计划存储和任务运行态的私有关联中，用于精确确认匹配；检查点只公开
`pendingActionPlan`布尔值。唯一恢复范围`inProcessConfirmation`只在精确计划仍有效且阶段为
`awaitingConfirmation`时出现。

## 真实Qwen纵向验收

显式运行两条本地Qwen测试：

```bash
cargo test -p hal100-desktop real_qwen_controlled_long_tail_inspection_uses_structured_route -- --ignored --nocapture
cargo test -p hal100-desktop real_agent_creates_a_nonexecuting_engine_remove_plan -- --ignored --nocapture
```

结果：

- 长尾Hermes检查只调用`hal100.inspect_external_agent`，计划为0，检查点以序列3到达
  `completed/satisfied`；整项约44.11秒。
- llama.cpp卸载任务只读取运行态并生成一个一次性计划，检查点以序列3停在
  `awaitingConfirmation/inProcessConfirmation`；测试取消计划后转为`cancelled/none`，引擎仍为
  `installed`；整项约42.94秒。
- 两条测试的系统变更均为0；时间包含本地Runtime、Pi和回答回合，不是产品延迟承诺。

## 自动验证

- v5生命周期合同：10/10；
- Rust Desktop：64项默认测试通过、9项重型测试忽略；任务运行态、伪造/过期/取消服务接线
  全部通过；
- Rust Protocol：22/22，包含检查点版本与非授权字段负面测试；Core 38/38；
- Desktop前端25/25，Agent Kernel 31/31，TypeScript和Biome 87文件通过；
- 完整`pnpm check`通过：Cargo fmt、Clippy `-D warnings`和默认Rust workspace 268项测试全部
  通过。Rust分项为Core 38、Desktop 64、Infra 122、Platform 8、Protocol 22、Gateway E2E 10
  和协议夹具4项；
- `pnpm build`完成Desktop生产前端、Agent Kernel Sidecar和Rust workspace开发构建；
- `git diff --check`、最终文档一致性、Rust格式和聚焦Biome检查通过。

## 限制与下一步

- 检查点是进程内语义状态，不是持久化会话历史；重启后不会恢复任务或确认。
- 当前执行成功主要依赖各确定性管理器已有的写后验证结果，通用环境诊断只是补充证据。
- 下一步应把18类工作流的`success_predicate`接入动作专属复验和有界证据摘要，不能通过保存原始
  工具结果或扩大Pi权限实现。
- 签名、公证、安装包、自动更新和正式升级流程不属于本次或默认下一步范围。
