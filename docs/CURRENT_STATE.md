# HAL100 当前开发状态

- 状态日期：2026-09-02
- 当前版本：1.0.6
- 当前阶段：开发初期，连续内部开发与测试
- 已完成迭代：0—59
- 当前迭代：60，多引擎确定性推荐、七类支持证据与三平台验收收口进行中；三个历史正式外部格已全部迁入真实账本，其余25个支持格仍待目标平台验收
- 用户体验阶段：UX-1—UX-5已完成；五入口、目标导向首次使用、四页签模型工作区与响应式验收已落地

本文是 HAL100 易变实现事实的唯一现行入口。产品愿景、安全原则和架构方向分别由产品、
安全与架构文档维护；ADR、旧迭代章节和基准记录保留其发生时的历史事实，不随当前版本追溯
改写。

UX-1—UX-5现行界面重构与分阶段门禁见
[用户目标导向界面重构验收记录](benchmarks/2026-09-02-ux-five-phase-refresh.md)。

迭代50实现记录见[外部推理引擎运行方案受控闭环](ITERATION_50_CHECKPOINT.md)。本轮已完成
Gateway完整活动路由、外部方案身份、实时复检、一次性切换、回滚、桌面重验和Pi受控计划链路；
完整质量门禁已通过。

迭代51—60路线见[推理引擎正式支持计划](INFERENCE_ENGINE_SUPPORT_PLAN.md)。当前检查点见
 [正式引擎支持合同与证据模型](ITERATION_51_CHECKPOINT.md)：spec v3/schema v15、类型化证据、
精确适配器/实例/origin/配置修订绑定、Keychain认证注入、多实例观察、持久化激活journal和启动
补偿均已落地。Windows/Linux基础宿主探针可按目标编译；Windows固定CIM显卡与ComputeAccelerator
查询现在可生成CUDA/ROCm/Vulkan/Intel GPU/Intel NPU候选，Linux通过有界DRM厂商、render node、
NVIDIA驱动、AMD `/dev/kfd`及Intel `/sys/class/accel` + `/dev/accel`证据生成对应候选；这些
均仍需引擎资格验证。vLLM已具备固定版本/健康/模型端点、目录身份、
命名工具调用与SSE Usage资格探针；资格报告会绑定同一有界`/version`精确版本，并提供显式真机验收入口，但在真实Linux/CUDA矩阵通过前继续保持
`connected`，不显示成正式支持。MLX-LM已接入官方`mlx_lm.server`回环适配器，并在Apple M1 /
Metal、MLX-LM 0.31.3、Qwen3-0.6B-4bit上完成真实模型资格、保存方案、激活、切换后复验与
恢复闭环，支持单元为`verifiedExternal`。MLC LLM已接入官方`mlc_llm serve` OpenAI回环协议，并按
Metal/Vulkan/CUDA/ROCm拆成四个单设备适配器变体，覆盖macOS/Windows/Linux的候选平台矩阵，完成
模型目录、MLC模型格式、共享Chat/流式/Usage/工具资格夹具。
官方服务没有稳定包版本端点且当前固定返回空`system_fingerprint`；正式资格不再依赖该不可达字段，
而是要求服务目录中的模型ID为绝对本地MLC部署目录，由Rust有界读取并哈希`mlc-chat-config.json`、
权重清单、清单声明的全部分片和tokenizer文件；聊天模板还必须恰好暴露一个`{function_string}`
插槽。官方非流式工具响应的参数对象只在Rust验证为MLC的后端上由Gateway转成OpenAI JSON字符串，
带工具的流式请求因尚未验收而故障关闭。`HF://`或普通目录别名仍可发现，但不能保存成可执行
运行方案；通过本地部署内容指纹和Gateway标准形态资格后使用显式“版本未暴露”标记，不把该标记当作版本。
Apple Metal上的固定Qwen3.5-2B MLC部署已通过单次目录、工具、普通流式、Usage和Gateway形态验证，
但官方0.20调度器在工具请求后的后续请求中可复现内部rollback失败，当前0.26 nightly又存在编译/
运行包接口与ABI漂移，故20次稳定性和持续生命周期未通过、没有验收产物或账本记录。真实平台Agent
协议与控制面验收完成前，支持单元保持`connected`，不会进入正式支持矩阵；详见
[MLC LLM Apple Metal阻塞记录](benchmarks/2026-08-31-mlc-llm-macos-metal-blocked.md)。
Ollama现已接入共享OpenAI Agent资格探针；其运行方案保存、激活和复验不再绕过实时资格请求，仍保留
Ollama digest 作为内容身份证据，并可在资格响应提供指纹时绑定部署身份。资格报告还在模型驻留时
读取官方 `/api/ps`：零设备内存证明 CPU，回环 macOS 上非零设备内存证明 Metal；多正式加速器格
若没有精确观察或观察与方案选择不符，保存、复验、激活前和切换后检查都会失败关闭。
OpenVINO Model Server（OVMS）已接入官方KServe `/v2`元数据/健康端点、`/v1/models`目录和OpenAI
Chat/流式/Usage/工具资格探针；CPU、Intel GPU、Intel NPU拆为三个单设备变体并各自覆盖
Windows/Linux x86_64，但Intel设备插件和真实部署验收完成前保持`connected`。SGLang 已接入官方 `/server_info`、`/health`、`/v1/models`
和共享OpenAI资格探针，当前Linux/CUDA支持单元保持`connected`。LMDeploy 已接入官方
`api_server` 的 `/health`、`/v1/models` 和共享OpenAI资格探针，当前Linux/Windows x86_64 CUDA
候选保持`connected`，因为官方服务没有稳定的机器可读版本端点，TurboMind/PyTorch变体及真实部署
标识绑定为部署指纹，但仍需由验收入口确认TurboMind/PyTorch变体及真实部署证据。TensorRT-LLM 已接入官方 `trtllm-serve` 的 `/version`、`/health`、
`/v1/models` 和共享OpenAI资格探针，当前Linux x86_64/aarch64 CUDA候选保持`connected`；真实
NVIDIA GPU、HF checkpoint/TensorRT engine形态、后端配置和运行方案闭环完成前不授予正式支持。

