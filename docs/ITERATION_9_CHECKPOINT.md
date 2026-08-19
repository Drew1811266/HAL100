# HAL100 迭代 9 暂停检查点

> 恢复结果：2026-08-19已从本检查点继续，最终1小时观察和全部迭代9门槛均通过。完整结果见[迭代9稳定性验收](benchmarks/2026-08-18-iteration-9-stability.md)。以下内容保留为暂停时的历史现场记录。

- 暂停时间：2026-08-18 19:06（Asia/Shanghai）
- 暂停原因：用户明确要求暂停并记录进度
- Goal状态：`paused`
- 当前阶段：迭代9“稳定性与内部测试准备”
- 开发形态：macOS Apple Silicon连续开发版，不打包

## 已完成

1. 建立1小时后台监控器，30秒采集CPU、RSS/物理内存、线程、文件、TCP、子进程、Agent会话目录、Gateway、日志大小、Usage/审计变化和敏感字段扫描；输出权限受限的CSV、JSON与Markdown报告。
2. 建立迭代9快速矩阵。最终重跑11/11阶段通过：全量检查、生产构建、SQLite占锁恢复、100万Usage、1万模型快照、真实Sidecar往返/25次启停/超大RPC、Gateway p95，以及官方OpenCode 1.18.11与1.17.9。
3. 实测100万Usage：插入6,799 ms、统计1,054 ms、预览128 ms、清理3,229 ms；1万模型快照刷新136 ms、列表38 ms；Gateway p95额外开销272 μs；Sidecar 25次启停3,833 ms。
4. OpenCode当前自动兼容下限固定为1.17.9；1.15.10在隔离测试中没有产生回答，明确记录为不兼容并显示升级警告。
5. 覆盖20并发流式限流和槽位释放、10100端口冲突、磁盘不足预检、SQLite损坏/占锁、后端断网与不重放、Sidecar异常帧和资源回收。
6. 5秒进程暂停/继续探针通过：Gateway恢复，前后Agent/模型子进程均为0。真机整机睡眠保留为内部测试员主动执行的手工项，不由脚本擅自触发。
7. 建立`docs/INTERNAL_TESTING.md`，包含主流程、故障矩阵、真机睡眠步骤、1小时门槛及S0—S3问题反馈/脱敏流程。
8. 增加仅debug生效的单实例隐藏窗口参数和安全包装脚本；release不响应，未运行HAL100时包装脚本不会意外启动新实例。
9. 自动审查发现并修复两项测试基础设施问题：`output/`被Biome当作源码、debug专用测试在release语义下的错误期待。

## 当前运行状态

- HAL100开发进程：PID 62006，暂停记录时CPU 0.0%，Gateway健康。
- 进程保持运行；没有退出或杀死用户启动的开发进程。
- 当前工作区仍是尚未建立初始Git提交的连续开发目录，`git status`中的项目文件均显示未跟踪；没有用破坏性Git命令处理。

## 1小时观察记录

最终1小时验收尚未完成，不能据此宣告迭代9完成。

- 第一次观察在186秒时因自动审查发现release测试条件问题而主动中止；报告：`output/stability/2026-08-18-iteration-9-1h-aborted-test-review/`。
- 修复并重新通过全量检查后，第二次观察在75秒时按用户要求暂停；4个样本CPU平均/最大均0.0%，物理内存42.3→42.4 MiB、最大42.5 MiB，文件/TCP增长0、线程增长1、Gateway失败0、Agent子进程与会话目录0、敏感审计/日志命中0。报告：`output/stability/2026-08-18-iteration-9-1h-paused-by-user/`。
- 两份报告的`passed=false`只由`monitor_interrupted`造成，不代表性能或安全门槛失败。

## 恢复开发时的顺序

1. 确认PID和Gateway；若应用重启，先运行`pnpm stability:hide-window`。当前PID若仍为62006且窗口保持隐藏，无需重启应用。
2. 确认没有Agent/模型子进程和会话目录。
3. 不修改Rust或前端，静置至少25秒后从零运行`pnpm stability:1h`；必须完整达到3,600秒。
4. 用最终JSON更新`docs/benchmarks/2026-08-18-iteration-9-stability.md`第4节，并同步`docs/PERFORMANCE.md`、`docs/ROADMAP.md`和README状态。
5. 执行最终`pnpm check`、`pnpm build`、脚本语法检查和`git diff --check`。若最终源码变更会影响后台运行时，1小时测试必须重新开始。
6. 所有门槛通过后才把迭代9和Goal标记完成；继续保持连续开发版，不生成安装包。

## 关键报告

- 完整快速矩阵：`output/stability/2026-08-18-iteration-9-matrix-rerun/summary.md`
- 暂停/恢复探针：`output/stability/2026-08-18-iteration-9-suspend-resume/summary.md`
- 用户暂停的后台样本：`output/stability/2026-08-18-iteration-9-1h-paused-by-user/summary.json`
- 迭代9验收草稿：`docs/benchmarks/2026-08-18-iteration-9-stability.md`
