# HAL100 推理引擎正式支持计划

- 规划日期：2026-08-28
- 产品基线：1.0.6，迭代0—59已完成，迭代60进行中
- 规划范围：迭代51—60
- 当前状态：实施中；迭代51—59的软件合同、平台探针、适配器和运行方案基础均已接入，MLX-LM Apple Silicon支持单元已完成真实纵向；共同控制面已覆盖类型化错误与跨引擎失败恢复夹具，vLLM、MLC LLM、OpenVINO Model Server、SGLang、LMDeploy与TensorRT-LLM仍按支持单元保留真实验收、性能稳定性和双真实服务回滚债务，当前集中在迭代60收口

模块、接口、schema、激活事务、三平台探测与测试的详细设计见
[多推理引擎架构蓝图](INFERENCE_ENGINE_ARCHITECTURE_BLUEPRINT.md)。

## 1. 目标

把协议层预留的MLX-LM、vLLM、SGLang、TensorRT-LLM、OpenVINO、MLC LLM和LMDeploy逐步
升级为HAL100正式支持的推理引擎，同时保持以下边界：

- 正式支持按“引擎 × 平台 × 架构 × 加速器 × 部署形态”逐格验收，不以一个枚举值代替全部组合。
- 外部服务默认仍由用户拥有；HAL100只获得连接、只读检查、路由和运行方案复验权。
- 只有HAL100明确拥有固定运行时和依赖闭包时，才能声明托管生命周期。
- Pi只能读取Rust生成的脱敏目录并申请受控计划，不能选择任意URL、命令、路径或扩大权限。
- 当前仍只维护从源码启动的开发版；不引入签名、公证、安装包、自动更新、正式升级或发布流程。

## 2. “正式支持”的定义

引擎状态使用统一的四级模型：

| 状态 | 含义 | 是否算正式支持 |
| --- | --- | --- |
| `reserved` | 只有受控引擎身份或路线预留 | 否 |
| `connected` | 可作为通用OpenAI/Anthropic后端发送请求，但无法证明具体引擎身份 | 否 |
| `verifiedExternal` | Rust可从用户已保存的后端实时证明引擎、服务版本、模型身份和协议能力，并完成运行方案复验/回滚 | 是 |
| `managed` | HAL100还拥有固定供应链、启动/停止、状态、容量、恢复和移除边界 | 是 |

“支持某引擎”必须同时给出支持单元，例如“vLLM / Linux x86_64 / CUDA / 外部服务”。一个单元
通过不自动扩展到其他操作系统、CPU架构、加速器、引擎后端或社区插件。

### 2.1 单个支持单元的最低验收条件

1. 引擎官方文档明确支持目标平台、硬件和服务接口。
2. Rust适配器具有稳定ID、版本化合同和固定端点策略；不接受Pi或WebView提供的任意探测URL。
3. 能区分“服务可达”“引擎身份已证实”“模型存在”“模型内容或部署身份证据强度”。
4. Gateway通过Models、Chat Completions、流式输出、工具调用、Usage、取消和错误映射合同。
5. 运行方案可保存、预检、一次性确认、原子切换、切换后复验、漂移故障关闭和失败恢复。
6. Desktop显示真实支持范围和证据强度；Pi只看到脱敏身份、兼容性、容量与准备度。
7. 有伪服务、超大响应、重定向、超时、模型替换、版本漂移、凭据缺失和并发变更测试。
8. 至少一组目标平台真实服务与真实推理纵向验收通过，并记录引擎版本、硬件和模型修订。
9. `pnpm check`、生产构建、文档一致性和差异检查通过。

## 3. 统一架构升级

### 3.1 目标感知的外部适配器

当前`ExternalInferenceEngineAdapter::inspect()`与`qualify()`均使用目标感知合同。正式多引擎支持
的目标只能来自Rust读取并验证的已保存后端：

```text
Saved backend + Keychain credential
             ↓ Rust validates origin/policy
VerifiedExternalEngineTarget
             ↓ engine-specific adapter
ExternalEngineSnapshotV2 + VerificationEvidence
```

- `VerifiedExternalEngineTarget`包含内部后端引用、规范origin、部署位置、认证引用和端点策略；不进入Pi。
- 适配器注册键升级为`engine kind + adapter variant + contract revision`，允许同一引擎存在官方服务、
  社区插件或不同服务形态而不混淆身份。
- 本机服务只允许显式回环地址；远端服务默认要求HTTPS。禁止userinfo、查询、fragment、跨origin
  重定向和隐式系统代理；凭据只由Rust在请求阶段解析。
- 每个适配器只访问官方文档允许的健康、版本、模型目录和只读元数据端点，统一限制超时、响应大小、
  模型数量和字段长度。