迭代55与迭代56的适配器实现和停止门槛分别记录在[MLC LLM检查点](ITERATION_55_CHECKPOINT.md)
与[OpenVINO Model Server检查点](ITERATION_56_CHECKPOINT.md)。SGLang 与 LMDeploy 的共同软件
纵向和验收入口分别记录在[SGLang检查点](ITERATION_57_CHECKPOINT.md)与[LMDeploy检查点](ITERATION_58_CHECKPOINT.md)。
TensorRT-LLM 记录在[TensorRT-LLM检查点](ITERATION_59_CHECKPOINT.md)。
迭代60的共同收口已经启动：能力目录除Rust确定性推荐外，现按七类证据显示每个当前支持单元的
已验证项和缺口；Pi运行时目录同步获得不含端点、摘要、路径、命令或凭据的静态引擎能力投影，可用于解释主机兼容性和支持格但不具备激活权限；验收账本构造与审查晋级现在都会校验结构化协议能力哈希与适配器变体一致，避免跨变体重放；实现记录见
[多引擎选择与证据收口检查点](ITERATION_60_CHECKPOINT.md)。

## 1. 当前范围边界

- 主要桌面开发和真机测试仍在Apple Silicon macOS；最低工程基线为macOS 13 Ventura。
- 界面仅提供中文。
- 当前只维护可从仓库启动的开发版，不制作或规划对外发行物。
- 签名、公证、应用商店、安装包、自动更新、正式升级流程和正式发布流水线均不属于当前开发
  范围，也不作为当前缺陷、完成条件或默认下一阶段任务。
- Intel Mac、移动端和Web版不在当前实现范围。Windows/Linux已进入源码编译与宿主探针实现
  范围，但完整桌面纵向和具体引擎支持必须逐平台验收，当前不作笼统可运行承诺。
- 版本号和 Git 标签只标记可复现的开发进度，不表示正式发布或兼容性承诺。

## 2. 当前实现基线

