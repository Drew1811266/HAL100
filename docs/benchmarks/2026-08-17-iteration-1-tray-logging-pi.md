# 迭代 1 托盘、日志与 Pi 运行时基准

> 历史基准：本文记录当时的按日轮转与关闭即销毁实现。当前大小轮转和主 WebView隐藏复用策略见[后续回归](2026-08-17-iteration-1-log-window-lifecycle.md)。

- 日期：2026-08-17
- 机器架构：Apple Silicon / arm64
- 系统：macOS 26.5（25F71）
- 桌面构建：Rust `release`，Tauri 2.11.5，未打包
- Sidecar运行时：Node.js 24.18.0
- HAL100版本：`0.0.1-dev.0`

## 1. 桌面后台场景

1. 启动带系统托盘、SQLite和异步 JSONL日志的 HAL100 release二进制。
2. 1.2秒后通过仅在 `benchmark-hooks` feature中存在的测试钩子关闭主窗口并销毁 WebView。
3. 保持无窗口后台运行，使用 macOS `top`连续采样5次，使用 `footprint`核对物理占用。
4. 10秒后调用程序化明确退出路径，验证退出码和遗留进程。

该场景没有启动 Gateway监听器、Agent Kernel、Agent Model Runtime或用户推理引擎。

| 指标 | 结果 | 当前预算 |
| --- | --- | --- |
| 后台空闲 CPU | 5次均为0.0% | ≤0.3% |
| 物理内存 footprint | 40 MB | ≤80 MB |
| 稳定后主进程线程 | 19 | 暂无硬门槛 |
| Web内容进程 | 窗口关闭后不存在 | 必须不存在 |
| 明确退出 | 退出码0；无遗留主进程 | 必须完全退出 |

与未加入托盘和日志的31 MB初始空壳基准相比，物理占用增加约9 MB，仍保留40 MB预算空间。日志线程不产生周期性事件，5次采样均未观察到空闲 CPU活动。

## 2. 日志验证

日志写入 macOS标准应用日志目录下的 `hal100.YYYY-MM-DD.jsonl`，由非阻塞写入线程处理，按日轮转并最多保留7个文件。本次启动只产生以下三类结构化事件：

- `desktop_runtime_started`：版本、平台和架构。
- `database_ready`：数据库迁移版本。
- `tray_ready`：托盘初始化完成。

日志没有记录应用数据目录、日志目录、用户名、提示词、回答、API Key或 Authorization Header。通用 `Redacted<T>`格式化包装具有单元测试，`Display`和`Debug`均只能输出 `[REDACTED]`。

按日轮转适用于当前低频生命周期日志，但不是高吞吐日志的最终方案。Gateway进入主流程前仍需增加单文件大小上限和重复错误聚合。

## 3. Pi Agent Core Sidecar场景

1. 使用锁定的 Node.js 24.18.0启动编译后的 Agent Kernel子进程。
2. 真实导入并实例化 `@earendil-works/pi-agent-core` 0.84.2的 `Agent`。
3. 注入一个只会拒绝调用的模型流函数，工具列表为空，不发起模型或网络请求。
4. 通过 Agent RPC v1发送 `system.ping`，收到能力状态后保持短暂空闲，再发送 `system.shutdown`。

| 指标 | 单次结果 |
| --- | --- |
| 冷启动到 `system.pong` | 159.7 ms |
| Sidecar RSS | 77,584 KB |
| Sidecar物理内存 footprint | 39 MB |
| shutdown到进程退出 | 10.7 ms |
| 退出码 | 0 |
| 遗留 Agent Kernel进程 | 0 |

回包明确报告 `piEnabled=true`、`registeredToolCount=0`，并保持 Coding Agent、动态扩展、资源发现和直接工具执行全部关闭。这证明锁定的 Pi内核可在当前 Node基线运行，但尚不等于 HAL100工具链已经完成。

### 3.1 模拟工具闭环补充验证

随后加入一个仅在技术验证中注册的 `hal100.inspect_system_summary`代理。确定性 Faux模型流让真实 Pi Agent Core产生一次工具调用，Sidecar发送 `tool.call.request`；Rust测试宿主使用 `SimulatedToolBroker`重新校验并返回固定模拟结果，Pi收到结果后完成第二轮响应。

实际子进程序列为：一次工具请求、一次模拟完成、一次 shutdown确认；退出码为0，无遗留 Sidecar进程。完成载荷报告 `registeredToolCount=1`、`brokerRoundTrips=1`、`directSystemExecution=false`、`modelRequests=0`、`networkRequests=0`。Rust与 TypeScript分别验证未知工具、额外参数、错误关联和超时失败关闭路径。

### 3.2 macOS开发沙箱补充验证

在同一机器上使用显式开发沙箱和最小环境重新运行 Sidecar：冷启动到 `system.pong`为171.1 ms，物理内存 footprint仍为39 MB，shutdown到退出为2.9 ms。单次未沙箱样本为159.7 ms，因此当前样本增加约11.4 ms冷启动时间，没有观察到物理内存增加。

自动化测试还在沙箱内完成完整 Pi模拟工具闭环，并验证读取会话目录外测试文件、写入外部测试目录、连接本机监听端口及启动 `/bin/echo`全部被拒绝；PATH、SSH Agent和 HTTP代理变量未继承。该测试基于已弃用的 `sandbox-exec`和私有 SBPL，只是未签名开发版的回归探针，不纳入正式安全承诺。

同一完整测试连续执行10轮均通过，结束后临时会话目录和 Agent Kernel进程均为0。

## 4. 结论与后续

- 托盘、异步日志和无窗口后台生命周期仍满足长期驻留的初始资源预算。
- 程序化明确退出可完全终止后台进程；托盘菜单使用同一退出路径。
- Pi Sidecar保持按需边界，没有进入常驻 Rust Core；退出后释放全部进程资源。
- 模拟 HAL100工具闭环已经证明 Pi只负责提出请求，Rust是唯一校验和结果来源；下一项技术验证是评估 macOS Sidecar进程权限收缩。
- Sidecar冷启动和内存目前是单次开发机样本；产品化前需多轮统计 p50/p95，并复测自包含分发方案。
