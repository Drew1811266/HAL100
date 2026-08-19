# 迭代 7：本地 HAL100 Agent 纵向闭环验收

## 1. 验收范围

本记录验证首条可产品化本地 Agent 链路：中文任务进入 Rust `AgentService`，按需启动独立 Qwen3.5-2B 运行时，经短期凭据访问 HAL100 Gateway，由 Pi Agent Core 编排工具调用，并由 Rust Tool Broker 重新授权和执行真实只读硬件探测。测试不覆盖尚未开放给 Agent 的安装、卸载、删除和配置写入工具。

这是首条闭环当时的快照。随后完成的运行目录、模型启动/切换计划和任务取消能力见[迭代7 Agent状态、计划与取消验收](2026-08-18-iteration-7-agent-actions.md)；后续能力没有改变本记录的原始测量条件。

## 2. 环境与固定制品

| 项目 | 值 |
| --- | --- |
| 日期 | 2026-08-18 |
| 机器 | Apple M1，16 GiB 统一内存 |
| 系统 | macOS 26.5（25F71），arm64 |
| Agent Kernel | `@earendil-works/pi-agent-core` 0.84.2、`@earendil-works/pi-ai` 0.84.2 |
| Node.js | 24.18.0，HAL100 Workspace 固定运行时 |
| Agent 模型 | `unsloth/Qwen3.5-2B-GGUF` / `Qwen3.5-2B-Q4_K_M.gguf` |
| 模型修订 | `f6d5376be1edb4d416d56da11e5397a961aca8ae` |
| 模型大小 | 1,280,835,840 字节 |
| 推理运行时 | HAL100 托管 llama.cpp `b10218`，独立随机回环端口 |
| 上下文与输出 | 6144 context，最多 768 output tokens，parallel 1，reasoning off |

Pi 0.84.2 会为上下文保留固定 4096 tokens。最初把模型和 Pi 上下文都设为 4096 时，实际请求被压缩到 `max_tokens: 1`，导致空答案。运行时与 Pi 模型契约统一提高到 6144 后，在保持 768 tokens 输出上限的同时恢复了工具调用和中文解释。该配置由 HTTP 契约测试固定。

模型启动前仍执行完整 SHA-256。为避免 1.28 GB 文件在 Rust 开发/测试构建中使用未优化哈希热循环，Workspace 仅对 `sha2` 依赖设置 package-level `opt-level = 3`；HAL100 自身仍保留可调试开发构建。实测模型校验由约 60 秒降至约 5 秒。

## 3. 场景结果

显式本机验收测试：

```bash
CARGO_TARGET_DIR=/tmp/hal100-iteration7-target \
  cargo test -p hal100-desktop \
  real_agent_completes_a_rust_hardware_probe -- --ignored --nocapture
```

结果为 `HAL100_AGENT_ACCEPTANCE accuracy=5/5 cold_ms=18157 warm_ms=[7800, 17744, 7466] idle_exit_ms=2500`。

| 场景 | 预期 | 结果 |
| --- | --- | --- |
| 首次硬件与模型建议 | 精确调用一次 Rust 系统摘要工具并给出硬件证据 | 通过 |
| 热态硬件规模建议 | 精确调用一次 Rust 工具 | 通过 |
| 热态 CPU、内存与 GGUF 建议 | 精确调用一次 Rust 工具 | 通过 |
| 解释 HAL100 Gateway 路由 | 不调用硬件工具，给出领域内解释 | 通过 |
| 通用写诗请求 | Rust 领域门禁在模型调用前拒绝 | 通过 |

这里的 `5/5` 是固定结构化验收场景完成率，不代表开放式任务或模型能力的普遍准确率。硬件任务除非完成 `hal100.inspect_system_summary`，否则 Rust 会拒绝最终结果；工具名称、精确参数、运行 ID、工具调用 ID、重复调用和调用总数均由 Rust 复验。

## 4. 延迟与内存

| 指标 | 实测 |
| --- | --- |
| 冷请求 | 18.157 秒，包含文件校验、模型启动、工具回合和答案回合 |
| 热请求 | 7.800、17.744、7.466 秒 |
| Agent 模型 RSS | 约 1,688,880 KiB；包含 mmap 的 GGUF 文件页 |
| Agent 模型私有物理占用 | 386.4 MiB，采样时峰值 386.4 MiB |
| Node Sidecar RSS | 约 112,752 KiB |
| Node Sidecar 私有物理占用 | 52.3 MiB，峰值 78.2 MiB |

RSS 会把映射的模型文件页计入常驻集，不等同于全部私有内存压力，因此同时记录 macOS `vmmap -summary` 的 physical footprint。热请求存在约 7.5—17.7 秒波动，主要由 2B 模型生成长度决定；当前满足内部开发可用性，但后续需要继续缩短系统提示并加入回答长度评估。

## 5. 空闲与资源回收

- 生产默认空闲退出为 120 秒；验收通过同一无轮询 generation timer 注入 2 秒测试值，并在 2.5 秒检查模型状态为 `stopped`。
- Sidecar 是单任务进程，完成 RPC shutdown 后立即退出，不等待两分钟。
- 验收后不存在 `llama-server --alias hal100-agent` 或 Agent Kernel Node 进程。
- `agent/sessions` 没有遗留会话目录，`.agent-session-*.key` 临时文件不存在。
- Agent 空闲后，长期桌面开发进程连续 5 次 CPU 采样均为 0.0%；私有物理占用 41.6 MiB，峰值 42.7 MiB，低于 80 MiB 预算。

## 6. 安全与数据边界

- Sidecar 只获得单次运行用的 Gateway Key；Key 不进入 SQLite、日志、审计和前端 DTO，RAII guard 在运行结束后从认证注册表移除。
- 私有 RPC 使用版本、长度前缀、1 MiB 帧上限、消息类型、请求 ID、运行 ID、工具调用 ID、响应超时与关闭确认。
- Sidecar 不注册 Shell、文件、进程、网络浏览、Coding Tools、扩展、Skills 或上下文自动发现；唯一工具是只读系统摘要代理。
- 系统信息只由 macOS Rust probe 获取。模型不能直接读取系统，也不能让 Sidecar执行工具。
- 用户模型路由不能创建、删除或恢复内部保留别名 `hal100-agent`，Agent 运行不改变用户当前活动后端。
- 审计只保存固定事件、模型名称、工具调用数、工具策略和稳定错误码；不保存提示词、回答、密钥或本地路径。
- 安装、卸载、删除、下载、配置写入和强制切换仍只能进入现有 Rust 原生确认命令；首条 Agent 工具没有这些执行能力。

## 7. 结论与残余风险

迭代 7 首条本地 Agent 纵向闭环达标：真实模型、真实 Gateway、真实 Pi 工具循环和真实 Rust 只读硬件探测共同完成，且空闲后回到零 Agent 子进程。当前 Sidecar 仍是当前用户权限下的固定供应链代码，进程边界不是操作系统沙箱；在签名与 helper/XPC 隔离方案完成前，必须继续保留该残余风险表述。安装、卸载、删除和模型切换的 Agent 工具尚未开放，属于后续迭代，不影响本次只读纵向闭环结论。