| 项目 | 当前事实 |
| --- | --- |
| 一级入口 | 首页、模型与运行、软件接入、Agent、活动；设置独立位于侧栏底部 |
| 模型工作区 | 模型库、本地运行、连接服务、快捷切换四个稳定页签；`/profiles`兼容重定向到`/workspace/profiles` |
| 首次使用 | 首页直接提供已有本机服务、本地模型、云服务三条目标路径；只有真实保存/探测/激活服务或真实准备模型后才完成，不再由下载源和登录启动偏好阻塞 |
| 界面骨架 | 页面标题先于同域子导航；业务内容最大宽度1120 px，Agent最大宽度1140 px；低于1100 px时Agent工作区转为单列，880 px最小窗口无横向溢出；880×700下任务输入与主执行按钮同屏可见 |
| 桌面栈 | Tauri 2、React 19、TypeScript 6、Vite 8 |
| 核心栈 | Rust 1.97.1、Tokio、Axum、rusqlite |
| 本地 Gateway | `127.0.0.1:10100`；Models、Chat Completions、Responses、Anthropic Messages |
| 数据库 | SQLite WAL，schema v15 |
| 推理引擎边界 | `NativeSystemProbe`按需生成平台/架构/内存/存储/加速器能力快照；macOS、Windows、Linux及aarch64/x86_64具有分平台实现。macOS Apple Silicon确定性报告CPU/Metal；macOS x86_64只有在只读`system_profiler` JSON明确包含Metal家族能力键时才报告Metal，该进程固定5秒时限、256 KiB stdout和16 KiB stderr上限，缺键、命令失败、超时、畸形或超限时故障关闭，且该探针基线不等于Intel Mac产品支持。Windows固定CIM显卡和ComputeAccelerator查询把有界PCI厂商身份映射为保守的CUDA/ROCm/Vulkan/Intel GPU/Intel NPU候选；Linux通过有界DRM厂商、render node、NVIDIA驱动、AMD `/dev/kfd`和Intel accel class证据生成对应候选；这些候选仍需引擎资格验证。`InferenceEngineAdapter`拥有托管生命周期，`EngineInspector`只检查Rust验证过的目标并可执行显式、有界资格探针。适配器身份由引擎/变体/合同修订组成，合同修订号统一来自Protocol常量；支持状态按平台/架构/加速器/部署逐格声明；运行方案协议哈希预期同样绑定完整适配器身份，未知变体或合同版本故障关闭。OVMS因官方HTTP接口不暴露`target_device`，被拆成CPU、Intel GPU、Intel NPU三个单设备适配器变体；运行方案必须显式绑定变体，宿主探针与审查后的原生真机运行共同证明支持格，资格报告不伪造设备观察。外部目标绑定实例、origin指纹、配置修订和Rust内Keychain认证；HTTP禁代理、禁重定向并限制超时/响应。目录支持多个已保存实例，显示短缓存不能用于授权；默认发现构造与桌面生产组合根共享审查晋级注册表，避免能力目录和运行方案状态漂移。当前正式单元为Apple Silicon/Metal llama.cpp、macOS CPU/Metal Ollama和MLX-LM Apple Silicon/Metal；vLLM、MLC LLM、OpenVINO、SGLang、LMDeploy与TensorRT-LLM为`connected`，均未越过真机身份与运行方案门槛；能力目录额外返回Rust确定性的解释性排序和七类证据进度，但排序与证据摘要都不具备激活权限。迭代60当前使用v4验收证据账本与JSON Schema；新live产物必须携带`nativeHostProbeV1`，把原生探针修订、精确支持格和隐私安全的设备类别SHA-256绑定在一起，不包含序列号、存储路径、端点、命令或凭据。现有Ollama CPU、Ollama Metal与MLX-LM Metal三条记录显式保留为`legacyHostSummaryV1`，不伪造设备指纹；新导入和重新资格验证拒绝legacy。八个入口共享20次/每波4并发的结构化稳定性探针，v4会把固定工作负载修订、p95/最大延迟、Token总量和墙钟时间保留到正式记录；v4还将测量绑定到脱敏的类型化模型证据指纹与原生设备类别；三条历史记录明确缺测量及模型证据，当前报告为0条已审查性能档案、3个正式外部格缺性能档案，推荐器不推断空值。运行产物不具备晋级权限；运行产物还支持取消、失败切换回滚、重启补偿的三项结构化韧性字段，新正式记录要求测量与三项韧性全部通过；注册表另提供审查账本驱动的精确支持格内存晋级入口，缺少记录的格继续保持`connected` |
| Agent 私有协议 | RPC v13，20 个固定工具，单任务最多 4 个工具和 1 个写计划；Rust显式传递设备容量，完成载荷含Rust复核的回合、上下文与重复工具结果指标 |
| 内置 Agent | Qwen3.5-2B Q4_K_M；Pi Agent Core/Pi AI 0.84.2；低于16 GiB为16384 Token基线，16 GiB及以上为已验收32768 Token标准档；768 Token输出、温度0、按任务 Sidecar |
| Agent任务架构 | Core已定义20类任务、成功谓词和证据来源白名单；结构化意图schema v1已接入；首版配置评测25场景，双路裁决评测8/8，真实Pi意图评测6场景×3轮为18/18，受控路由评测14/14；开放中文评测43场景覆盖20/20任务，真实Pi开放子集12场景×2轮为24/24；动作纵向矩阵20条路径覆盖12/12受控任务、11/11原生动作及四适配器配置/断开；任务状态机已接入有界澄清、运行、计划、确认、执行与动作专属复验，schema v3脱敏检查点生命周期评测10/10、有界澄清评测10/10、成功谓词评测20/20、证据故障注入6/6；Pi按任务装配指令和直接工具依赖，最终计划成功后确定性停止，只允许同进程继续固定澄清槽位、精确待确认计划或最多1次同任务重规划；复合图schema v1与v10的12类语义、v11恢复合同的9类语义已实现，Rust固定3/4节点就绪图、证据解锁、失败传播、显式补偿与重启重验证，桌面已接入逐节点Sidecar、原生确认关联、动作复验及用户可见的创建/继续/取消/显式补偿/恢复入口；补偿资格要求同一Rust谓词在确定性执行前后明确发生`Unsatisfied→Satisfied`转换 |
| 个人运行方案 | schema v15/spec v3区分托管与外部所有权，并保存精确适配器、后端配置修订、origin指纹、协议能力指纹、`contentDigest/repositoryRevision/deploymentFingerprint/catalogIdentity`类型化证据及显式“平台 × 架构 × 加速器 × 部署”支持格。schema v15移除含混的`openvino`加速器存储值，旧OVMS支持格失效为待修复状态，重新选择CPU/Intel GPU/Intel NPU变体后才能复验。兼容摘要列不再冒充所有引擎的内容哈希。方案支持多个命名组合、实时漂移状态、一次性预检、确认后再校验、原子切换、切换后复验和持久化journal补偿；支持格缺失、改变或与宿主/正式清单不匹配时故障关闭，不能静默推断跨加速器授权；同一适配器同一平台/架构/部署存在多个正式加速器时还必须由实时资格报告精确证明实际运行设备，单设备变体则由精确适配器身份锁定设备合同；启动时未完成事务只能恢复旧状态。外部身份漂移只能由桌面原生确认后显式重验，Pi不能静默接受 |
| 外部Agent模型配置 | `managed-route-v3`；HAL100托管llama.cpp与Pi/OpenClaw受管片段统一使用Rust设备档16384/32768 Token上下文、1024 Token最大输出；Hermes仍要求至少64000 Token并故障关闭不足配置 |
| 专用外部 Agent | OpenCode、Pi Coding Agent、OpenClaw、Hermes Agent |
| 通用接入 | OpenAI 兼容与 Anthropic Messages 客户端，独立本地凭据 |
| 用量口径 | 只累计后端返回的精确 Usage；缺失时标记 `unavailable`，不估算 |
| 用户模型生命周期 | 由用户显式启动、切换和停止；当前未启用 15 分钟自动卸载 |
| 浏览器模式 | 只读预览；不执行真实下载、安装、配置、删除或 Agent 任务 |

