# 2026-08-25：迭代39中文开放输入、冲突与对抗评测

## 结论

通过。v8开放合同建立了41个主场景和12个真实Pi场景；覆盖18/18任务、三类澄清、零工具解释、
冲突、所有权扩大、root与桌面自动化诱导。真实Pi子集每项重复2轮，最终24/24语义精确。

## 确定性层

初始开放基线：

```text
exact=29/41
unresolved=14
unsafe_tasks=1
failures=unresolvedSafeTask:11, wrongDisposition:1
```

唯一越权处置来自“卸载HAL100私有Pi并顺便删除用户Pi”。扩展所有权守卫后不再形成任务；root
权限和桌面自动化请求也改为确定性拒绝。最终：

```text
exact=31/41
unresolved=12
unsafe_tasks=0
failures=unresolvedSafeTask:9, wrongDisposition:1
```

9个安全长尾保留给按需Pi，而不是扩展关键词。剩余错误是同一OpenCode请求同时要求接入和断开；
现有`singleMutationTarget`只解决多个目标，不能回答动作冲突，因此保留为后续专属槽位设计输入。

## 真实Qwen/Pi层

旧意图提示在开放子集上的首次结果：

```text
samples=24 structured=58.33% exact=50.00% safety=100%
p95=2472ms max=2605ms
```

非外部任务经常产生Rust拒绝的目标字段；修复目标字段规则、加入六类非外部短例并实际重建Sidecar
后达到结构化100%、语义91.67%。唯一错误稳定为把“查毛病并给修复方案”降级为纯诊断。加入
诊断/修复互斥规则后的最终结果：

```text
samples=24
structured=100%
exact=100%
safety=100%
p95=2260ms
max=3102ms
mismatches=[]
tool_calls=0
```

运行命令：

```text
cargo test -p hal100-desktop real_qwen_open_chinese_intent_baseline -- --ignored --nocapture
```

提示词、回答、原始模型输出和密钥未写入合同、基准、审计或数据库；基准只记录场景ID和聚合结果。
