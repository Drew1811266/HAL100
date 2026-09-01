# HAL100 性能预算

## 1. 原则

HAL100是长期后台软件。性能不是发布前优化项，而是每次迭代的回归门槛。

资源必须按生命周期拆分：

- 常驻：Rust Core、托盘、网关、SQLite连接；主 WebView首次创建后隐藏复用。
- 按需：首次打开前的 React WebView、Agent Kernel Sidecar、Agent Model Runtime、用户模型、目录扫描和下载任务。
- 外部：Ollama、vLLM等不计入 HAL100 Core开销，但需单独展示。

## 2. 初始性能目标

以下是工程验收目标，需通过实测校准，不是未经验证的产品承诺。

| 状态 | Apple Silicon目标 |
| --- | --- |
| 后台空闲、主窗口隐藏或尚未创建、无模型 | 平均 CPU ≤ 0.3%，常驻内存 ≤ 80 MB |
| UI打开且静止 | 平均 CPU ≤ 1%，HAL100总内存 ≤ 180 MB |
| 本机网关转发 | p95附加延迟 ≤ 5 ms |
| 20个并发流式连接 | Core内存保持 ≤ 120 MB且无持续增长 |
| Agent空闲 | Kernel Sidecar和 Model Runtime均退出，Agent模型内存为0 |
| 1小时空闲运行 | 无持续内存、句柄、连接和唤醒频率增长 |

用户推理模型本身的 RAM、GPU和 CPU占用单独统计。

## 3. WebView生命周期

- 登录后台启动且界面尚未使用时可以不创建 WebView。
- 主 WebView创建后，窗口关闭时隐藏并复用，直至明确退出。
- React状态不能作为长期事实源。
- 托盘打开窗口时显示既有主 WebView；如尚未创建则创建，并从 Rust Core加载状态。
- UI不可见时不存在前端轮询、动画、计时器和图表刷新。
- Token统计图使用项目内静态SVG组件，每次最多绘制30个请求点；不引入图表运行时依赖、不注册ResizeObserver，也不进行逐帧动画。
- 临时窗口关闭时销毁；强制销毁主 WebView只用于带 feature的框架回收诊断。

2026-08-19图表与排版回归的生产构建、CPU和内存采样见[排版与Token可视化回归](benchmarks/2026-08-19-layout-and-usage-visualization.md)。

## 4. 网关性能

- 使用异步 I/O。
- 流式响应采用有界缓冲和背压。
- 同协议转发不缓存完整响应。
- 客户端断开立即传播取消。
- 解析 Usage不能阻塞数据转发。
- CPU密集 Token估算若在后续启用必须进入受控工作线程池；当前 Alpha缺失后端Usage时只标记`unavailable`，不执行估算。
- 为每个后端设置连接池、并发上限和超时。
- 所有内部通道都有容量上限，避免慢客户端造成内存增长。

## 5. 数据库写入

- 流式期间只更新内存请求状态。
- 请求结束后产生一条 Usage事件。
- SQLite由单写入器消费有界队列。
- 使用 WAL和短事务批量提交。
- 为常见统计维度和开始时间建立索引；日/小时数据从同一Usage事实表按范围聚合。
- 保留策略不运行后台清理；用户预览并原生确认后才显式删除过期记录。
- 数据库维护不得在主请求路径执行。

## 6. 当前监测策略

| 状态 | 当前策略 |
| --- | --- |
| 后台空闲 | 不轮询 |
| 有推理请求 | 使用请求和流式事件，不另开指标轮询 |
| Usage页面可见 | 用户进入页面或显式刷新时查询，不后台轮询 |
| 下载/安装任务 | 只在窗口活跃且任务进行中轮询相关任务 |

禁止为了读取指标而每秒启动 `system_profiler`、`ioreg`、PowerShell或其他外部进程。优先使用原生 API和已有子进程状态。

## 7. 模型目录

- 使用 macOS文件事件进行增量索引。
- 对事件防抖和去重。
- 不定时递归扫描全部模型目录。
- 外部模型启动时根据路径、大小和修改时间识别变化。
- 只在受控下载完成或用户明确要求时计算大文件完整哈希。

## 8. Agent资源

- Pi Agent Core只存在于按需 Agent Kernel Sidecar，不进入长期常驻 Rust Core。
- Kernel Sidecar和本地 Agent Model Runtime均懒加载。
- Sidecar按单任务启动并在RPC shutdown后立即退出；本地 Model Runtime在最后一次任务后空闲两分钟释放模型与 KV Cache。
- 确定性路由已明确任务、澄清或拒绝时不调用Pi意图分类；只有`Unresolved`请求最多增加一次零工具模型回合。意图模型固定`temperature=0`和128输出Token上限；真实Qwen在6场景×3轮热模型验收中的p95与最大推理延迟均约2.61秒。该数字不包含首次模型Runtime启动，不应外推为完整任务延迟。
- 确定性澄清与拒绝由Rust固定回答，不启动Kernel、Agent Model Runtime或临时Gateway路由；真实模型只用于需要工具编排、零工具解释或确定性入口未解析的Pi意图任务。
- 结构化任务只下发工作流叶能力及其前置闭包，不与旧关键词工具求并集；这限制无关工具定义、模型回合和工具结果Token。含工具未解析时不进行额外回退模型调用。
- 执行系统指令只装配当前任务工具，Provider上下文只保留最新直接工具依赖；最终计划成功后固定
  收口，不再调用模型复述计划。v9的18条动作路径最小模型回合由55降到37（-32.7%），三工具
  隔离对照重复结果Token归零且发送量至少下降40%。
