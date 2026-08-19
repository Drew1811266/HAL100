# 迭代 7：Agent 状态读取、受控计划与取消验收

## 1. 验收范围

本记录验证本地 HAL100 Agent 的第二条产品纵向闭环：Pi Agent Core 可以请求读取 HAL100 的脱敏模型、托管 llama.cpp 和 Gateway 状态，并可依据读取结果请求 Rust 生成一次性的本地模型启动/切换计划。生成计划不等于执行；只有用户在 Tauri 中通过 Rust 原生确认后，确定性 `LlamaCppManager` 才能重新校验并执行。安装、卸载、删除、下载、配置写入和强制切换仍没有 Agent 执行工具。

本轮同时验证活动任务取消：取消标记不等待 180 秒模型超时，Rust 会终止当前 Sidecar、释放临时凭据与会话目录，并立即停止独立 Agent Model Runtime。

## 2. 环境

| 项目 | 值 |
| --- | --- |
| 日期 | 2026-08-18 |
| 机器 | Apple M1，16 GiB 统一内存 |
| 系统 | macOS 26.5（25F71），arm64 |
| Agent Kernel | Pi Agent Core / Pi AI 0.84.2，固定 Node.js 24.18.0 |
| Agent 模型 | Qwen3.5-2B Q4_K_M，1,280,835,840 字节 |
| 推理运行时 | HAL100 托管 llama.cpp `b10218`，独立随机回环端口 |
| 计划有效期 | 5 分钟；只保留最新计划；精确 ID；一次消费 |
| 取消检查 | SHA-256每1 MiB、模型健康等待每50 ms、RPC接收每100 ms检查；Sidecar交换总超时仍为180秒 |

## 3. 工具与权限结果

产品 Sidecar只注册三个 HAL100工具：

1. `hal100.inspect_system_summary`：读取 Rust 生成的 Apple Silicon硬件摘要。
2. `hal100.inspect_runtime_catalog`：读取脱敏的引擎、活动路由和模型目录；不返回文件路径或凭据。
3. `hal100.plan_model_start`：只接受前一步结果中的精确 `modelId`，仅生成启动/切换计划。

Rust Tool Broker重新验证工具名、精确参数、run ID、tool call ID、调用顺序、唯一性和每任务最多4次调用。`plan_model_start`在同一任务未先完成目录读取时失败关闭。Sidecar每回合只向模型暴露Rust预判的唯一下一工具；若2B模型在成功返回工具结果后提前给出文字答案，Pi会话最多追加两次固定纠偏提示，仍未完成则Rust拒绝。认证、路由、连接等固定Provider错误不纠偏重试。计划绑定run、目标模型、生成时的当前模型、到期时间和原生确认要求；新任务会使旧计划失效，伪造、超长、过期、已消费或缺少原生确认标记的计划均被拒绝。

Rust原生确认之后仍会再次按计划ID取走最新计划，并通过现有的安全排空`LlamaCppManager::start_model`执行；Sidecar和聊天文本没有执行入口。用户取消确认时计划立即废弃并审计。

## 4. 真实 Qwen 验收

显式本机测试：

```bash
CARGO_TARGET_DIR=/tmp/hal100-iteration7b-target \
  cargo test -p hal100-desktop \
  real_agent_completes_a_rust_hardware_probe -- --ignored --nocapture
```

结果：

```text
HAL100_AGENT_ACCEPTANCE accuracy=9/9 cold_ms=24098 warm_ms=[12027, 11275, 22791] catalog_ms=13722 plan_ms=28479 cold_cancel_ms=6 inference_cancel_ms=98 idle_exit_ms=2500
```

| 场景 | 结果 |
| --- | --- |
| 冷态硬件检测 | 通过；精确完成 Rust硬件工具 |
| 两项热态硬件/GGUF建议 | 通过；均包含真实硬件证据 |
| Gateway领域说明 | 通过；不调用无关工具 |
| 运行环境目录读取 | 通过；只调用脱敏目录工具 |
| 模型启动/切换计划 | 通过；先读取目录，再用精确modelId生成一个计划 |
| 计划不执行 | 通过；计划生成前后用户托管模型状态完全一致 |
| 通用写诗请求 | 通过；在模型启动前由Rust领域门禁拒绝 |
| 冷启动校验中取消 | 通过；取消到任务回收完成6 ms |
| 推理中取消 | 通过；取消到Sidecar与模型回收完成98 ms |

这里的`9/9`是固定结构化验收场景完成率，不是对开放式模型能力的泛化声明。自动测试主动废弃生成的计划，不代替用户点击原生确认，也不会切换用户模型。最终完整样本中的计划链为28.479秒；单独聚焦探针观测到一次58.45秒的有界纠偏最坏样本，因此当前可靠性达标但交互延迟仍是后续优化项。

## 5. UI、审计与数据边界

- 中文Agent页展示状态读取、计划生成快捷任务、工具时间线、等待确认卡和活动任务取消。
- 确认按钮只调用Tauri命令；原生系统对话框由Rust创建。WebView不能提交`confirmed=true`一类等价授权。
- 审计记录计划生成、废弃、执行、失败和任务取消，只展示白名单标量：操作、modelId、原因、稳定错误码、工具次数和工具策略。
- 提示词、回答、模型路径、API Key和嵌套对象不会进入审计详情。
- 1280×720浏览器只读预览无横向溢出；页面明确说明计划不会自动执行，安装、卸载和删除未开放给Agent。

## 6. 回归与资源

- `pnpm check`通过：Biome、TypeScript、前端13项、Sidecar14项、Rust Clippy全目标及Workspace测试全部成功。
- 生产前端、Sidecar和Rust Workspace构建通过。
- 验收后没有`hal100-agent-runtime`、Agent Kernel Node或Agent用`llama-server`进程；没有遗留会话目录或临时会话Key。
- 长驻桌面开发进程5次CPU采样为`0.0、0.0、0.5、0.0、0.0%`；私有物理占用41.2 MiB、峰值43.1 MiB，仍低于80 MiB后台预算。

## 7. 结论

迭代7第二条受控Agent闭环达标：模型可以理解本地运行状态并生成有用计划，但不能把“计划”伪装成“执行”；原生确认、重新校验、确定性执行和审计仍全部位于Rust。任务取消能快速回收1.28 GB模型运行时相关进程，后台空闲时不新增轮询或常驻Agent资源。下一阶段可以在保持同一Policy/Executor边界的前提下开发用户主动启用的云端增强Agent。
