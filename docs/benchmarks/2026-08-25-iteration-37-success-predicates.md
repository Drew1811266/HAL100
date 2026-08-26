# 2026-08-25：迭代37成功谓词与有界证据复验验收

## 结论

通过。18类Rust工作流的成功谓词已经接入类型化只读结果、幂等完成和确认执行后的动作专属现实
复验。模型回答、Sidecar完成消息、工具调用成功和执行器摘要都不能直接把任务标记为成功。

本迭代没有增加Pi工具、Agent RPC消息、SQLite表或后台轮询，也没有改变原生确认前零系统写入
的边界。

## v6固定合同

合同：`contracts/agent-evals/v6-success-predicates.json`。

| 指标 | 结果 |
| --- | ---: |
| 工作流成功谓词与允许证据来源 | 18/18 |
| 证据故障注入 | 6/6 |
| 模型声明成功 | 0 |
| 错误工作流证据推进 | 0 |
| 单任务非终态重规划上限 | 1 |
| Agent证据持久化敏感字段 | 0 |

6个故障场景覆盖模型只给文字、只读结果不满足、动作首次不满足、同任务继续一次、动作第二次仍
不满足和动作复验证据不可用。只读不满足与证据不可用进入`Blocked`；动作首次不满足进入
`Planning`并记录`1/1`，第二次不满足进入`Blocked`。

## 类型化证据与执行闭环

- 系统、运行目录、环境诊断、运维历史、短时健康、模型目录、仓库和外部Agent状态在Rust仍持有
  DTO时缩减为枚举化结论；运行态不保存原始工具结果。
- 模型启动要求精确目标同时是活动模型且引擎为`Running`；模型移除重新查询模型库；引擎安装与
  移除重新查询`installState`。
- 四类外部Agent配置与断开重新运行专用检测；Pi私有安装与移除重新验证HAL100受管入口。
- 修复复验使用原发现的代码、组件和私有目标关联；同一发现仍存在为不满足，发现可能被截断或
  新诊断不可用为证据不可用。
- 类型化前置状态已满足时允许幂等无计划完成；这个豁免由Rust证据触发，不能由Sidecar省略计划
  工具触发。

临时数据目录中的确认执行测试真实创建一个缺失托管模型索引、消费精确一次性计划并执行索引
移除。任务只有在重新查询模型库确认目标缺席后，才到达
`completed/satisfied/modelLibraryRecheck`。Agent专属动作审计序列化结果不含计划ID、run ID、
模型ID或路径；正常模型领域审计继续保留用户可见的操作结果。

## schema v2检查点

检查点在v1任务语义和阶段之上增加：

- `successPredicate`；
- `evidenceSource`和`evidenceObservationCount`；
- `replanAttemptCount`和`maxReplanAttempts`；
- `unsatisfied`与`evidenceUnavailable`复验状态。

检查点仍只存在Desktop进程内。Protocol和Desktop负面测试继续拒绝提示词、回答、具体资源、
计划/run ID、路径、凭据、API Key和原始工具结果；错误工作流来源也不能推进阶段。Agent页技术
详情以中文显示结论、来源、观察数和重规划计数，但前端不参与判断。

## 真实Qwen/Pi纵向验收

显式运行：

```bash
cargo test -p hal100-desktop real_qwen_controlled_long_tail_inspection_uses_structured_route -- --ignored --nocapture
cargo test -p hal100-desktop real_agent_creates_a_nonexecuting_engine_remove_plan -- --ignored --nocapture
```

结果：

- 长尾Hermes检查只调用`hal100.inspect_external_agent`，计划为0；schema v2检查点到达
  `completed/satisfied/externalIntegrationStatus`，整项50.41秒。
- llama.cpp卸载请求只读取运行态并生成一个一次性计划，检查点停在
  `awaitingConfirmation/actionPlan/inProcessConfirmation`；测试清理计划后引擎仍为`installed`，
  整项27.00秒。
- 真实模型负责意图和工具编排，成功结论由Rust类型化状态产生。为了不改变真实开发环境，确认
  执行后的复验使用上文临时模型索引纵向测试，而不卸载真实llama.cpp。

## 自动验证

- v5生命周期合同10/10，v6成功谓词18/18、故障注入6/6；
- Rust聚焦测试覆盖证据三态、来源白名单、幂等完成、一次重规划、动作专属复验、修复发现身份和
  真实临时索引执行；
- 完整`pnpm check`通过：Biome检查88个文件，Desktop前端25项、Agent Kernel 31项、Rust
  工作区277项默认测试全部通过，26项显式环境/性能测试保持忽略，Clippy严格零警告；
- 完整`pnpm build`通过：Desktop生产前端、Agent Kernel编译产物和Rust工作区均成功构建；
- `pnpm docs:check`确认任务检查点schema v2、18/18成功谓词、6/6证据故障和迭代边界一致，
  `git diff --check`通过。

## 限制与下一步

- 一次重规划只保留同一进程内的任务规范和计数，不自动重放提示词，也不持久化对话。
- 当前真实Qwen验收覆盖只读完成和计划暂停；每种真实第三方CLI的确认执行仍由既有适配器隔离
  HOME端到端测试承担，不能外推为所有现场配置都成功。
- 下一步应接入有界澄清与同任务继续，复用当前状态和证据边界，不扩大工具、不保存用户对话、
  不恢复旧授权。
- 签名、公证、安装包、自动更新和正式升级流程不属于本次或默认下一步范围。