- RPC v13只在任务结束时计算有界数值指标；Provider精确Usage与`ceil(可见字符数/4)`工具结果
  对照估算分开，不保留正文，不新增后台线程、计时器或采样循环。
- 任务检查点是单个小型Rust内存对象，只在既有运行、确认、执行或显式状态读取路径更新；计划
  到期在状态读取时惰性回收，不新增后台timer、轮询、SQLite写入或Sidecar回合。
- Agent模型选择同时考虑工具正确率、冷启动和峰值内存。
- 云端模式只启动 Kernel Sidecar，不得启动本地 Agent Model Runtime。
- Agent工具执行不得占用网关异步线程。
- Sidecar与 Rust之间的 RPC必须有界，流式文本和工具进度不能形成无界队列。
- Sidecar启动时不得扫描用户目录、项目目录、Pi资源或联网刷新模型目录。
- Sidecar退出后不得遗留 Node/Bun、llama.cpp子进程、管道、临时文件或定时器。
- Sidecar活动内存、冷启动和退出时间在技术验证中单独设定预算，不得通过提高后台空闲预算消化其开销。

## 9. 日志

- 不记录流式正文。
- 默认关闭 Debug级别。
- 日志按大小轮转并限制保留量。
- 高频重复错误合并计数。
- 后台空闲时不产生周期性“正常”日志。

迭代1开发版采用异步 JSONL：活动文件上限5 MiB，最多保留6个归档，即正常总量约35 MiB；日志目录为 `0700`，文件为 `0600`。同一稳定错误码使用60秒无定时器聚合窗口。Gateway和后续后台任务接入错误日志时必须使用该聚合边界。

OpenCode检测与配置不加入后台循环。CLI版本检测只在软件接入页面加载或用户主动刷新时执行，最多等待2秒并调度到阻塞任务线程；配置解析、备份和原子写入同样不占用Gateway异步I/O线程。页面未打开时该模块没有线程、计时器、文件监听或额外文件句柄。

迭代4硬件画像同样是按需操作：模型页首次打开或用户点击刷新时，在阻塞任务中各执行一次固定`sysctl`和`df`读取；React Query使用无限`staleTime`且关闭聚焦自动刷新。模型页没有轮询，窗口隐藏后硬件和模型目录模块均不会产生周期唤醒。若未来需要1—2秒性能图采样，不得复用这条外部进程路径。

迭代4 Alpha新增远端目录、下载管理、托管 llama.cpp、单轮模型测试和 Usage统计后仍保持事件驱动：目录只在搜索时联网；下载活动期间才进行500 ms界面刷新，结束后停止；进度约每4 MiB写一次；引擎状态与统计页不轮询；未显式启动模型时不存在`llama-server`进程。

迭代6把下载刷新进一步绑定到窗口可见且聚焦状态：`visibilitychange`、`focus`和`blur`事件只更新内存布尔值，不运行计时器；隐藏或失焦时React Query返回`false`并停止刷新。首次启动、设置、审计、通用接入和数据清理均为页面加载或用户操作触发，没有`setInterval`、`setTimeout`或后台保留任务。2026-08-18开发进程空闲5次CPU采样均为0.0%，`vmmap`物理占用40.8 MiB、峰值43.3 MiB，仍满足80 MiB预算。

迭代7的Agent同样不加入常驻轮询：Agent页状态只在页面进入、任务完成或用户操作后刷新；Sidecar每个任务退出；模型运行时只安排一个可被generation失效的空闲timer。首条Apple M1真实5项场景完成率5/5，冷请求18.157秒，热请求7.466—17.744秒。第二条最终固定9项场景完成率9/9：冷请求24.098秒，运行目录读取13.722秒，双工具计划28.479秒，冷启动取消6 ms、推理中取消及回收98 ms；聚焦计划探针另观测到一次58.45秒的有界纠偏最坏样本。活动时 llama-server私有物理占用386.4 MiB，Node Sidecar 52.3 MiB、峰值78.2 MiB；模型文件mmap使RSS约1.61 GiB，因此内存记录同时保留RSS与physical footprint。第二条验收后同样无Agent子进程、会话目录或临时Key；桌面进程5次CPU采样为0.0、0.0、0.5、0.0、0.0%，私有物理占用41.2 MiB、峰值43.1 MiB。完整记录见[首条本地Agent闭环](benchmarks/2026-08-18-iteration-7-local-agent.md)与[状态、计划和取消闭环](benchmarks/2026-08-18-iteration-7-agent-actions.md)。

