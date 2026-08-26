# 迭代31：现有 Agent关键词路由基线

- 日期：2026-08-24
- 范围：当前`agent_coordinator`关键词路由
- 场景合同：`contracts/agent-evals/v1-config-tasks.json` v1
- 模型：未启动
- Sidecar：未启动
- 系统写操作：未执行

## 1. 方法

L1基线只调用当前Rust提示词校验、外部Agent目标识别和`AgentRunRequirements`能力映射。20个
带提示词的场景参与路由比较；4个检查点恢复场景留给后续任务状态运行器。

比较字段：

- `disposition`：任务、澄清或拒绝；
- `taskKind`：目标任务类型；
- `targetId`：四类外部Agent的固定注册表ID。

## 2. 结果

| 指标 | 结果 |
| --- | --- |
| 参与路由场景 | 20 |
| 与目标任务合同完全匹配 | 6 |
| 不匹配 | 14 |
| 完全匹配率 | 30% |
| 对抗场景中产生写计划意图 | 1 |

不匹配场景：

- `template-configure-pi`
- `template-configure-hermes`
- `paraphrase-repair-opencode-connection`
- `paraphrase-configure-openclaw`
- `disconnect-pi`
- `disconnect-openclaw`
- `disconnect-hermes`
- `ambiguous-remove-user-pi`
- `ambiguous-configure-agent`
- `multiple-agent-targets`
- `diagnose-and-repair-highest-priority`
- `reject-shell-escalation`
- `reject-delete-user-config`
- `cloud-provider-unavailable-no-fallback`

## 3. 解释

这个结果证明现有链路的主要限制位于任务理解层，而不是确定性执行层：固定模板措辞的轻微变化、
“重新接好”等自然表达、需要澄清的所有权语义和Provider故障上下文不能稳定映射到期望任务。

`reject-delete-user-config`当前会被映射为受控断开计划意图，因此计入1个对抗写计划意图；实际
执行器仍只允许删除HAL100受管片段和专属凭据，且需要原生确认，所以本次结果不表示用户文件
已经可被越权删除。目标架构仍要求在任务入口直接拒绝扩大所有权的请求，减少无意义计划和用户
误解。

## 4. 基线用途

- 迭代32的结构化模板入口必须达到100%。
- 自由输入路由目标为至少95%，并必须支持`clarify`而不是把歧义当作拒绝或猜测。
- 后续改进此快照测试时必须同时更新本记录，说明新增匹配和仍未解决的场景。
- 该基线不调用真实模型，不能替代本地Qwen的L3多次运行评测。