### 3.2 证据强度而不是伪造统一digest

Ollama提供模型digest，但其他服务不一定公开权重摘要。运行方案spec v3/schema v15已把当前
Ollama专用字段泛化为类型化证据，而不是给所有引擎虚构`model_digest`：

| 证据类型 | 可证明内容 | UI表达 |
| --- | --- | --- |
| `contentDigest` | 服务公开的内容摘要与模型名同时匹配 | 内容身份已验证 |
| `repositoryRevision` | 官方模型仓库与不可变修订匹配 | 模型修订已验证 |
| `deploymentFingerprint` | 引擎版本、规范配置元数据和模型部署标识匹配 | 部署身份已验证 |
| `catalogIdentity` | 只能确认当前服务仍公开同名模型 | 服务目录已验证；权重由外部引擎负责 |

证据记录至少绑定适配器合同版本、具体服务实例ID、规范origin的不可逆指纹、配置修订、引擎版本、模型ID、证据类型/值和验证时间。
低强度证据不得在界面或Agent回答中升级为内容完整性。保存、计划、执行前、切换后和最终成功谓词
必须使用同一种证据重新验证。

迭代60当前使用版本化验收证据账本 `contracts/inference-engines/v4-acceptance-evidence.json`，并以
`contracts/inference-engines/v4-acceptance-evidence.schema.json` 固化账本的 JSON 结构合同；v1/v2/v3文件
仅保留历史可读性。账本记录
按精确适配器与“平台 × 架构 × 加速器 × 部署”支持格关联，并只允许存储脱敏的实例ID、origin指纹、配置修订、协议能力哈希、版本、模型修订、
主机摘要、类型化宿主证明、证据类型、仓库内来源和简短断言；Rust解析器会限制大小、字段长度、记录/证据数量，拒绝
URL、绝对路径、控制字符、重复证据和不完整的正式记录。当前账本已有 Ollama CPU、Ollama Metal
与 MLX-LM Metal 三条 macOS/aarch64 真实记录；它们保留为明确的`legacyHostSummaryV1`，不伪造
设备类别指纹，并由Rust固定迁移allowlist限制为原记录ID、变体、支持格和验收时间。其余25个支持格以及这三格未来的重新资格验证都必须由`NativeSystemProbe`生成
`nativeHostProbeV1`，绑定探针修订、精确支持格和不含序列号/路径的设备类别SHA-256。外部适配器的正式晋级门禁可通过 `validate_manifest` 要求每个正式外部支持格存在
匹配记录。HAL100 托管格则由自有 manifest、固定供应链和生命周期合同证明。

真实验收入口另可在显式设置 `HAL100_ACCEPTANCE_EVIDENCE_EMIT=1` 时输出单次验收运行产物，或在
同时设置 `HAL100_ACCEPTANCE_EVIDENCE_WRITE=1` 与明确的 `HAL100_ACCEPTANCE_EVIDENCE_OUT` 路径时
以 create-new 方式写出文件。产物合同位于
`contracts/inference-engines/v4-acceptance-run.schema.json`，只保存脱敏的支持格、实例ID、origin指纹、配置修订、协议能力哈希、版本/模型修订
摘要、主机摘要、原生探针修订、设备类别指纹、类型化模型证据指纹和短断言；部分运行允许缺少稳定性证据，因而只是待审查材料，不能直接替代正式账本记录，
也不能改变 `connected` 支持等级。审查完成后由 `append_run_as_formal` 在内存中校验七类证据并
原子追加；审查时必须把运行产物中的模型ID脱敏摘要替换为可复核的模型修订，不能直接把摘要当作
正式身份。任一步骤失败都不改变已有账本。输出路径不会自动创建父目录，也不会覆盖已有文件。

注册表提供显式的 `new_with_reviewed_acceptance_evidence` 组合入口：它读取审查后的账本，为每个
精确匹配的“引擎 × 平台 × 架构 × 加速器 × 部署”支持格生成只存在于内存的正式状态投影，并继续
委托原适配器执行网络探测与资格请求。没有账本记录的格不会被提升，已有记录与manifest不一致时
组合失败；账本记录还必须匹配适配器的完整变体/合同协议能力哈希，未知适配器或未声明固定协议合同
的变体也会失败关闭。因此晋级不依赖手工修改适配器状态，也不会把测试包装器或连接状态写回生产合同。真机
验收阶段可用同一运行方案门禁验证闭环，审查导入后再通过该入口启用对应正式格。

