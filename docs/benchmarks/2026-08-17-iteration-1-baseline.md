# 迭代 1 初始后台性能基准

- 日期：2026-08-17
- 机器架构：Apple Silicon / arm64
- 系统：macOS 26.5（25F71）
- 构建：Rust `release`，Tauri 2.11，未打包
- HAL100版本：`0.0.1-dev.0`

## 场景

1. 启动 HAL100 release二进制。
2. 初始化 React WebView、Tauri IPC和本地 SQLite。
3. 1.5秒后通过仅在 `benchmark-hooks` Cargo feature中存在的测试钩子关闭主窗口。
4. 保持 Rust事件循环运行。
5. 使用 macOS `top` 连续采样5次，每次间隔1秒；使用 `footprint` 和 `vmmap`核对物理占用。

该构建没有启动 Gateway监听器、Agent Kernel、Agent Model Runtime或用户推理引擎。

## 结果

| 指标 | 结果 | 当前预算 |
| --- | --- | --- |
| 后台空闲 CPU | 5次均为0.0% | ≤0.3% |
| 物理内存 footprint | 31 MB | ≤80 MB |
| 峰值 footprint | 32.3 MB | ≤80 MB |
| 主进程线程 | 13 | 暂无硬门槛 |
| Web内容进程 | 窗口关闭后不存在 | 必须不存在 |

`ps` 的 RSS约为108 MB，但其中包含大量系统框架共享的干净页面；macOS `footprint`报告的私有物理占用为31 MB。HAL100后续统一使用 Activity Monitor“内存”/`phys_footprint`作为80 MB预算的判定口径，同时保留 RSS作为诊断信息。

SQLite首次初始化产生主数据库、WAL与共享内存文件，迁移版本为1。空闲期间没有周期性日志和 Agent进程。

## 生命周期验证

- 原始实现关闭最后一个窗口后直接退出，未满足后台常驻要求。
- 修正后，只有 `ExitRequested.code == None` 的用户窗口关闭请求会被阻止退出；未来托盘“退出”调用程序化退出时仍可正常终止。
- 主窗口销毁后，通过再次启动同一二进制触发单实例回调；第二进程立即退出，后台进程保持唯一并进入窗口恢复路径。
- macOS Dock `Reopen`事件也进入同一窗口恢复函数。

## 限制与后续

- 当前是空壳基准，只证明架构起点满足预算，不代表加入 Gateway和长期任务后的最终结果。
- 尚未进行1小时稳定性、睡眠唤醒、重复创建/销毁窗口和句柄泄漏测试。
- Vite开发服务器启动时间149 ms、Rust热构建启动0.36秒只作为开发体验记录，不是产品冷启动指标。
- 迭代2加入 Gateway和 Usage写入器后必须按同一口径重新测量。
