# 2026-08-26 迭代42：真实Pi复合整图验收

## 目标与隔离

证明复合图能用真实Qwen3.5-2B/Pi生成写计划，经确认路径执行，并且只由Rust现实证据完成整图。
测试只复用已校验的GGUF和llama.cpp二进制；SQLite、Gateway、凭据注册表、HOME、OpenCode配置、
图检查点和审计均位于独立临时目录，不读取或修改用户现有OpenCode配置。

## 场景

Rust构造固定3节点图：确认引擎已安装→确认指定模型已运行→配置OpenCode接入。隔离Gateway先
启动已校验模型，使前两节点可由Rust现实状态预检幂等完成；第三节点必须经过真实Pi工具循环。

1. 引擎节点读取安装态，0模型回合、0计划、0写入；
2. 模型节点读取精确活动model ID，0模型回合、0计划、0写入；
3. 真实Pi在迭代43的32K/RPC v12复跑中仍用2回合读取OpenCode状态并生成唯一配置计划；
4. 图停在`awaitingConfirmation`，节点记录需要重新授权但不保存plan ID；
5. 伪造plan ID被拒绝且没有消费真实计划；
6. 精确计划经确认路径交给确定性OpenCode适配器；
7. Rust重新读取接入状态，`IntegrationRecheck`满足后节点与整图进入`succeeded`；
8. 终态脱敏恢复文件被清理，临时目录随后删除。

| 指标 | 结果 |
| --- | ---: |
| 引擎节点模型回合 | 0 |
| 模型节点模型回合 | 0 |
| 配置节点真实模型回合 | 2 |
| 重复工具结果Token | 0 |
| 写计划 | 1 |
| 伪造计划接受 | 0 |
| 动作后现实证据 | `integrationRecheck` |
| 图最终状态 | `succeeded` |
| 用户配置写入 | 0 |

显式命令：

`cargo test -p hal100-desktop real_agent_completes_isolated_confirmed_configuration_graph -- --ignored --nocapture`

输出：

`AGENT_GRAPH_CONFIRMED_LIVE context_window=32768 engine_turns=0 model_turns=0 configuration_turns=2 repeated_tool_tokens=0 final_state=succeeded`

该验收与v10/v11合同、隔离补偿纵向共同满足迭代42完成条件。完整`pnpm check`、`pnpm build`和
`git diff --check`在同一收口阶段执行。
