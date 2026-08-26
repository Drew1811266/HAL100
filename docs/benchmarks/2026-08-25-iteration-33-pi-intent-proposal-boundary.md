# 2026-08-25：迭代33按需Pi意图提案边界验收

## 结论

通过。HAL100已经在RPC v10中接入真实Pi Agent Core结构化意图调用，并由Rust执行二次校验和双路裁决。该能力只在确定性路由为`Unresolved`时调用，仍处于影子模式，不改变工具选择、计划、确认、执行或用户回答。

本记录验证的是协议、调用策略、数据边界和裁决确定性，不代表真实Qwen对开放自然语言的最终准确率。

## 实现边界

- `agent.intent.start`不携带`requiredTools`，Pi Agent Core以空工具数组运行一次。
- Sidecar只接受意图schema v1精确JSON，最大2 KiB；无效输出缩减为`invalid_intent_output`，Provider异常缩减为固定错误码。
- 原始模型输出、Markdown、自由文本理由和Provider错误正文不进入RPC。
- Rust再次验证schema、任务、目标、Provider和外部Agent注册ID，再输出六态裁决。
- 明确`Task`、`Clarify`或`Reject`不请求Pi；只有`Unresolved`最多增加一个意图模型回合。
- 裁决日志只包含固定结果键、注册任务键和有界目标ID，不记录提示词或回答。

## 自动评测结果

| 检查 | 结果 |
| --- | --- |
| 配置任务v1文本场景 | 20/20；两个对抗场景均未形成任务 |
| Pi调用与裁决v2合同 | 8/8 |
| Rust Core意图测试 | 10/10 |
| Sidecar意图测试 | 4/4；共享schema枚举一致、单次Pi调用、零工具、无效输出与固定失败码 |
| Agent Kernel全套 | 31/31 |
| Agent RPC Rust单元/fixture | 19/19、4/4 |
| 云端Gateway→Sidecar闭环 | 通过；一次意图请求加一次旧执行请求，均使用同一用户选择的云端Provider |

云端闭环记录两条精确Usage，总Token为28；两次上游请求都使用临时HAL100模型路由解析到`cloud-test`和固定测试后端，不包含会话凭据，不启动本地Agent Runtime，也没有静默回退。

## 质量门禁

- `pnpm check`通过：文档一致性、Biome 84文件、TypeScript、Desktop 25项、Agent Kernel 31项、Cargo fmt、Clippy `-D warnings`和默认Rust workspace 248项测试全部通过。
- `pnpm build`通过：Desktop生产前端、Agent Kernel Sidecar与Rust workspace开发构建完成。
- 重型真实Qwen、联网目录、官方第三方CLI、规模和开发沙箱探针保持显式运行，不计入本次日常快速绿灯。

门禁过程中发现并修复两项迁移残留：Core共享工具清单测试仍断言RPC v9；云端闭环夹具只准备一次上游响应，未覆盖新增意图回合。修复后均从头通过完整门禁。

## 未完成项

- 尚未对真实Qwen执行多次自然语言长尾评测，因此不报告模型提案准确率、延迟分布或方差。
- 提案尚未接管旧`requiredTools`计算和执行路由。
- 尚未建立只含枚举和耗时的产品内影子统计。

这些事项进入迭代34“Pi提案质量与影子观测”。签名、公证、安装包、自动更新和正式升级流程不属于该迭代。