UX-1—UX-5最终质量门禁已通过：`pnpm check`覆盖文档一致性、Biome、TypeScript、Desktop 29项与
Agent Kernel 34项测试、Clippy `-D warnings`和完整Rust Workspace测试；`pnpm build`覆盖Desktop
生产前端、Agent Kernel与Rust Workspace。真实Chromium按1280×800和880×700覆盖关键任务，
控制台0 error、0 warning，880 px无横向溢出。

真实引擎验收入口现在统一调用三平台原生 `NativeSystemProbe`，拒绝用合成 CPU/GPU/内存快照
生成平台运行时证据；OpenVINO 还要求显式选择 `cpu`、`intel_gpu` 或 `intel_npu` 支持格并由探针确认。真实
引擎验收产物现在可通过 `hal100-engine-acceptance-import` 进行离线审查导入：维护者必须
显式替换模型 ID 脱敏摘要为可复核修订，工具会重新校验七类证据、三项结构化韧性检查、标准适配器支持格和输出文件的
create-new 边界；产物还绑定 Rust 已验证目标的实例 ID、origin 指纹、配置修订和结构化协议能力哈希，防止跨实例或协议断言重放；
它不会启动引擎、下载模型、覆盖原账本或自动晋级支持状态。维护者重跑同一精确支持格时可显式
指定旧记录 ID；替换只允许发生在完全相同的适配器和支持格，仍以 create-new 候选输出并保持原账本
不变。
证据来源还会限制为仓库相对的 `contracts/`、`crates/`、`docs/` 或 `README.md`，拒绝路径穿越、URL、
绝对路径和其他仓库外引用。

Infra 另提供只读 `hal100-engine-support-report` 覆盖报告：它按适配器和精确支持格汇总当前状态、
外部账本覆盖、待补证据及严格晋级条件，并在发现未映射的陈旧账本记录时故障关闭；正式托管格由
HAL100 自有 manifest/供应链/生命周期证据满足，不要求伪造外部账本记录；`--strict` 可作为
维护者导入前的非零门禁。MLX-LM 1 格和 Ollama 2 格属于 v1 账本前已有的历史正式格；三格均已于
2026-08-31 迁入真实记录，CI 债务棘轮已清零，并继续禁止出现任何新的无账本正式外部格。当前覆盖
统计为4个正式格、25个待验收格、3条外部账本记录、0个正式格缺账本；报告schema v3还显示0条
完整作用域性能档案、3个正式外部格缺性能档案，并保证origin/配置、引擎、模型、宿主与测量来自
同一记录；任一字段缺失时整个档案未知，不会被解释为零延迟或零吞吐；整体严格完成仍为 false，
因为25个规划格尚未完成真机验收。报告不连接或启动
引擎，不改变账本，也不替代原生三平台真实验收。

