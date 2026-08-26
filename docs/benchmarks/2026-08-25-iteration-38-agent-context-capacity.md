# 2026-08-25：迭代38 Agent上下文容量验收

## 结论

通过。内置Qwen/Pi Agent Runtime从6144提升到16384 Token；Pi固定4096 Token保留区之后，可用
输入空间由约2048提升到12288 Token。输出仍为768 Token，配置任务温度为0。

## 固定合同

- `contracts/agent-runtime/v1-capacity.json` schema v1；
- Rust `llama-server --ctx-size`与Sidecar `contextWindow`均为16384；
- `parallel 1`、`reasoning off`、本地最大输出768；
- 模型元数据：`n_ctx_train=262144`、`n_ctx_orig_yarn=262144`，16K未使用上下文扩展。

## Apple M1 / 16 GiB结果

真实长尾Hermes检查：完成，49.44秒。完整9场景本地Agent验收：

```text
accuracy=9/9
cold_ms=22716
warm_ms=[8447,15820,13230]
catalog_ms=11120
plan_ms=46311
cold_cancel_ms=8
inference_cancel_ms=71
idle_exit_ms=2500
```

资源采样：

| 指标 | 16K结果 | 历史6K样本 |
| --- | ---: | ---: |
| llama-server峰值RSS | 2,066,368 KiB | 1,688,880 KiB |
| 物理占用 | 556.1 MiB | 386.4 MiB |
| 物理占用峰值 | 556.2 MiB | 386.4 MiB |

RSS包含GGUF映射页；物理占用更接近实际内存压力。当前增加约170 MiB物理占用，在最低测试设备
Apple M1/16 GiB上接受。32K与自适应窗口尚未验收，不能从本结果外推。

## 发现与修复

完整验收首次暴露零工具Gateway解释被Pi错误提议为`external_agent_target`澄清。Rust此前只验证
澄清枚举合法，没有验证目标是否已经出现在原提示中；拒绝该提案后，旧关键词路由又因OpenCode
名称附带检查工具而故障关闭。

修复后：

- Core按原提示验证三类澄清是否确有缺失槽位；
- 纯机制解释只能接受`unresolved`提案；
- 旧能力路由对同一解释语义也返回零工具；
- Sidecar明确区分机制解释和当前状态检查；
- 真实回答重新命中HAL100、Gateway、后端及路由/转发证据，且未调用工具。

回答正文、提示词、模型路径和临时密钥未写入基准、日志、审计或数据库。