审查导入由 `hal100-engine-acceptance-import` 提供离线闭环：它读取有界、非符号链接的运行产物和
现有账本，要求维护者显式提供可复核的模型不可变修订（拒绝 `model-id-sha256:*` 关联摘要），只
允许 `passed` 产物形成 `verifiedExternal` 记录，并在写出新文件前用全部标准适配器重放支持格匹配
校验。输出使用 create-new 语义，不覆盖输入账本；若记录、适配器变体、平台或加速器不一致则不
产生输出。重新资格验证同一格时，维护者可用 `--replace-record-id` 显式命名旧记录；替换要求新旧
记录的适配器、平台、架构、加速器和部署完全一致，并继续写到新的候选文件。该工具不会启动引擎、
下载模型或把任何运行产物自动变成正式支持。
来源断言还必须是仓库相对路径，并且只能落在 `contracts/`、`crates/`、`docs/` 或仓库根部
`README.md`；`../`、反斜杠、双斜杠、URL 和绝对路径均会被拒绝。

为让三平台维护者在真实服务验收前看到可审计的缺口，Infra 提供只读
`hal100-engine-support-report`：它遍历标准适配器注册表，按“平台 × 架构 × 加速器 × 部署”列出
manifest状态、审查账本覆盖、有效状态、七类证据进度和逐格性能档案。报告schema v3只在origin/
配置、引擎身份、类型化模型证据、原生宿主证明和稳定性测量来自同一记录时投影
`reviewedPerformanceProfile`，并汇总`reviewedPerformanceProfiles`与
`formalExternalCellsMissingPerformanceProfile`；任一作用域字段缺失时整个档案保持`None`，绝不
解释为零延迟或零吞吐。`--ledger` 可检查候选账本，`--strict` 在仍有
非正式支持格或正式外部格缺少账本记录时返回非零状态；托管格由 HAL100 自有 manifest、供应链和
生命周期合同证明，不强制伪造一条外部服务账本记录。报告拒绝未映射或陈旧记录，不连接、启动、安装、
下载或激活任何引擎，不能替代真实服务和人工审查。严格 CLI 还从可执行适配器注册表取得每个变体
的 canonical 协议能力哈希并复核账本；哈希基线缺失或漂移时直接失败关闭，不能把候选账本误报为
可晋级。对于 v1 账本建立前的 MLX-LM 1 格和 Ollama 2 格，非 ignored CI 棘轮只允许无账本债务
集合缩小，禁止通过直接修改 manifest 增加新的无账本正式格。三格已于2026-08-31全部迁入真实记录，
历史无账本债务现为0；覆盖报告仍是4/29正式、25/29待验收，不能把债务清零误报为全矩阵完成。

八个 live acceptance 入口现在共享 Rust 有界稳定性探针：对同一已验证目标分五波发起 20 次
Chat Completions 请求，每波最多 4 个并发，只保留固定工作负载修订、尝试数、并发度、p95/最大延迟、
prompt/completion Token总量和总墙钟时间等聚合值；响应必须
包含非空 choices 与正的 prompt/completion usage，失败即拒绝生成稳定性证据。运行产物把这些值放在
`stability` 对象中，稳定性断言没有对应测量值时无法通过解析。该探针不启动、重启或控制外部服务，
也不把这一采样结果解释为绝对吞吐或跨平台性能结论。v3导入会把该对象原样保留到正式记录；新建
或复验的v3正式记录缺少它时故障关闭。三条早于v3的正式记录继续显式缺测量，并由固定历史allowlist
兼容，因此当前报告为0条已审查性能档案、3个正式外部格缺性能档案；推荐器不会据此猜测性能排序。

Ollama 的 OpenAI 兼容资格路径会发送类型化的 `reasoning_effort: "none"`，关闭思考模型默认的
内部推理，使固定输出预算用于验证工具调用、流式事件和 usage 合同。该选项只由 Ollama 适配器
提供给协议与稳定性探针，不增加 Token 预算、不降低工具调用标准，也不向其他引擎传播专属语义。
资格完成后还读取官方 `/api/ps` 的已驻留模型：`size_vram=0` 精确证明 CPU；回环 macOS 上非零
设备驻留精确证明 Metal。`EngineQualificationReport.runtimeDeviceEvidence`明确区分
`modelResidencyObservation`、`adapterVariantContract`和`unresolved`。授权层不再根据“当前只有一个
正式格”替适配器推断设备：驻留观察必须与选择一致；固定变体合同还必须由descriptor与全部支持格
共同证明该变体只有一种加速器；未解析报告始终失败关闭。真实依据
与精确测量记录在 `docs/benchmarks/2026-08-31-ollama-0.33.2-macos-metal.md` 和
`docs/benchmarks/2026-08-31-ollama-0.33.2-macos-cpu.md`。