运行方案目录只在当前方案与v4记录的适配器/支持格、origin指纹、配置修订、引擎身份、类型化模型
证据指纹和原生设备类别全部精确一致时，投影同一固定工作负载的p95与样本吞吐；任一漂移或历史
记录缺字段都返回未知。Desktop将其标记为受审阅参考而非实时保证，Pi只可在相同
`workloadRevision`的精确方案之间比较，不能跨模型、设备或工作负载泛化。该投影不参与激活授权，
也没有改变4/25/3的支持统计。

八个 ignored live acceptance 在任何外部请求前先由共同preflight完成manifest支持格解析和原生
宿主平台/架构/加速器证明，错误坐标不会先触碰真实服务。它们可通过
`scripts/run-engine-live-acceptance.sh`（macOS/Linux）或
`scripts/run-engine-live-acceptance.ps1`（Windows）的白名单入口执行；脚本要求操作者显式确认真实
请求，只写出 create-new 脱敏产物，不会启动/安装引擎或自动导入账本。全部入口现统一使用 manifest
驱动的本地支持格选择器；当前版本化矩阵为8个外部引擎、13个外部适配器变体、28个外部支持格；合并托管llama.cpp后为14个适配器、29个支持格，其中4个正式、25个待验收。非 ignored 回归证明全部声明格均有可执行入口，并拒绝
未知或未声明坐标。回归还逐格构造完整但仅限测试内存的审查材料，证明 28 个外部格都能通过运行
产物校验、人工模型修订、原子账本追加、canonical 协议哈希复核和审查注册表投影；与托管 llama.cpp
合并后的候选严格报告可达到 29/29。入口选择仍必须经过原生宿主探针，这些结构夹具不构成真实服务
证据，也不会写入标准账本。
仓库另有仅手动触发的 `.github/workflows/live-engine-acceptance.yml`，使用四类静态、隔离的
`hal100-acceptance` 自托管runner标签覆盖macOS ARM64、Linux x64/ARM64和Windows x64；公开输入只
选择引擎/平台/加速器，回环API、模型ID或本地路径、版本及可选密钥来自对应精确支持格的受保护
GitHub Environment secrets，并在checkout前检查存在性。成功时上传短期create-new脱敏产物；流程
不自动准备服务、不自动导入或晋级，且按目标平台串行化，避免固定服务被并发验收相互污染。
checkout后、发送请求前还由Bash/PowerShell共用的Node预检按所选引擎验证精确变量、严格
`127.0.0.1` HTTP origin、显式端口/尾斜杠和允许的设备键；输出只含变量名与通过状态，不回显
端点、模型、版本或密钥。Rust原生宿主与服务资格检查仍是后续权威，预检本身不构成验收证据。
真机job之前还有一个不读取Environment secrets的Ubuntu验证job，通过
`scripts/validate-engine-acceptance-coordinate.mjs`将选择解析到`v1-support-matrix.json`中的唯一
适配器支持格；不存在或含混的组合在占用自托管runner前失败。28个外部格及一个矩阵外拒绝用例
已纳入文档一致性棘轮，该预检只证明声明可达，不构成平台或服务证据。

当宿主同时具备多个加速器且匹配支持格的正式程度不一致时，能力目录会返回
`supportCellAmbiguous` 并保持不可用。运行方案现在持久化用户明确选择的支持格；缺少支持格或支持格
漂移时目录、复验和激活均保持保守状态，不会把某个已验收格的状态扩展到同一主机上的其他引擎设备。
即使宿主同时报告 CPU/Metal，Ollama 也必须通过模型驻留观察证明实际执行设备；本轮真实验收曾因
Metal 标签复用仍驻留的 CPU runner 被拒绝，显式卸载后才在 `size_vram>0` 的 Metal runner 上通过。

## 3. 当前 Agent 能力边界

Agent 可读取脱敏系统、运行时、诊断、外部 Agent、运维历史和短时健康状态，并可为以下操作
生成一次性计划：

- 模型启动或安全切换、停止当前托管模型、模型移除、公开 GGUF 搜索与下载；
- 读取托管与外部已保存运行方案，并为精确`profileId`请求Rust实时复验后生成安全切换计划；
- 固定 llama.cpp 安装与移除、确定性诊断修复；
- 四类外部 Agent 的配置与断开；
- Pi Coding Agent 的 HAL100 私有安装与私有运行时移除。

所有计划只在 Rust 内存中短期存在，必须经过原生确认、现实状态复验和确定性执行。Agent
没有任意 Shell、任意文件读写、外部模型源文件删除、失败下载分片清理、强制切换、桌面
自动化或 root 权限。

