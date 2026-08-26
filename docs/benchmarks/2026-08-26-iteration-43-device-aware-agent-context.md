# 2026-08-26 迭代43：设备感知 Agent 长上下文验收

## 结论

Apple M1/16 GiB的32K标准档通过真实容量、取消、20轮连续任务、资源回收和复合配置纵向验收。
产品不再把16K散落硬编码为唯一容量；Rust只在16K基线与32K标准档之间选择，Pi和用户输入
不能扩大档位。64K尚未取得对应最低设备证据，保持关闭。

## 真实长输入

隔离启动Qwen3.5-2B Q4_K_M与llama.cpp b10218，固定`--ctx-size 32768 --parallel 1
--reasoning off`。输入由1200段编号配置上下文组成，不保存请求或回答正文，只记录数值与判定。

| 指标 | 结果 |
| --- | ---: |
| Provider提示Token | 27,725 |
| 输出Token | 5 |
| 截断 | 0 |
| 计数答案 | 正确（1200） |
| 总HTTP耗时 | 73.119秒 |
| 提示处理耗时 | 72.944秒 |
| 提示吞吐 | 380.09 Token/s |
| 物理占用峰值 | 566.3 MiB |
| 停止后残留进程/端口 | 0 / 0 |

## 真实取消与配置图

两个被忽略的显式验收均只复用已校验模型/引擎资产，数据库、Gateway、凭据、HOME、配置和检查点
位于临时目录：

| 场景 | 结果 |
| --- | ---: |
| 32K冷启动取消 | 0毫秒 |
| 32K活跃推理取消 | 75毫秒 |
| 取消后Kernel/模型状态 | `stopped` / `stopped` |
| 32K复合图模型回合 | 0 / 0 / 2 |
| 复合图重复工具结果Token | 0 |
| 确认后证据/终态 | `integrationRecheck` / `succeeded` |

## 连续任务稳定性

同一隔离32K运行时连续执行20次真实Pi只读运行目录任务。每轮都重新启动按任务Sidecar，只允许
一个`inspect_runtime_catalog`工具；模型运行时在300秒测试空闲窗内保持热态。任务结束后Kernel
立即退出，最终显式停机再检查模型进程与活动任务。

| 指标 | 结果 |
| --- | ---: |
| 连续任务 | 20 / 20 |
| 最大执行模型回合 | 2 |
| 重复工具结果Token | 0 |
| 最大Provider输入Token | 517 |
| 总耗时 | 213.655秒 |
| 最慢单轮 | 17.373秒 |
| 每轮结束活动任务/Kernel | 0 / 0 |
| 显式停机后活动任务/子运行时 | 0 / 0 |

该结果只是真实Apple M1/16 GiB证据。8、24、64和128 GiB条目是对Rust选择函数的版本化边界
测试，不是这些硬件的性能实测；它们分别证明低档回退、16 GiB阈值和高内存设备仍被封顶32K。

命令：

```text
cargo test -p hal100-desktop real_agent_32k_profile_cancels_and_recovers_in_isolation -- --ignored --nocapture
cargo test -p hal100-desktop real_agent_completes_isolated_confirmed_configuration_graph -- --ignored --nocapture
cargo test -p hal100-desktop real_agent_32k_repeated_tasks_are_stable_and_reclaim_resources -- --ignored --nocapture
```

## 合同与边界

- `contracts/agent-runtime/v2-device-capacity.json`固定Rust权威、16 GiB阈值、16K/32K两档和传输上限；
- `contracts/agent-evals/v12-device-context-stability.json`固定7个设备选择边界、20轮连续任务和停机
  后零残留门槛；Rust单元测试直接读取全部选择用例；
- RPC v12在意图与执行请求中携带Rust档案，Sidecar拒绝合同外容量；
- 内置Agent llama.cpp、托管用户模型与`managed-route-v3`外部模型描述读取同一档案；
- Agent状态显示档位、上下文、保留区前可用输入和输出上限；
- 本次不保存提示词、回答、工具原文、路径或凭据，不增加工具/执行权限，也不涉及正式分发流程。