运行产物现在还可携带可脱敏的 `resilience` 对象，结构化记录共享控制面是否通过取消、失败切换
回滚和重启补偿三项检查。该对象允许在普通 live probe 中缺省或标记未通过，因此不会把部分运行
误当作正式支持；但 `verifiedExternal`/`managed` 的正式记录必须同时包含三个 `*Verified: true`
字段，导入门禁会拒绝缺失或任一失败的韧性证据。韧性检查只证明 HAL100 Gateway 与运行方案事务
边界的安全行为，不替代目标引擎的真实服务、模型修订、平台和加速器验收；取消、切换失败和重启
补偿的纵向探针仍需在原生三平台验收阶段执行并由人工审查其来源。

所有 live acceptance 入口还必须在发送任何目录、资格或推理请求之前，通过
`hal100-platform::NativeSystemProbe`读取运行测试的真实主机快照，并校验平台、架构和所声明的
加速器；测试不再允许用合成的CPU/GPU/内存字段生成平台运行时证据。支持格选择与原生证明由一个
共同preflight完成，错误平台或加速器会在触碰真实模型前故障关闭。
OpenVINO Model Server 入口要求显式设置
`HAL100_OPENVINO_ACCELERATOR=cpu|intel_gpu|intel_npu`。OVMS官方HTTP元数据、模型目录、配置状态
和metrics均不暴露实际`target_device`，因此实现拆分为CPU、Intel GPU、Intel NPU三个单设备适配器
变体；运行方案绑定精确变体，原生探针确认宿主硬件，审查后的真机运行记录确认该部署合同。资格报告
标记`adapterVariantContract`而不是服务观察，不把环境变量或宿主拥有设备伪装成服务主动回报。

为避免各平台手工运行方式漂移，仓库提供 `scripts/run-engine-live-acceptance.sh`（macOS/Linux）和
`scripts/run-engine-live-acceptance.ps1`（Windows）白名单入口。
它只映射到八个固定的 ignored 测试（含 Ollama），要求操作者设置 `HAL100_RUN_REAL_ACCEPTANCE=1`，并以
create-new 方式输出脱敏运行产物；服务启动、模型准备、版本/部署指纹确认和账本导入仍由人工
分阶段完成，脚本不会执行安装、下载、停止、重配置或自动晋级。Ollama 入口已按 manifest 覆盖
macOS/aarch64 的 CPU、Metal 以及 Windows/Linux x86_64 CPU 四个支持格，要求显式
`HAL100_OLLAMA_ACCELERATOR=cpu|metal`；选中的平台、架构和加速器必须同时由原生探针与引擎运行时
驻留观察证明。宿主拥有某种加速器不再等价于目标模型正在该加速器上执行。
`.github/workflows/live-engine-acceptance.yml`在这些脚本之上提供仅手动触发的原生执行编排：静态绑定
带`hal100-acceptance`标签的隔离自托管macOS ARM64、Linux x64/ARM64和Windows x64 runner，按目标
平台串行执行，成功时只上传短期create-new脱敏产物。它不使用普通托管runner冒充硬件，不准备或
重配置服务，不自动导入账本，也不改变支持状态；输入的平台/加速器不在manifest时仍由Rust共同
preflight在首个服务请求前故障关闭。
工作流公开dispatch参数只允许选择引擎、平台和加速器；回环API root、模型ID（MLC时为绝对本地
部署目录）、审查版本及可选vLLM密钥由名称绑定该精确坐标的受保护GitHub Environment secrets
注入。配置在checkout前只做非空检查且不打印值，避免主机路径或模型身份进入workflow事件；缺少
保护配置时不会执行仓库代码或触碰服务。
在任何自托管job之前，普通Ubuntu上的无秘密`validate-coordinate` job读取版本化支持矩阵，把引擎、
平台/架构和加速器选择解析为唯一`adapterVariant + contractRevision + supportUnit`。不存在、重复或
非本地的组合立即拒绝，四类真机job全部以`needs`依赖该结果；这一阶段不读取目标配置、不连接服务，
也不生成验收证据。文档门禁逐格反向验证全部28个外部支持格都能唯一解析，并固定拒绝Windows/vLLM
这类矩阵外组合。
操作者不维护第二份手写坐标表：`scripts/list-engine-acceptance-targets.mjs`从同一矩阵生成待验收
Environment清单，包含runner坐标、完整适配器身份、状态和所需secret名称而不包含值；主机隔离、
保护配置、运行、人工审查和清理步骤见[真机验收runner手册](INFERENCE_ENGINE_ACCEPTANCE_RUNNERS.md)。
八个入口现统一通过同一个 manifest 驱动的本地支持格选择器解析编译平台、架构和加速器；当前版本化矩阵为8个外部引擎、13个外部适配器变体、28个外部支持格；合并托管llama.cpp后为14个适配器、29个支持格，其中4个正式、25个待验收。非
ignored 覆盖回归逐一验证全部外部支持格都能抵达对应真实验收入口，并拒绝
未知或未声明坐标。该回归只证明入口可达性，真实服务与原生加速器证据仍必须在目标主机取得。
同一覆盖套件还为全部 28 格生成只存在于测试内存的完整结构夹具，逐格穿过运行产物校验、人工提供
模型不可变修订、原子账本追加、canonical 协议能力哈希复核、审查注册表投影和严格覆盖报告；合并
托管 llama.cpp 后可证明候选管线能够达到 29/29。该测试只排除某个支持格在真实验收后无法导入或
晋级的结构断点，不会生成、写入或替代任何真实平台记录。