当前或最近一项可信任务以schema v3脱敏检查点展示阶段、序列、任务/目标类别、成功谓词、证据
来源、三态结论、观察/重规划计数，以及澄清种类、尝试数和过期时间。检查点不保存提示词、回答、
具体资源、计划/run ID、路径、凭据或原始工具结果，也不写SQLite。固定澄清最多2次且5分钟
失效；仍有效的待确认计划可以在同一进程内继续；动作复验不满足时，同一精确任务最多允许1次
非终态重规划。冲突、证据不可用、超过上限、取消、过期、新任务、失败或应用重启均故障关闭或
撤销恢复权限。

只读任务必须由Rust类型化工具结果满足谓词才能完成。确认动作执行后，Rust分别重新读取模型
活动态、模型库、引擎安装态、运行方案活动路由与外部引擎实时身份、外部Agent配置态、HAL100私有安装态或修复后诊断；执行器摘要、
模型回答、Sidecar和前端都不能声明成功。Agent专属证据检查点与审计只保留枚举化来源、结论、
计数和动作类别，不持久化计划授权、运行ID、具体目标或原始证据。

RPC v13效率指标只保留数值，不保存提示词、回答或原始工具结果。`reported*`字段来自Provider
精确Usage；工具结果Token估算只用于同场景上下文去重比较，不进入用量统计或计费口径。

复合图检查点只保存最多8个节点、每节点最多4个依赖的序号、任务/目标类别、成功谓词、枚举状态与证据摘要；
不保存具体资源或任何计划/确认权限。非终态图以不超过16KiB的0600原子JSON保存；协议和文件均
拒绝未知字段，成功或已补偿终态删除恢复文件。重启后只展示可恢复语义形状，用户必须重新选择
精确模型与软件；Rust校验3/4节点形状后把全部节点退回`ready/blocked`现实重验证，并清空旧
证据、变更标记和重新授权标记，不能复用旧成功、旧计划或旧确认。Agent页面已经提供复合图创建、
节点预览、逐节点继续、取消、显式安全补偿和重新绑定恢复；不会自动批量执行或自动补偿，所有
正向与逆向写节点都逐项生成新的一次性计划并使用原生确认。补偿候选只来自执行前明确未满足、
确定性执行成功且执行后现实复验满足的HAL100所有状态；模型文字、进程退出码、未知前态和幂等
已满足都不能产生补偿权限。

## 4. 当前验证基线

- `pnpm check`覆盖 Biome、TypeScript、Desktop 26 项测试、Agent Kernel 34 项测试、Rust
  fmt、Clippy和默认 Rust workspace 测试。
- 最近的多引擎目标回归中，`hal100-infra`执行233项库测试（226通过、7项显式忽略），
  `hal100-protocol`执行39项并全部通过，`hal100-platform`执行17项并全部通过；协议/平台代码已对
  Linux x86_64与Windows x86_64目标交叉检查。联网目录、真实模型、官方第三方CLI、规模、性能、
  vLLM Linux/CUDA真机资格和开发沙箱探针仍需显式运行，不包含在日常快速绿灯中。
- 迭代38的16K真实Agent验收达到9/9；Apple M1/16 GiB上模型运行时物理占用峰值556.2 MiB，
  冷启动取消8毫秒、推理中取消71毫秒，空闲回收通过。
- 迭代43的32K候选在同一Apple M1/16 GiB上完成27,725 Token真实输入：无截断、答案准确，
  总耗时73.12秒、提示吞吐380.09 Token/s、物理占用峰值566.3 MiB，停止后进程与端口回收。
  设备选择边界矩阵7/7覆盖未知内存、16 GiB阈值和高内存仍封顶32K；这些是策略测试，不是多机
  性能实测。32K连续任务20/20，总耗时213.655秒、最慢单轮17.373秒、最大2执行回合、重复工具
  结果0 Token，显式停机后活动任务与子运行时均为0。Rust现只开放16K/32K两个合同档，不从
  本机证据外推64K。
- 迭代38有界澄清合同达到10/10；真实Qwen/Pi选择OpenCode后只生成1项精确配置计划并停在
  `awaitingConfirmation`，纵向验收63.10秒通过。
- 迭代39建立的开放评测扩展后，确定性层为33/43、越权任务0并覆盖20/20任务；真实Pi开放子集24/24、安全4/4，p95为
  2260毫秒，全程零工具。
- v9动作纵向矩阵扩展后达到20/20：覆盖12类受控任务、11种原生动作、四个外部Agent的配置与
  断开、3类诊断修复执行器、8类关键失败证据；合同路径最终状态证据率为100%。
- 迭代41按v9的18条动作路径把最小模型回合总数从55降到37（-32.7%）；三工具隔离对照的
  重复工具结果Token从非零降到0，发送工具结果Token不超过旧值60%。真实本地Qwen模型计划以
  16384窗口在2个执行回合完成：纠偏0、重复工具结果0、Provider输入1375/输出391、峰值输入
  793 Token。
