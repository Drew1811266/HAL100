# 2026-08-25 迭代41：Agent效率与长上下文装配

## 范围

- 内置运行时：Qwen3.5-2B Q4_K_M，Pi Agent Core/Pi AI 0.84.2
- 执行合同：RPC v11、v9受控动作18条路径、三工具隔离对照
- 外部配置：`managed-route-v2`，16384上下文/1024最大输出
- 数据边界：只记录数值指标和场景集合，不保存提示词、回答或原始工具结果

## 自动对照

| 指标 | 旧装配 | 当前装配 | 结果 |
| --- | ---: | ---: | ---: |
| v9动作路径 | 18 | 18 | 能力与验证不变 |
| 动作路径最小模型回合总数 | 55 | 37 | -32.7% |
| 三工具链重复工具结果Token | 非零 | 0 | -100% |
| 三工具链发送工具结果Token | 旧值 | ≤旧值60% | 至少-40% |
| 最终计划后的额外解释回合 | 18 | 0 | 固定安全说明替代 |
| 未授权工具/写操作 | 0 | 0 | 不变 |

回合口径为实际Provider stream调用。旧值按每个必需工具一次调用加一次最终解释计算；当前动作
计划在最后工具成功后确定性停止。该口径不把原生确认、Rust执行和现实状态复验算作模型回合，
也没有删除这些步骤。

工具结果Token按Pi上下文估算器相同的`ceil(可见字符数/4)`计算。隔离对照使用相同三段结构化
结果和相同工具链，唯一变量是旧完整历史与当前直接依赖装配；因此比例可比较，但不是计费Token。

## 真实Pi验收

执行固定本地模型启动计划，Rust路由直接明确任务，不增加意图回合：

| 指标 | 结果 |
| --- | ---: |
| 上下文窗口 | 16384 Token |
| 意图模型回合 | 0 |
| 执行模型回合 | 2 |
| 纠偏提示 | 0 |
| Provider报告输入 | 1375 Token |
| Provider报告输出 | 391 Token |
| 峰值Provider输入 | 793 Token |
| 峰值装配估算 | 312 Token |
| 发送工具结果估算 | 90 Token |
| 重复工具结果估算 | 0 Token |
| 工具轨迹 | 运行目录→一次性启动计划 |
| 系统变更 | 0；计划已丢弃 |

命令：

```text
cargo test -p hal100-desktop real_agent_creates_a_nonexecuting_model_plan -- --ignored --nocapture
```

## 长配置合同

- `LlamaCppManager`以`--ctx-size 16384 --parallel 1 --reasoning off`启动HAL100托管用户模型；
- Pi Coding Agent/OpenCode/OpenClaw模型片段声明16384上下文；配置预览显示revision
  `managed-route-v2`；
- 共享JSON合同、Rust常量、适配器片段和假llama-server启动参数测试一致；
- Hermes 0.18.2的64000 Token最低门槛保持不变，16K配置不会被冒充为兼容。

## 结论

本轮达到平均模型回合至少下降30%和重复工具结果Token至少下降40%的结构性门槛，同时保留16K
长窗口、Rust工具权威、一次性计划、原生确认和动作专属复验。后续多设备与长时验收仍需扩大，
不能把本机单次真实Pi结果解释为所有Apple Silicon设备的性能承诺。

## 完整门禁

- `pnpm check`：通过；含Biome、TypeScript、Desktop 25项、Agent Kernel 34项、Rust fmt、
  Clippy与默认workspace测试；
- 默认Rust workspace：295项通过；联网目录、真实模型、官方第三方CLI、规模、性能和开发沙箱
  探针按设计保持显式忽略；
- `pnpm build`：通过；Desktop生产前端、Agent Kernel Sidecar与Rust workspace均构建成功；
- `git diff --check`：通过，无空白错误。