迭代8两种云端范围继续保持用户触发：后端目录和内存会话状态只在页面首次进入或用户刷新时读取，预览、原生确认和请求都无后台计时器；活动会话只是一个小型Rust内存对象，不保持连接、Sidecar或模型。每项云端任务只启动短生命周期Node Sidecar，不启动本地Qwen。当前会话无网真实Gateway纵向测试约0.34秒完成，任务后临时路由、客户端Key和Sidecar均回收。若既有本地Qwen空闲timer在云端任务期间到期，timer不旋转代次，只以100毫秒低频等待运行锁释放后立即停止本地运行时，避免云端任务造成模型长期驻留。最终生产构建后的5次CPU采样为0.0%、0.0%、0.0%、0.5%、0.1%，`vmmap`物理占用42.2 MiB、峰值43.1 MiB，且不存在Agent子进程，继续满足80 MiB后台预算。详见[云端Agent双范围闭环](benchmarks/2026-08-18-iteration-8-cloud-agent.md)。

迭代9在主窗口隐藏、无模型和无Agent任务的当前连续开发版上完成3,601秒后台观察：117个样本的平均CPU为0.0043%、最大0.3%，物理内存41.7→42.0 MiB、最大42.2 MiB；文件和TCP首尾增长均为0，线程减少1。Gateway失败、Agent子进程、会话目录、不安全审计行及疑似含密钥日志文件均为0，Usage和审计表在空闲期间没有新增记录。结果满足本文件的全部1小时门槛，详见[迭代9稳定性验收](benchmarks/2026-08-18-iteration-9-stability.md)。

当前 Apple M1上连续100次完整`sysctl + df`探测总墙钟0.30秒，约3毫秒/次；这是用户触发延迟，不计入后台空闲成本。GGUF导入只读取24字节固定头并执行文件metadata检查，不在导入路径计算大文件哈希。已索引文件状态检查只遍历数据库中的确定路径，并且只在模型页加载或显式刷新时执行。

## 10. 验证矩阵

- 网关：1、10、20并发流式连接。
- 生命周期：反复隐藏/显示生产主 WebView；另以测试 feature反复创建/销毁，观察框架回收。
- Agent：分别反复启动/终止 Kernel Sidecar与 Model Runtime，检查内存、句柄、管道和子进程回收。
- 数据：百万条 Usage查询、聚合和清理。
- 文件：大模型目录增量变更。
- 系统：睡眠/唤醒、断网、端口冲突。
- 稳定性：1小时后台运行。
- 故障：后端崩溃、客户端取消、数据库忙和磁盘空间不足。

macOS开发期使用 Instruments、Activity Monitor、Energy Log和 `powermetrics`。性能测试结果需记录机器、系统版本、构建模式和样本条件。

## 11. 已记录基准

- [2026-08-17：迭代1初始后台性能基准](benchmarks/2026-08-17-iteration-1-baseline.md)
- [2026-08-17：迭代1托盘、日志与 Pi运行时基准](benchmarks/2026-08-17-iteration-1-tray-logging-pi.md)
- [2026-08-17：迭代1日志与窗口生命周期回归](benchmarks/2026-08-17-iteration-1-log-window-lifecycle.md)
- [2026-08-17：迭代2 Gateway最小闭环基准](benchmarks/2026-08-17-iteration-2-gateway-baseline.md)
- [2026-08-18：迭代3 OpenCode核心链路基准](benchmarks/2026-08-18-iteration-3-opencode-core.md)
- [2026-08-18：迭代4模型管理基础基准](benchmarks/2026-08-18-iteration-4-model-foundation.md)
- [2026-08-18：迭代7本地Agent纵向闭环](benchmarks/2026-08-18-iteration-7-local-agent.md)
- [2026-08-18：迭代7 Agent状态、计划与取消](benchmarks/2026-08-18-iteration-7-agent-actions.md)
- [2026-08-18：迭代8云端Agent双范围纵向闭环](benchmarks/2026-08-18-iteration-8-cloud-agent.md)
- [2026-08-25：迭代36任务状态机与脱敏检查点](benchmarks/2026-08-25-iteration-36-task-checkpoints.md)
- [2026-08-25：迭代41 Agent效率与长上下文装配](benchmarks/2026-08-25-iteration-41-agent-efficiency-and-context.md)
- [2026-08-26：迭代43设备感知Agent长上下文与连续任务稳定性](benchmarks/2026-08-26-iteration-43-device-aware-agent-context.md)
- [2026-08-18—19：迭代9稳定性与内部测试准备验收](benchmarks/2026-08-18-iteration-9-stability.md)
- [2026-08-18：迭代4 Alpha核心闭环基准](benchmarks/2026-08-18-iteration-4-alpha-core.md)