### 3.3 协议能力合同

“OpenAI兼容”不能作为单个布尔值。每个适配器按实时探测和版本化验收矩阵声明：

- Models、Chat Completions、Completions、Responses、Embeddings；
- 流式SSE、Usage、工具调用、结构化输出、多模态；
- 最大上下文、最大输出、并发、批处理和取消语义；
- 认证方式、API根和模型名重写规则。

Gateway只为已验收能力开放路由。上下文容量未知时保持空值，Rust和Pi都不得猜测。

### 3.4 平台与真机证据

- Windows和Linux先实现可从源码运行的`NativeSystemProbe`、CPU/架构/加速器探测和CI构建；这不
  制作安装包。
- GPU能力只来自原生API或官方运行时的受控只读探测，不能从环境变量、进程名或营销支持表推断。
  当前 Linux 探针从有界 DRM 厂商/render node、NVIDIA 驱动、AMD `/dev/kfd`和Intel accel class
  证据生成 CUDA、Vulkan、ROCm、Intel GPU、Intel NPU候选；Windows 探针从固定 CIM 显卡与
  ComputeAccelerator PCI厂商字段生成对应候选。候选
  仍不等价于对应运行时或引擎资格，缺少资格时必须故障关闭。
- 真机矩阵按平台单独保存证据；缺少目标硬件时该支持单元保持`planned`或`experimental`。

### 3.5 Desktop与Agent

- 能力目录显示“已连接”“已验证外部”“HAL100托管”以及精确的平台/加速器支持格。
- 运行方案显示证据类型、最后验证时间、容量来源和漂移原因，不直接展示秘密或内部摘要值。
- Rust根据兼容性、现实可用性、已验证方案、容量和任务协议需求生成确定性推荐；Pi可以解释，
  但不能覆盖选择门禁或自动安装引擎。
- 当宿主同时具备多个加速器，而匹配支持格的状态不一致时，Rust 返回
  `supportCellAmbiguous` 并故障关闭保存、激活和推荐；不能用“最高支持等级”替代对具体加速器的
  选择。后续要开放该场景，必须把用户选择的加速器写入运行方案绑定并在每次复验时重新证明。
- 一个任务只激活一个已保存方案；不做模型输出驱动的静默后端回退。

## 4. 官方能力基线与产品定位

| 引擎 | 首个正式支持单元 | 支持形态 | 规划约束 |
| --- | --- | --- | --- |
| vLLM | Linux x86_64/aarch64；先CUDA，再ROCm/XPU/CPU逐格验收 | `verifiedExternal` | 官方OpenAI服务和版本/健康端点适合做第一条通用服务适配器；Windows原生不纳入首批，vLLM-Metal作为独立社区变体 |
| MLX-LM | macOS Apple Silicon / Metal | 优先`managed`，或满足强身份后`verifiedExternal` | 官方服务仅定位本机且安全检查基础；必须固定回环，不能作为远端服务默认开放 |
| MLC LLM | Apple Silicon macOS/Metal、Windows/Vulkan、Linux/Vulkan或CUDA分别验收 | `verifiedExternal`或固定本机运行时 | 官方REST服务跨平台，但MLC编译产物与普通HF权重身份不同，必须保存MLC部署指纹；HAL100产品边界明确不支持Intel Mac，因此manifest和正式验收矩阵不声明macOS x86_64支持格 |
| OpenVINO | Windows/Linux x86_64 Intel CPU/GPU/NPU；以OVMS为首个变体 | `verifiedExternal` | 使用KServe健康/元数据与OpenAI GenAI端点；不同设备插件分别验收，不能笼统写成“Intel均支持” |
| SGLang | Linux x86_64 / NVIDIA CUDA；AMD、XPU、CPU后续逐格扩展 | `verifiedExternal` | 官方支持多种硬件，但首条纵向以Linux/CUDA稳定组合收敛 |
| LMDeploy | Linux/Windows x86_64 / NVIDIA CUDA；ROCm另行验收 | `verifiedExternal`或受控本机启动器 | 先证明官方服务身份/版本端点；若只能看到通用模型目录，不授予`verifiedExternal` |
| TensorRT-LLM | Linux x86_64/aarch64 / NVIDIA支持矩阵GPU | `verifiedExternal` | 仅支持官方列出的Linux与GPU架构；使用`trtllm-serve`版本、健康、模型和OpenAI端点 |