- 迭代42真实Pi复合图前两节点探针在不执行任何写操作的前提下推进至模型节点待确认；加入Rust
  现实状态预检后，模型回合从9降至3（-66.7%），引擎已满足节点为0模型回合，墙钟时间从
  125.65秒降至39.05秒（-68.9%）。隔离OpenCode纵向又证明正向配置与逆向断开均使用独立计划、
  独立确认和动作后现实复验，并最终进入`compensated`。v11恢复纵向验证0600/16KiB脱敏文件、
  未知字段与错形拒绝、重新选择精确目标、全节点失权重验证及终态清理。最终真实Pi隔离整图中，
  引擎/模型幂等节点均为0回合，OpenCode配置节点2回合、重复工具结果0；伪造计划拒绝，精确计划
  经确认路径执行并由`IntegrationRecheck`完成，整图最终为`succeeded`。
- 迭代43新增`stop_model`确定性纵向：Pi只能在运行目录后复制当前活动`modelId`，Rust在计划与
  执行前双重复核，原生确认后调用现有托管停止器，并以`RuntimeRecheck`要求运行时为`Stopped`
  且活动模型为空。隔离真实Pi 32K验收用2个模型回合完成，伪造计划拒绝，模型文件与数据库索引
  均保留。
- 迭代44完成全局界面层级与排版收口：标题先于模型/活动子导航，主要业务页统一内容宽度、卡片
  表面、按钮和响应式间距；Agent复合任务、状态、单任务输入与结果重新建立主次关系。Playwright
  已在880 px最小窗口逐页验证7个现行入口无横向溢出，并在1200/1440 px覆盖亮色、深色和Agent
  单/双栏状态，控制台错误为0。
- 迭代45继续收口首页与Agent关键任务层级：首页把重复的状态与推荐动作合并为单一卡片，修复
  状态标签漂移；Agent在1100 px以上保持输入/结果并视，闲置复合任务下移，活动或可恢复任务
  才提升，运行主动作先于推荐模板，空结果在工作台滚动时保持可见。Playwright已按用户截图的
  1159 × 846视口、880 × 700最小视口和深色主题复核，均无横向溢出或控制台错误。
- 迭代46新增个人运行方案：schema v10保存无凭据的版本化验证快照，Rust在切换前复验模型、引擎
  与容量策略并在失败时尝试恢复原模型；RPC v13让Pi从脱敏目录选择精确方案，只能生成一次性
  原生确认计划。运行方案页在1159 × 846、880 × 846和深色主题完成回归，横向溢出与控制台错误均为0。
- 迭代47建立跨平台多引擎的首条架构纵向：协议层把引擎、Gateway协议、所有权、目标平台、
  加速器和模型格式拆为独立白名单；Infra增加对象安全的`InferenceEngineAdapter`并由现有
  `LlamaCppManager`实现。运行方案不再依赖管理器中的`llama.cpp`常量，schema v11保留旧方案
  并允许固定未来引擎身份；尚无适配器的方案明确判定不可用，不会被当前引擎误激活。
- 迭代48把原`MacOsSystemProbe`收敛为平台中立的`NativeSystemProbe`，按需产生
  `HostCapabilitySnapshot`，分别记录平台、aarch64/x86_64架构、CPU/内存/存储和已验证加速器。
  引擎描述符新增CPU架构要求并与宿主能力做平台、架构、加速器三重交集；桌面能力目录和运行
  方案共享该结论，不兼容方案故障关闭。当前探测实现仍只验证Apple Silicon/Metal，Windows/
  Linux只是可表达的目标而不是已完成运行支持，且没有新增后台采样器。
- 迭代49新增与托管生命周期完全分离的`ExternalInferenceEngineAdapter`。首个Ollama实现只访问
  固定`127.0.0.1:11434`的官方版本与模型目录端点，禁用代理、限制超时与响应大小，并严格校验
  模型名称和64位十六进制摘要；无效目录只保留引擎身份并标记目录不完整。桌面能力目录始终列出
  外部适配器描述符，仅在服务可达时附加运行快照；已保存、启用且API根地址完全匹配的外部后端
  才能产生无凭据的只读运行方案候选。本轮没有安装、启停、模型拉取、删除、SQLite迁移、Agent
  工具或发行流程。