官方资料入口：

- [MLX-LM HTTP Model Server](https://github.com/ml-explore/mlx-lm/blob/main/mlx_lm/SERVER.md)
- [vLLM硬件安装矩阵](https://docs.vllm.ai/en/stable/getting_started/installation/gpu/)与
  [OpenAI兼容服务](https://docs.vllm.ai/en/latest/serving/online_serving/openai_compatible_server/)
- [SGLang Quickstart](https://github.com/sgl-project/sglang/blob/main/docs/docs/get-started/quickstart.mdx)
- [TensorRT-LLM支持矩阵](https://nvidia.github.io/TensorRT-LLM/reference/support-matrix.html)与
  [`trtllm-serve`](https://nvidia.github.io/TensorRT-LLM/commands/trtllm-serve/trtllm-serve.html)
- [OpenVINO Model Server](https://docs.openvino.ai/2026/model-server/ovms_what_is_openvino_model_server.html)
- [MLC LLM平台矩阵](https://github.com/mlc-ai/mlc-llm/blob/main/README.md)与
  [REST API](https://llm.mlc.ai/docs/deploy/rest.html)
- [LMDeploy安装矩阵](https://lmdeploy.readthedocs.io/en/stable/get_started/installation.html)与
  [OpenAI服务Quickstart](https://lmdeploy.readthedocs.io/en/latest/get_started/get_started.html)

这些链接是规划时的官方事实基线；真正实现时必须固定并记录当时验收的版本，不能追随`latest`
标签自动改变支持结论。

## 5. 迭代路线

### 迭代51：正式支持合同与证据模型

- 引入四级支持状态和平台级支持单元。
- 建立目标感知、多实例、带适配器变体的外部引擎合同。
- 设计运行方案spec v3/schema v13和四类验证证据，迁移Ollama而不降低现有digest保证。
- 建立通用伪服务、恶意响应和官方服务纵向验收夹具。
- 输出每个引擎的端点/身份/API差距报告；没有强身份的引擎不得提前标为正式支持。

完成条件：Ollama在新合同上保持现有闭环；vLLM最小假服务证明第二种引擎可复用合同；旧方案向前
迁移且漂移/回滚测试通过。迭代51只建设合同，不把预留引擎改成“已支持”。

迭代51内部按“无行为重构 → 目标/观察服务 → spec v3/schema v13 → vLLM合同试桩”四个子阶段
推进，避免同时重写适配器、数据库、路由和UI。详见架构蓝图第19节。

### 迭代52：Windows/Linux源码运行与能力探测基线

- 让Core、Protocol、Infra和开发版Desktop在Windows/Linux CI编译并运行无平台秘密依赖的测试。
- 实现Windows/Linux CPU、内存、存储、架构和受控加速器探测；未知能力故障关闭。
- 建立可重复的Linux服务容器验收和Windows本机服务验收入口，不制作安装包。
- 已加入 `.github/workflows/source-check.yml` 的 macOS 14、Ubuntu 24.04 和 Windows 源码级 `pnpm check`
  矩阵；该流程只验证源码和测试，不代表目标平台的具体引擎支持单元已经通过真机验收。
- 已加入 `.github/workflows/live-engine-acceptance.yml` 的四类隔离自托管原生runner手动编排；它只
  执行已准备服务的真实验收并上传脱敏产物，仍需人工审查、导入和精确支持格晋级。

完成条件：三平台从源码启动路径、能力快照和测试报告可复现；仍未验收的GPU不会显示为可用。

### 迭代53：vLLM正式外部支持

- 升级现有`externalVllm`连接为版本/健康/模型/能力明确的适配器。
- 首先验收Linux CUDA；ROCm、XPU和CPU以独立支持单元追加。
- 完成运行方案、Gateway协议、Usage、工具、流式、取消、漂移与回滚闭环。

### 迭代54：MLX-LM Apple Silicon正式支持

- 已完成Apple Silicon/Metal和回环服务；明确官方本机服务的基础安全限制。
- 官方发现端点不暴露版本时，发现快照标记版本不完整；资格请求从`system_fingerprint`提取精确 MLX-LM 版本，不伪造版本证据。
- 资格请求同时把官方运行指纹与模型标识绑定成`deploymentFingerprint`，方案保存/激活复验使用该
  指纹；这仍不声称已取得模型权重内容摘要。
- 已通过MLX-LM 0.31.3 + Qwen3-0.6B-4bit真实模型的目录、上下文请求、工具调用、流式、Usage、保存、激活、动作后复验和活动验证；Qwen2.5-0.5B工具能力不足拒绝用例通过。
- 后端协议入口现支持显式`engine + adapterVariant`，后续引擎无需继续扩张旧`BackendKind`。

### 迭代55：MLC LLM跨平台正式支持

- 已完成共同协议软件纵向，并把原先横跨四类设备的适配器拆为`official-openai-metal`、
  `official-openai-vulkan`、`official-openai-cuda`、`official-openai-rocm`四个单设备变体：固定官方
  `mlc_llm serve`回环目标，读取`/v1/models`，识别MLC模型格式，复用共享OpenAI资格探针验证Chat、
  流式、Usage与单工具调用。四个变体共同覆盖原有10格，不增加或删减平台承诺。
- MLC官方REST不提供稳定包版本端点，当前实现还固定输出空`system_fingerprint`，因此不能把该字段
  作为正式支持前提。适配器现要求正式资格使用绝对本地MLC部署目录；Rust在独立阻塞任务中有界
  校验路径边界、文件数、单份元数据和总字节数，哈希模型配置、权重清单、清单声明的全部分片与
  tokenizer文件，生成可重复复验的部署内容指纹；聊天模板必须恰好包含一个
  `{function_string}`插槽。相对路径、`HF://`、目录穿越、缺失文件、分片
  大小漂移和超限部署全部失败关闭；通过后运行方案仍以显式“版本未暴露”标记保存，不伪造包版本。
- 官方服务当前把非流式工具参数序列化为对象。Gateway只对Rust绑定为MLC的后端把它收敛为OpenAI
  要求的JSON字符串，通用OpenAI后端保持严格透传；带工具的流式请求在正式资格落地前直接拒绝。
  运行方案真实纵向除直接服务资格外，还要再次经过Gateway验证标准字符串形态。
- Apple Metal已使用隔离官方稳定wheel完成小模型JIT编译、`metal:0`加载、REST目录、Chat与Usage
  预检；Ministral-3-3B候选因不能稳定复制完整工具名被正式探针拒绝，未形成证据。随后固定
  Qwen3.5-2B MLC revision、全部权重对象、Metal动态库和内容指纹覆盖的HAL100工具模板：单次目录、
  标准工具调用、Gateway参数规范化、普通流式与Usage均通过，但官方0.20调度器在工具请求后的后续
  请求中可复现内部rollback检查失败，串行也会终止后台调度线程；当前0.26 nightly编译与运行包又
  存在接口/ABI漂移。正式稳定性和持续生命周期未通过，没有生成或导入证据，该格继续保持
  `connected`。完整固定输入与停止决定见
  [MLC LLM macOS Metal阻塞记录](benchmarks/2026-08-31-mlc-llm-macos-metal-blocked.md)。
  之后分别完成Windows Vulkan与Linux Vulkan/CUDA支持单元；运行方案绑定部署内容，
  不只绑定源HF模型名或可变路径。

### 迭代56：OpenVINO Model Server正式支持

- 已完成`ovms-openai-cpu`、`ovms-openai-intel-gpu`、`ovms-openai-intel-npu`三个单设备软件适配器：
  固定官方回环目标，使用KServe `/v2`服务器元数据与健康端点、OpenAI `/v1/models`和Chat
  Completions；共享资格探针覆盖流式、Usage和单工具调用。
- 模型证据使用`catalogIdentity`与`openVino`格式；服务器版本由官方元数据绑定，未把模型目录ID
  误当成OpenVINO IR内容摘要。
- 当前Windows/Linux x86_64的CPU、Intel GPU、Intel NPU六个支持单元保持`connected`；下一步分别
  取得版本、模型修订、目标设备合同和运行方案保存/激活/回滚证据。三类设备不能共用一个泛化
  `openvino`加速器值或跨变体复用证据。
- 待验收Windows/Linux Intel CPU、Intel GPU与Intel NPU；旧`ovms-openai-server`后端绑定在schema
  v15迁移中解除，旧含混支持格失效，必须由用户重新选择精确设备变体后复验。
- 把OpenVINO IR、GGUF导入和模型版本语义映射为独立证据类型。

### 迭代57：SGLang正式外部支持

- 已完成`official-openai-server`软件适配器：固定官方回环目标，使用`/server_info`读取版本、
  `/health`读取就绪状态、`/v1/models`读取模型目录，并复用共享Chat/流式/Usage/单工具调用资格探针。
- 当前Linux x86_64/CUDA支持单元保持`connected`；真实GPU服务、模型/权重修订、并发稳定性和
  运行方案闭环完成前不授予`verifiedExternal`。
- 待验收Linux/NVIDIA CUDA；AMD、Intel XPU和CPU作为单独支持单元。
- 固定服务器信息、模型目录、OpenAI能力和引擎参数摘要；不从启动命令反推现实状态。
- 加入高并发、前缀缓存、结构化输出和多模态能力的可选验收，不影响基础聊天闭环。

### 迭代58：LMDeploy正式外部支持

- 已完成官方`api_server`软件适配：固定`127.0.0.1:23333/v1/`回环目标，读取`/health`与
  `/v1/models`，复用共享Chat/流式/Usage/单工具调用资格探针；模型目录证据使用
  `catalogIdentity/lmdeploy-model-id`，格式记录为`safetensors`。
- 当前Linux/Windows x86_64/CUDA支持单元保持`connected`。官方服务合同未提供稳定的机器可读
  版本端点，TurboMind/PyTorch变体也不出现在模型目录中；资格响应中的非空`system_fingerprint`
  现在可与模型标识绑定为部署指纹，运行方案可用显式“版本未暴露”标记保存，但仍必须通过固定
  部署的受控本机验收补足身份，不能以显示名称冒充正式支持。
- 真实验收入口必须验证Linux/Windows CUDA服务、固定模型与共享OpenAI资格；ROCm、其他架构和
  Anthropic兼容端点作为独立支持单元，不从CUDA/OpenAI证据扩展。

### 迭代59：TensorRT-LLM正式外部支持

- 已完成共同软件纵向：只接受官方支持矩阵中的Linux x86_64/aarch64与NVIDIA GPU候选，固定
  `trtllm-serve` 回环目标，使用 `/version`、`/health`、`/v1/models` 及 OpenAI端点建立身份和
  协议证据，支持状态保持 `connected`。
- 新增显式 `trtllm-serve-openai-server` 适配器、Rust注册、桌面引擎绑定、固定发现目标和默认
  忽略的真实验收入口；模型目录证据使用 `catalogIdentity/tensorrt-llm-model-id`，格式记录为
  `safetensors`，不把目录ID冒充权重摘要。
- vLLM、OVMS、SGLang 和 TensorRT-LLM 的真机入口已统一复用保存/实时复验/激活/动作后验证辅助层；
  目标服务未准备时测试保持忽略，不能用夹具结果替代真实平台证据。
- HF checkpoint 与预构建 TensorRT engine 目录、PyTorch/TensorRT backend、TP/PP/EP并行配置和
  模型切换限制必须通过独立部署证据绑定；在Linux NVIDIA真机完成版本、GPU能力、模型修订、
  协议资格、并发稳定性及运行方案保存/激活/复验/回滚前，不授予 `verifiedExternal`。

### 迭代60：多引擎智能选择与收口

- 统一支持矩阵、运行方案、错误分类、性能/容量档案和跨引擎回滚。错误分类已经以Protocol
  `RuntimeProfileFailure`完成，Desktop与Pi共同消费稳定code、阶段、可重试性和恢复动作；其余项目
  继续按真实支持格验收推进。
- 已把七类正式支持证据进度接入统一能力目录：官方合同、协议资格、平台真机、引擎身份、模型
  部署身份、运行方案闭环和稳定性。`connected`只能声明前两类，正式支持单元必须七类完整，并且
  验收运行产物中的取消、失败切换回滚、重启补偿三项韧性检查必须全部通过。
- Rust按任务协议、模型身份、上下文、硬件、吞吐/延迟目标和用户已验证方案生成确定性推荐。
- Pi只解释推荐和生成受控计划；不自动安装、不静默切换、不把实验支持描述成正式支持。
- 在三平台完成代表性真实矩阵、长时稳定性和多引擎共存验收。

## 6. 优先级与停止规则

优先顺序是“安全身份与共同合同 → 平台真实能力 → 单引擎纵向 → 智能选择”，不得同时铺开七套
半成品适配器。每个引擎迭代开始前必须确认目标硬件和官方版本可获得；没有真机、官方端点或可
验证模型身份时，该支持单元保持规划状态，并转向同一引擎可被真实验收的其他单元，不降低完成
标准。

以下情况必须停止并重新决策：

- 只能通过任意Shell、读取用户进程参数或扫描文件系统才能识别服务；
- 官方API无法区分具体引擎且HAL100也不拥有固定运行时；
- 需要把远端HTTP、跨origin重定向或未受控凭据暴露给Pi/WebView；
- 某平台只能依赖非官方分支，却准备沿用官方引擎的“正式支持”标记；
- 为追求统一UI而隐藏模型身份或能力证据差异。