- 迭代50把候选接入Rust拥有的外部运行方案闭环：Gateway以原子`backend + resolved model`
  路由解释`hal100-active`并在SQLite schema v12中完整持久化；托管会话保存和恢复完整旧路由。
  共享外部引擎注册表为桌面与方案管理器批量复用一次实时探测；保存、预检、确认后执行和执行后
  证据均复核后端/API根、版本、模型与digest，任一漂移故障关闭，切换或写库失败恢复旧路由与
  托管状态。桌面可保存多个Ollama方案并经原生确认显式重验漂移快照；Pi只看到脱敏所有权、
  后端ID和可选容量，只能请求Rust实时预检并生成一次性原生确认计划，不拥有重验、安装、拉取、
  启停或路由写入权限。
- 迭代51A—51D完成静态manifest、目标感知多实例观察、spec v3/schema v13迁移、四类类型化证据、
  精确适配器/配置/origin绑定、激活authority漂移检查，以及SQLite单飞journal、CAS阶段和启动
  补偿。vLLM作为第二引擎接入固定只读合同但没有提前提升支持等级。
- 迭代52平台基线把宿主探针按cfg拆为macOS、Windows与Linux实现，并对Linux/Windows目标完成
  交叉检查。Linux CUDA候选要求NVIDIA驱动版本与DRM PCI厂商双证据；这只证明宿主候选，不替代
  具体引擎的真实推理资格。
- 迭代53保留验收债务：vLLM模型使用`catalogIdentity`而非伪内容digest，已保存后端凭据通过不可序列化
Rust目标注入检查请求，多个后端实例独立观察。显式资格探针验证Models、Chat unary、SSE、
精确Usage和单工具调用，并产生稳定能力指纹；真实Linux/CUDA验收尚未执行，因此manifest仍为
`connected`且界面不会生成可保存候选。
- 迭代54已完成软件与真机闭环：MLX-LM适配器固定`127.0.0.1`官方`/health`、`/v1/models`和
`/v1/chat/completions`，发现阶段明确标记版本不完整，资格阶段从官方`system_fingerprint`提取
精确版本；共享OpenAI资格验证器检查单工具调用、流式结束、Usage和指纹一致性。Apple M1真实
验收使用MLX-LM 0.31.3与Qwen3-0.6B-4bit，保存/激活/动作后复验/活动方案验证全部通过；Qwen2.5-
0.5B因工具调用能力不足被正确拒绝。通用OpenAI后端现可显式保存`engine + adapterVariant`，不再
  靠旧`BackendKind`推断新引擎身份。
- 迭代60的运行资格报告已将设备依据类型化为模型驻留实测、固定适配器变体合同或未解析；授权层不再
  用“当前只有一个正式格”推断运行设备。Ollama保持驻留实测，单设备变体显式声明合同；MLC LLM已
  完成Metal/Vulkan/CUDA/ROCm四变体拆分，未来每个支持格可独立验收且不会跨设备复用资格。
- 运行方案跨引擎故障现由Protocol固定为`code/stage/retryable/recoveryAction`，由Rust管理器统一从
  持久化、引擎、后端、资格、证据、支持格、激活与恢复错误映射。Pi工具直接复用稳定安全code，
  Desktop IPC返回同一结构且界面按code显示固定说明；端点、模型身份、上游响应和底层错误正文不会
  因失败路径进入Pi或WebView。
- 跨引擎控制面回归现以Ollama CPU与MLC LLM Metal两个隔离适配器证明：从已活动MLC路由切换到
  Ollama后若动作后证据漂移，Gateway与SQLite都恢复精确MLC路由，journal清空，适配器检查不串线，
  原方案仍活动且失败方案不标活。该夹具不构成MLC真机或正式支持证据。
- `pnpm build`覆盖 Desktop生产前端、Agent Kernel Sidecar和Rust workspace开发构建。
- 已记录的1小时空闲基线为平均CPU 0.0043%、最大0.3%、物理内存约42 MiB；该结果只对应
  记录中的Apple M1测试条件。

## 5. 维护规则

以下事实发生变化时，必须在同一变更中先更新本文，再同步相关专题文档和测试：

- 当前版本、开发阶段、平台或发布范围；
- 已完成迭代和下一迭代状态；
- SQLite schema、Agent RPC版本或工具数量；
- 一级导航、专用客户端集合或关键能力边界；
- 默认质量闸门和重型验收范围。

`pnpm docs:check`会从代码和配置中读取版本、SQLite schema、Agent RPC、工具数量、Pi依赖
版本及界面阶段，并与本文和路线图交叉校验；该命令已包含在`pnpm check`中。

文档职责分工：

- 本文：当前实现与范围快照；
- `PRODUCT.md`与`HAL100_SOFTWARE_PRODUCT_DOCUMENT.md`：稳定产品定义和验收要求；
- `ROADMAP.md`：迭代历史和当前规划状态；
- `ARCHITECTURE.md`、`SECURITY.md`、`PERFORMANCE.md`：现行技术原则与专题事实；
- `adr/`与`benchmarks/`：不可追溯改写的历史决策和验收证据。
