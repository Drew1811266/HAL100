# 迭代60检查点：多引擎确定性推荐与正式支持证据进度

- 日期：2026-08-28
- 产品版本：1.0.5开发版
- 状态：共同收口已启动；三平台真实服务矩阵仍在进行

## 1. 已落地能力

统一能力目录现在由Rust同时返回当前宿主兼容性、正式支持状态、已观察外部实例、确定性推荐和
正式支持证据进度。推荐只把`managed`或`verifiedExternal`支持单元标记为可选；`connected`
实例即使已经被发现也只能提高可见性，不能获得运行方案保存或激活资格。

证据进度固定为七类：

1. 官方服务与平台合同；
2. 有界协议资格；
3. 目标平台真机服务；
4. 精确引擎身份；
5. 模型或部署身份；
6. 运行方案保存/激活/复验/回滚闭环；
7. 稳定性与并发证据。

当前`connected`适配器只记为完成前两类；`reserved`只记录官方合同；只有已经通过正式验收的
`verifiedExternal`或`managed`单元才显示七类完整。该摘要不保存凭据、URL、模型路径、启动命令
或原始测试输出，也不具备执行权限。

每个支持单元现在可以携带同一份类型化证据摘要。注册表要求`verifiedExternal`与`managed`
单元必须显式声明完整七类证据；缺失或不完整的正式manifest会被拒绝。`connected`与`reserved`
单元仍可省略该字段，并由Rust按状态投影保守进度。

资格报告现在可以额外返回`deploymentFingerprint`。MLX-LM把官方响应中的版本/运行指纹与模型
标识做确定性绑定，运行方案保存与激活复验会使用同一指纹；没有部署指纹的引擎仍只能使用目录
身份，不能借此晋级正式支持。

## 2. 产品呈现

运行方案页的引擎能力列表现在同时显示排序分数、支持状态和`已验证/总证据`，并列出尚缺的
平台真机、身份、运行方案闭环和稳定性项。这样用户和测试人员可以区分“软件适配器已接入”与
“当前平台正式支持”，Pi也只能解释Rust生成的同一份脱敏结论。

## 3. 安全门禁

- 证据进度从Rust manifest支持单元派生，不读取进程参数、任意文件或WebView输入。
- 正式状态与支持单元证据摘要由注册表做一致性校验，不能只改状态枚举绕过七类证据门禁。
- 部署指纹只能来自适配器的有界模型资格请求，并在方案复验时重新取得；它不是模型权重摘要。
- 观察到运行实例不改变支持等级；排序分数不改变`compatible`和运行方案门禁。
- 每次保存或激活外部方案仍必须执行目标、版本、模型证据和协议能力实时复验。
- MLC LLM与LMDeploy缺少稳定机器可读版本/部署指纹时继续保持`connected`，不会因协议通过而晋级。

## 4. 自动化证据

- `engine_support_evidence`单元测试验证`connected`只完成软件合同和协议资格，正式单元没有证据债务。
- vLLM、OpenVINO Model Server、SGLang 和 TensorRT-LLM 的显式真机入口现在共享同一套测试辅助层，
  在协议资格通过后继续执行运行方案保存、实时复验、激活、切换后复验和活动状态检查；这些测试仍
  默认忽略，只有在目标平台准备好固定服务时才会真实运行。
- 项目级`pnpm check`覆盖文档一致性、前端类型与测试、workspace Clippy和完整Rust回归。

## 5. 剩余工作

1. 在macOS、Windows和Linux代表性硬件上运行各引擎的显式真机验收入口并固定版本、模型修订和
   硬件证据。
2. 为各支持单元记录容量、首Token延迟、吞吐、并发、取消、崩溃恢复和长时稳定性档案。
3. 控制面夹具已完成不同引擎同时存在、跨引擎切换失败、旧路由精确恢复和既有重启补偿；仍需在
   两个真实服务同时存在的代表性主机上复跑同一纵向。
4. 只有对应支持单元七类证据完整后，才把该单元从`connected`晋级为`verifiedExternal`。

## 6. 本次暂停记录

- 记录时间：2026-08-27 19:41 CST。
- 已补齐外部方案重新验证的部署身份闭环：重新验证会复用资格请求返回的部署指纹，避免把
  MLX-LM 方案从`deploymentFingerprint`错误降级为目录身份。
- 针对部署指纹新增合法格式绑定与非法格式拒绝单元测试；`hal100-infra`单元测试结果为
  180通过、7忽略、0失败（共187项）。
- `cargo test -p hal100-infra --tests --no-run`与
  `cargo clippy -p hal100-infra --all-targets -- -D warnings`均通过。
- 本次`pnpm check`已由用户要求暂停：文档一致性、Biome、前端类型检查、桌面26项测试与
  Sidecar 34项测试已通过；随后进入 workspace Rust 检查阶段时被中止，因此项目级全量门禁
  仍需下次继续完成，不能标记为全绿。
- 暂停后继续开发的第一步：从 workspace Clippy/测试的中断点恢复，完成全量门禁，再补充
  MLX-LM 方案保存/重验证的集成回归；真实缺少运行时的引擎仍保持`connected`，不得晋级。

## 7. 恢复后进展

- 2026-08-28已从中断点恢复并完整通过`pnpm check`，覆盖文档一致性、Biome、TypeScript、
  Desktop 26项、Sidecar 34项、workspace Clippy、Agent Kernel构建与Rust workspace测试。
- 当前状态入口已校正为“迭代0—59完成、迭代60进行中”，README、产品文档和支持计划同步，
  文档检查输出不再停留在迭代55。
- 支持单元证据摘要已进入协议manifest；注册表新增正式状态缺少证据和证据不完整的拒绝回归。
  最新`hal100-infra`单元测试为184通过、7忽略、0失败（共191项），Infra全目标Clippy通过。
- 宿主兼容性结果现在携带实际命中支持单元的证据摘要，能力目录会优先使用该单元数据；不同
  平台/架构/加速器支持格不会再共享一个引擎级证据结论。

## 8. 本次暂停记录（2026-08-28）

- 当前仍处于迭代60“共同收口”阶段。支持等级没有被人为提升：正式支持单元仍为既有的
  llama.cpp（托管）、Ollama（macOS外部）和 MLX-LM（Apple Silicon/Metal外部）；vLLM、
  MLC LLM、OpenVINO Model Server、SGLang、LMDeploy 与 TensorRT-LLM 继续保持
  `connected`，等待真实平台、身份、稳定性及回滚证据。
- 本轮已完成支持单元级证据摘要的协议传递：宿主兼容性会返回实际命中的支持格证据，能力目录
  优先使用该摘要；注册表会拒绝正式单元缺少七类证据、证据不完整或重复平台/架构/加速器/部署格。
- 验证结果已固化：协议引擎针对性测试7项通过；注册表针对性测试4项通过；`hal100-infra`
  单元测试184项通过、7项忽略、0失败；`cargo clippy -p hal100-infra --all-targets -- -D warnings`
  通过；项目级 `pnpm check` 全量通过（文档一致性、Biome、前端类型、Desktop 26项、Sidecar
  34项、workspace Clippy、Agent Kernel构建与Rust workspace测试）。
- 版本基线为 `main` 上的 `v1.0.4` 开发版（当前HEAD为 `9d26d28`）。本次暂停不执行提交、
  合并或推送；工作区已有的未提交变更全部保留，恢复后应先按变更清单复核再继续。
- 恢复后的首要入口：建立并导入各引擎/支持格的真实验收证据，补齐容量与稳定性档案，完成
  多引擎并存、切换失败、旧路由恢复和重启补偿纵向；只有七类证据完整的支持格才允许晋级
  `verifiedExternal` 或 `managed`。

## 9. 恢复后新增：验收证据账本（2026-08-28）

- 新增 `contracts/inference-engines/v1-acceptance-evidence.json` 与 Rust 类型化解析器。账本以
  精确适配器和平台/架构/加速器/部署格为主键，约束记录大小、字段长度、证据数量、来源形态和
  时间戳；不允许凭据、URL、绝对路径、命令、控制字符或原始模型输出进入记录。
- 正式支持晋级新增显式门禁 `ExternalInferenceEngineRegistry::new_with_acceptance_evidence`。
  普通注册仍可承载 `connected`/`reserved` 适配器；一旦要求正式晋级，缺少匹配验收记录会故障
  关闭。当前账本为空，因此没有扩大任何现有支持等级。
- 新增6项账本单元测试及1项外部注册表门禁回归；定向测试与 Infra Clippy 均通过。最新一次
  项目级 `pnpm check` 在重复支持格与陈旧记录拒绝回归加入后全量通过；`hal100-infra` 单元
  测试为191通过、7忽略、0失败（共198项）。
- 恢复后的下一项不是填写占位记录，而是在各目标平台执行真实引擎验收并审查脱敏结果，再把
  记录导入账本；在此之前 vLLM、MLC LLM、OpenVINO、SGLang、LMDeploy、TensorRT-LLM 仍保持
  `connected`，不允许保存或激活运行方案。

- 同期新增 `v1-acceptance-run.schema.json` 与测试辅助输出：`HAL100_ACCEPTANCE_EVIDENCE_EMIT=1`
  只输出单次脱敏运行产物，显式的 `HAL100_ACCEPTANCE_EVIDENCE_WRITE=1` 加输出路径才会以
  create-new 方式写文件；产物可为部分证据，不能直接通过正式晋级门禁，也不会覆盖旧文件。审查
  后由 `append_run_as_formal` 校验七类证据并原子追加，失败不改变已有账本。

## 10. 本次暂停记录（2026-08-28 10:37 CST）

- 按用户要求暂停继续开发。当前目标仍保持 active，未宣称“全部引擎正式支持”完成，也未将
  任何 `connected` 引擎人为晋级。
- 代码与测试状态以本检查点第9节为准：验收证据账本解析、字段约束、重复支持格/陈旧记录
  拒绝，以及显式 `new_with_acceptance_evidence` 门禁均已落地；项目级 `pnpm check` 最近一次
  全量通过，Infra 单元测试为191通过、7忽略、0失败。
- 当前正式支持边界未变：llama.cpp（托管）、Ollama（macOS 外部）和 MLX-LM（Apple
  Silicon/Metal 外部）；vLLM、MLC LLM、OpenVINO Model Server、SGLang、LMDeploy、
  TensorRT-LLM 仍为 `connected`。
- 版本与仓库状态：`main` / `v1.0.4` / `9d26d28`；本次暂停不执行 commit、merge、push，
  工作区内已有未提交变更不得清理或覆盖。
- 恢复时从“各平台真实服务验收→生成并审查脱敏账本记录→补齐稳定性/并发与故障恢复证据→
  逐支持格晋级”开始；在真实证据导入前，账本继续保持空记录，正式支持门禁继续故障关闭。

## 11. 恢复后新增：运行产物与审查导入闭环（2026-08-28 11:02 CST）

- 新增 `contracts/inference-engines/v1-acceptance-run.schema.json` 与
  `InferenceEngineAcceptanceRun`。运行产物只在显式设置 `HAL100_ACCEPTANCE_EVIDENCE_EMIT=1`
  或同时设置 `HAL100_ACCEPTANCE_EVIDENCE_WRITE=1` 和明确输出路径时产生；文件使用 create-new，
  不创建父目录、不覆盖已有文件，且不包含凭据、URL、绝对路径、命令或原始输出。
- MLX-LM、vLLM、MLC LLM、OpenVINO Model Server、SGLang、LMDeploy、TensorRT-LLM 的忽略式真机
  入口均已接入统一产出辅助层。MLC LLM 与 LMDeploy 只记录实际可证明的协议/平台材料；没有
  稳定版本、部署身份或生命周期证据时不会生成对应断言，也不会改变 `connected` 等级。
- MLC LLM 适配器现在会在资格响应提供非空 `system_fingerprint` 时，将其与模型标识做确定性
  SHA-256 绑定为 `deploymentFingerprint`；空指纹仍保持不可用，不被当作版本或内容摘要。
- 新增 `append_reviewed_record` 与 `append_run_as_formal`：运行产物必须人工审查并补齐七类证据，
  并把其中的 `model-id-sha256:*` 防泄漏关联值替换为可复核模型修订，才能在内存中转换为正式
  记录；重复记录、重复支持格、字段漂移或任何校验失败都会在追加前故障关闭，已有账本保持不变。
  当前仓库账本仍为空。
- 本轮验证：验收证据模块12项测试通过；Infra所有目标Clippy通过；所有验收测试编译通过；
  最新项目级 `pnpm check` 全量通过（文档一致性、Biome、TypeScript、Desktop 26项、Sidecar
  34项、workspace Clippy、Agent Kernel构建、Rust workspace：Core 54、Desktop 99通过/16忽略、
  Infra 197通过/7忽略、Platform 12、Protocol 36；7个真实引擎入口默认忽略）。
- 正式支持等级仍未扩大。下一步必须在代表性 macOS/Windows/Linux 服务上真实运行这些入口，
  审查并导入证据，再补齐并发/稳定性、切换失败和重启补偿纵向；本轮未执行 commit、merge 或 push。

## 12. 本次暂停记录（2026-08-28 11:18 CST）

- 已修正 MLC LLM 真机验收入口中对部署指纹的过时断言：官方响应包含非空
  `system_fingerprint` 时允许适配器产生绑定模型标识的 64 位十六进制
  `deploymentFingerprint`；缺失时仍保持可选，不把空值伪装成版本或部署身份。
- 本次修改后门禁全部通过：`cargo fmt --all -- --check`、`cargo test -p hal100-infra --tests --no-run`、
  `cargo clippy -p hal100-infra --all-targets -- -D warnings`、`pnpm docs:check`、`git diff --check`，
  以及完整 `pnpm check`（文档一致性、Biome、TypeScript、Desktop 26项、Sidecar 34项、workspace
  Clippy、Agent Kernel 构建、Rust workspace：Core 54、Desktop 99通过/16忽略、Infra 197通过/7忽略、
  Platform 12、Protocol 36；七个真实引擎入口默认忽略）。
- 正式支持等级与账本状态没有变化：llama.cpp（托管）、Ollama（macOS 外部）和 MLX-LM（Apple
  Silicon/Metal 外部）保持既有等级；vLLM、MLC LLM、OpenVINO Model Server、SGLang、LMDeploy、
  TensorRT-LLM 仍为 `connected`，账本仍为空。
- 当前目标继续保持 active。恢复后从各引擎代表性平台真实服务运行、人工审查并导入脱敏账本记录，
  再补齐并发/稳定性、切换失败与重启补偿证据；本次仍不执行 commit、merge 或 push，工作区既有未提交
  变更全部保留。

## 13. 继续推进记录（2026-08-28 11:32 CST）

- 将稳定性证据从自由文本升级为结构化 `stability` 测量：共享 Rust 探针对同一已验证目标执行 20
  次 Chat Completions 请求，每波最多 4 个并发，每次响应必须有非空 choices 和正的 prompt/completion
  usage；产物只保存尝试次数、并发度和最大延迟，不保存提示词、回答、凭据或端点。
- 七个 live acceptance 入口均已调用该探针，运行产物中的稳定性证据必须同时携带结构化测量值；
  缺少测量值、超出边界或请求失败会故障关闭。伪服务测试已验证固定并发波次，相关 Infra 测试与
  Clippy、验收测试编译均通过；完整 workspace 测试中的 Infra 为 200 项通过、7 项忽略。
- 该能力只证明一个有界重复/并发样本，不替代取消、服务重启、切换失败和重启补偿的真实证据；因此
  正式支持等级仍不变，验收账本仍为空，七个新增引擎仍为 `connected`。

## 14. 本次暂停记录（2026-08-28 11:43 CST）

- 已开始收敛“部署身份不等于软件包版本”的验收字段：运行产物、正式记录和 JSON Schema
  增加可选 `deploymentFingerprint`；正式记录拟允许以有效的 64 位十六进制部署指纹替代
  `engineVersion`，以覆盖 MLC LLM 等无法稳定暴露包版本、但能提供部署指纹的服务。
- 七个新增引擎的忽略式验收入口已把适配器报告中的部署指纹传入产物辅助层；MLC LLM 的
  `system_fingerprint` 仍只在非空且经过模型标识绑定时使用，不会把空值冒充版本或身份。
- 本段改动尚未完成验证：验收证据单元测试夹具仍需补齐新字段，并需补充“无 engineVersion、
  有 deploymentFingerprint 可形成正式记录”的回归测试；随后应重新运行格式检查、Infra
  测试/Clippy、文档检查和完整 `pnpm check`。在这些门禁重新通过前，不应把本段视为已完成。
- 当前正式支持等级、验收账本和仓库基线均未改变：llama.cpp（托管）、Ollama（macOS 外部）
  与 MLX-LM（Apple Silicon/Metal 外部）保持既有等级；vLLM、MLC LLM、OpenVINO Model
  Server、SGLang、LMDeploy、TensorRT-LLM 仍为 `connected`；账本仍为空；`main` / `v1.0.4` /
  `9d26d28`，未执行 commit、merge 或 push。
- 恢复顺序：先补齐上述夹具/测试并完成全量门禁，再更新支持计划与当前状态文档，最后才继续
  各平台真实服务验收、人工审查和正式支持单元晋级。暂停期间保留工作区全部未提交变更。

## 15. 部署指纹闭环恢复记录（2026-08-28 11:51 CST）

- 已补齐验收证据测试夹具中的 `deploymentFingerprint` 字段，并新增回归测试：当服务没有可用的
  软件包版本、但提供有效的 64 位十六进制部署指纹时，审查后的运行产物可以形成正式记录；指纹
  仍需经过格式校验，空值或非法值会故障关闭。
- 验收辅助层的引擎身份断言已改为同时覆盖“引擎版本或部署身份”，七个 live acceptance 入口
  继续把适配器报告中的部署指纹作为可选脱敏字段写入产物。
- 本轮门禁全部通过：`cargo fmt --all -- --check`、`cargo test -p hal100-infra --lib
  engine_acceptance_evidence`（14 项）、`cargo test -p hal100-infra --tests --no-run`、
  Infra 全目标 Clippy、`pnpm docs:check` 和完整 `pnpm check`。完整 Rust workspace 的 Infra
  单元测试为 201 通过、7 忽略；七个真实引擎入口仍默认忽略。
- 正式支持等级没有扩大，账本仍为空：llama.cpp（托管）、Ollama（macOS 外部）和 MLX-LM
  （Apple Silicon/Metal 外部）保持既有等级；vLLM、MLC LLM、OpenVINO Model Server、SGLang、
  LMDeploy、TensorRT-LLM 仍为 `connected`。
- 下一项缺口仍是各引擎在代表性 macOS/Windows/Linux 服务上的真实身份、长时/并发、切换失败与
  重启补偿证据；只有审查后的七类证据完整并导入账本，才允许逐支持格晋级。当前版本基线仍为
  `main` / `v1.0.4` / `9d26d28`，没有执行 commit、merge 或 push。

## 16. 本次暂停记录（2026-08-28 12:01 CST）

- 已把“服务未暴露稳定软件包版本、但提供可验证部署身份”这一边界接入运行方案保存链路：
  `ENGINE_VERSION_NOT_EXPOSED` 是显式的“版本未暴露”标记，只有在存在有效
  `deploymentFingerprint` 时才允许保存；它不会被当作真实版本号、模型摘要或内容哈希。LMDeploy
  适配器在资格响应提供非空 `system_fingerprint` 时，会按模型标识做确定性绑定；缺失或非法指纹
  仍然故障关闭。
- Agent 与运行方案页面已对该标记做用户可读展示（“版本未暴露（由部署身份绑定）”），避免把
  内部哨兵值泄漏到用户界面；正式验收记录仍拒绝把该哨兵值当作 `engineVersion`，必须使用真实版本
  或有效部署指纹。
- 最新门禁均已通过：Rust 格式检查、Infra 验收测试编译、Infra 全目标 Clippy、文档一致性、
  `git diff --check` 以及完整 `pnpm check`；当前 Infra 单元测试为 202 项通过、7 项忽略，七个
  真机验收入口仍默认忽略。工作区未新增失败回归。
- 正式支持等级与验收账本没有变化：llama.cpp（托管）、Ollama（macOS 外部）和 MLX-LM（Apple
  Silicon/Metal 外部）保持既有等级；vLLM、MLC LLM、OpenVINO Model Server、SGLang、LMDeploy、
  TensorRT-LLM 仍为 `connected`，账本仍为空。没有真实多平台服务证据，因此不晋级任何引擎。
- 按用户要求暂停后续开发。Goal 继续保持 active；恢复时优先补充“带部署指纹但无包版本”的
  运行方案保存→激活→验证集成回归，再进入各平台真实服务验收、稳定性/并发、切换失败和重启补偿
  证据导入。当前基线仍为 `main` / `v1.0.4` / `9d26d28`；本次不执行 commit、merge、push，
  工作区现有未提交变更全部保留。

## 17. 账本驱动晋级与验收闭环记录（2026-08-28 12:17 CST）

- 新增 `ExternalInferenceEngineRegistry::new_with_reviewed_acceptance_evidence`：审查后的验收
  账本现在可以为精确的“引擎 × 平台 × 架构 × 加速器 × 部署”支持格生成只存在于内存的正式
  manifest 投影，并继续委托原适配器的探测、资格与认证边界。缺少记录的格保持 `connected`，
  记录与manifest不一致或外部适配器出现 `managed` 记录时故障关闭；不修改适配器源码、支持矩阵、
  数据库或账本文件。
- 真机验收辅助层新增仅测试作用域的目标支持格包装器：vLLM、OpenVINO Model Server、SGLang、
  TensorRT-LLM 等待晋级适配器可以在真实服务验收中运行“保存→复验→计划→激活→执行后验证”，
  而不需要把生产支持状态提前改成正式。该包装器不产生正式账本记录，MLC LLM/LMDeploy 仍使用
  部署指纹场景并在无真实证据时保持 `connected`。
- 新增运行方案回归：模拟 MLC 部署提供有效部署指纹但不暴露包版本时，方案成功保存、预检、激活、
  路由切换、激活后复验并可再次验证；内部 `qualification-required` 只作为“版本未暴露”哨兵，
  UI 不展示原始值，正式账本也拒绝将其当作版本。
- 本轮验证全部通过：`cargo fmt --all -- --check`、Infra 全目标 Clippy、Infra 验收测试编译、
  `pnpm docs:check`、`git diff --check` 和完整 `pnpm check`。完整 workspace 结果为 Core 54、
  Desktop 99通过/16忽略、Infra 204通过/7忽略、Platform 12、Protocol 36；七个真实引擎入口
  继续默认忽略。
- 支持等级与账本仍未扩大：llama.cpp（托管）、Ollama（macOS 外部）和 MLX-LM（Apple Silicon/
  Metal 外部）保持既有等级；vLLM、MLC LLM、OpenVINO Model Server、SGLang、LMDeploy、
  TensorRT-LLM 仍为 `connected`，账本为空。Goal 继续 active，下一步是代表性平台真实服务验收、
  故障切换/重启补偿证据和人工审查导入；当前不执行 commit、merge、push。

## 18. 本次暂停记录（2026-08-28 12:28 CST）

- 将真机验收生命周期辅助函数收敛为结构化输入，显式绑定目标加速器；OpenVINO Model Server
  不再因默认选择而误记为 CPU 格，vLLM、SGLang、TensorRT-LLM 也分别明确 CUDA 格。该辅助层
  仍仅在测试作用域内把目标支持格投影为 `VerifiedExternal`，不改变生产 manifest、支持矩阵或账本。
- 应用组合根改用 `standard_with_reviewed_acceptance_promotions`：未来导入人工审查后的精确账本记录
  时，只在内存中晋级匹配的“引擎 × 平台 × 架构 × 加速器 × 部署”格；当前账本为空，因此既有
  `connected`/正式状态保持不变，缺失记录不会导致启动失败，未知或不一致记录仍故障关闭。
- 本轮门禁全部通过：`cargo fmt --all`、Infra 全目标 Clippy（`-D warnings`）、Infra 所有测试编译、
  `pnpm check` 与 `git diff --check`。项目级结果为文档一致性通过、Biome 108 文件、Agent Kernel
  34 项、Desktop 26 项、Core 54、Desktop 99 通过/16 忽略、Infra 205 通过/7 忽略、Platform 12、
  Protocol 36；七个真实引擎验收入口仍按设计默认忽略。
- 正式支持等级与验收账本没有变化：llama.cpp（托管）、Ollama（macOS 外部）和 MLX-LM（Apple
  Silicon/Metal 外部）保持既有等级；vLLM、MLC LLM、OpenVINO Model Server、SGLang、LMDeploy、
  TensorRT-LLM 仍为 `connected`。当前没有代表性 macOS/Windows/Linux 真实服务证据，不能晋级。
- 按用户要求暂停开发。Goal 继续保持 active；恢复时从真实平台服务验收、人工审查/导入证据、并发
  稳定性、切换失败和重启补偿纵向继续。当前基线仍为 `main` / `v1.0.4` / `9d26d28`；工作区已有
  大量未提交改动全部保留，本轮不执行 commit、merge 或 push。

## 19. 审查验收导入闭环记录（2026-08-28 12:41 CST）

- 新增 `InferenceEngineAcceptanceRun::into_formal_record_with_model_revision`，把脱敏运行产物
  导入正式账本前的人工审查边界固定下来：仅接受完整且 `passed` 的产物，审查人必须明确填入
  不以 `model-id-sha256:` 开头的不可变模型修订标识；空值、哨兵值或超界文本均故障关闭。
- 新增离线命令 `hal100-engine-acceptance-import`：对运行产物和现有账本执行有界、非符号链接读取，
  只允许将精确目标支持格追加为 `verifiedExternal`，再用全部标准适配器校验候选账本，最后以
  create-new 方式写出新账本。工具不会启动引擎、下载模型、修改支持矩阵，也不会自动晋级或覆盖已有文件。
- 应用组合根已接入 `standard_with_reviewed_acceptance_ledger`；无记录的支持格保持原状态，未知、
  不一致或越权记录故障关闭。真实引擎证据仍须由维护者在代表性平台完成验收、人工核对后再运行导入。
- 定向验收证据测试为 16 项通过；命令行 `--help` 与参数/路径安全行为已验证；完整 `pnpm check`
  通过（文档一致性、Biome 108 文件、TypeScript、Agent Kernel 34、Desktop 26、Core 54、
  Desktop 99 通过/16 忽略、Infra 207 通过/7 忽略、Platform 12、Protocol 36，以及全部 doc-tests）；
  `git diff --check` 通过。
- 正式支持等级与账本状态没有变化：llama.cpp（托管）、Ollama（macOS 外部）和 MLX-LM（Apple
  Silicon/Metal 外部）保持既有等级；vLLM、MLC LLM、OpenVINO Model Server、SGLang、LMDeploy、
  TensorRT-LLM 仍为 `connected`，账本仍为空。当前没有代表性 macOS/Windows/Linux 真实服务证据，
  因此不晋级任何引擎。
- Goal 继续保持 active。下一步是逐引擎完成真实多平台服务的身份、长时/并发、切换失败和重启补偿
  验收，人工审查并导入账本后逐支持格晋级；本轮不执行 commit、merge、push，工作区原有未提交
  变更全部保留。

## 20. 多加速器支持格歧义保护记录（2026-08-28 12:49 CST）

- 修正 `InferenceEngineManifest::compatibility_with` 的安全边界：当同一宿主同时匹配多个加速器，
  且其中至少一个支持格已正式验收、另一个仍为 `connected`/`reserved` 时，不再取最高支持等级
  作为整体结论，而是返回 `supportCellAmbiguous` 并故障关闭兼容性。
- 确定性推荐新增同名原因，桌面类型与文案同步；用户必须先明确选择具体加速器，才能在未来为该
  支持格建立方案保存与激活绑定。当前仍未新增任何真实引擎支持等级，也未修改验收账本。
- 新增协议兼容性与推荐回归测试：混合 OpenVINO CPU/设备支持格拒绝未选择场景；推荐结果保持
  不可用且分数为 0。定向 Protocol 9 项、Infra 推荐 3 项测试通过；此前完整 `pnpm check` 仍为
  退出码 0，文档一致性与 `git diff --check` 在文档更新后再次通过。
- 正式支持边界保持不变：llama.cpp（托管）、Ollama（macOS 外部）、MLX-LM（Apple Silicon/Metal
  外部）为既有正式单元；vLLM、MLC LLM、OpenVINO Model Server、SGLang、LMDeploy、TensorRT-LLM
  仍为 `connected`。长期 Goal 继续 active；真实多平台服务验收和显式加速器绑定仍是后续晋级门槛。

## 21. 恢复后状态复核记录（2026-08-28 12:55 CST）

- 复核当前工作区和本机外部状态：未发现 `ollama`、`mlx_lm`、`mlc_llm`、`vllm`、`sglang`、
  `lmdeploy`、`trtllm-serve` 或 `ovms` 可执行入口，8000、8001、8080、11434、23333、30000
  回环监听端口均为空；因此本轮没有可安全导入的真实服务验收产物。
- 支持账本继续保持空记录，任何新增引擎不因伪服务、静态 manifest 或“已连接”观察而晋级。下一次
  应从准备一台代表性 Linux CUDA 或 Windows Intel/MLC 主机开始，运行对应 ignored acceptance，
  再进行人工审查导入和精确支持格复验。

## 22. 支持格歧义协议线稳定性记录（2026-08-28 12:57 CST）

- 为新增的 `SupportCellAmbiguous` 兼容性问题和推荐原因补充稳定 JSON 名称回归，锁定桌面端使用的
  `supportCellAmbiguous` wire value，避免跨平台 UI 因枚举重命名产生漂移。
- Protocol 引擎测试现为 10 项通过；代码已重新格式化。该项只加强状态表达和故障关闭，不改变
  支持矩阵、验收账本或任何引擎的正式等级。
- 新增 wire 回归后再次运行完整 `pnpm check`，退出码为 0；文档一致性和 `git diff --check` 均通过。

## 23. 验收来源路径边界记录（2026-08-28 13:05 CST）

- 收紧验收运行产物与正式账本的来源断言：来源必须是仓库相对路径，并限定在 `contracts/`、
  `crates/`、`docs/` 或根部 `README.md`；路径穿越、双斜杠、反斜杠、URL、绝对路径和伪造根部文件名
  均故障关闭。该约束同时作用于运行产物解析和正式记录追加。
- 新增路径边界回归覆盖，验收证据模块定向测试为 17 项通过；Infra 全目标 Clippy 通过。支持矩阵、
  账本内容和引擎支持等级保持不变。

## 24. 本次暂停与精确恢复点（2026-08-28 13:10 CST）

- 按用户要求暂停开发。本次暂停后不再继续修改功能代码；长期 Goal 继续保持 active，不标记完成，
  也不把尚未实现的规划写成已有能力。
- 当前已完成到“支持格歧义故障关闭”：当同一宿主同时匹配正式和非正式加速器支持格时，协议、
  推荐层和桌面文案会返回稳定的 `supportCellAmbiguous`，禁止把最高支持等级当成整个宿主的授权。
  验收运行产物、人工审查导入、精确支持格账本投影和来源路径边界均已具备。
- 当前尚未开始“运行方案精确支持格持久化”。运行方案协议仍为 spec v3，SQLite 仍为 schema v13；
  `RuntimeProfileAdapterBinding`、外部方案草稿和数据库记录尚未保存“平台 × 架构 × 加速器 × 部署”
  四元组，激活授权也尚未把该四元组纳入 CAS/漂移检查。因此现有歧义场景继续安全地故障关闭，
  不能宣称已经支持用户显式选择后保存和激活。
- 精确恢复顺序固定为：先定义支持格 DTO 与选择规则；再新增 SQLite schema v14 的向前迁移；随后
  接入托管/外部方案保存、目录复验、激活计划、授权绑定与恢复；最后同步桌面候选项/加速器选择，
  补齐协议、数据库、管理器、Tauri 和 React 测试并重新运行完整门禁。旧方案缺少支持格时必须保持
  保守状态，不能静默推断跨加速器授权。
- 正式支持边界没有变化：llama.cpp 为托管正式支持，Ollama 为 macOS 外部正式支持，MLX-LM 为
  Apple Silicon/Metal 外部正式支持；vLLM、MLC LLM、OpenVINO Model Server、SGLang、LMDeploy、
  TensorRT-LLM 仍为 `connected`。`contracts/inference-engines/v1-acceptance-evidence.json` 仍为空，
  没有真实多平台服务证据，因此不晋级任何支持格。
- 暂停时已确认此前最新完整 `pnpm check`、Infra 全目标 Clippy、验收来源路径定向测试和
  `git diff --check` 均通过；本次只新增暂停记录，不重复运行耗时的全量测试。仓库基线仍为
  `main` / `v1.0.4` / `9d26d28`，与 `origin/main` 对齐；工作区已有大量未提交改动全部保留，
  不执行 commit、merge 或 push。

## 25. 支持格持久化与暂停记录（2026-08-28）

- 已完成暂停点之后的精确恢复项：新增 `RuntimeProfileSupportCell`，以“平台 × 架构 × 加速器 ×
  部署”四元组贯穿运行方案协议、外部候选、保存草稿、激活计划和授权绑定。支持格选择遵循
  正式 manifest：唯一正式匹配格可自动解析；多个匹配格必须由用户明确选择；显式选择非正式或
  与宿主不匹配的格会故障关闭。
- SQLite 已升级到 schema v14。`runtime_profiles` 持久化四个支持格字段，并通过 allowlist 与完整性
  触发器拒绝未知或部分四元组；旧 schema v13 方案仍可读，但因缺少支持格进入 `NeedsRepair`，
  不会静默推断加速器授权。激活 CAS、目录复验和启动恢复均把支持格纳入漂移检查。
- Tauri 能力目录只为当前宿主上精确匹配且已正式支持的外部实例生成候选，并把支持格传到桌面；
  React 运行方案页新增支持格选择、已保存方案标签和激活计划展示。支持格变化、缺失和歧义均有
  稳定协议枚举与中文文案。
- 定向验证已完成：Protocol、数据库和 RuntimeProfileManager 单元测试通过；Tauri 外部候选精确
  身份测试通过；workspace `cargo check --all-targets` 通过；前端 Biome 与 TypeScript 检查通过。
  最新完整 `pnpm check` 已通过：文档一致性（schema v14）、Biome 108 文件、TypeScript、前端
  26、Agent Kernel 34、Core 54、Desktop 99（16 忽略）、Infra 211（7 忽略）、Platform 12、
  Protocol 38、全部 doc-tests，以及 Clippy、Agent Kernel build 均无失败；`cargo fmt --all --check`
  与 `git diff --check` 也通过。
- 正式支持等级没有变化：llama.cpp（托管）、Ollama（macOS 外部）和 MLX-LM（Apple Silicon /
  Metal 外部）保持既有等级；vLLM、MLC LLM、OpenVINO Model Server、SGLang、LMDeploy、
  TensorRT-LLM 仍为 `connected`。本机没有七个待验收服务或有效端口，验收账本仍为空，不能晋级
  任何新支持格。
- 本记录完成后暂停开发。长期 Goal 保持 active；恢复时从真实代表性 macOS/Windows/Linux 服务
  验收、人工审查导入账本和逐格晋级继续，不执行签名、公证、安装包、自动更新、正式升级流程，
  也不在当前工作区执行 commit、merge 或 push。

## 26. 激活授权漂移回归与验收入口收紧（2026-08-28）

- 新增 RuntimeProfileManager 回归：运行方案激活计划生成后，如果数据库中的支持格四元组被改变，
  即使模型证据、版本和时间戳保持不变，`apply_activation` 也会因授权绑定 CAS 不匹配返回
  `ProfileChanged`，不会切换 Gateway 路由或创建激活 journal。
- 收紧 LMDeploy 真实验收入口：当前 manifest 只声明 Linux/Windows x86_64 CUDA 支持格，入口不再
  接受未声明的 ROCm 参数，避免验收产物把未来规划单元伪装成当前支持单元。vLLM 入口同时严格
  断言 Linux，并按实际 x86_64/aarch64 主机生成对应支持格，禁止在其他平台产生伪造的 Linux 证据。
- 新增回归后，支持等级、验收账本和矩阵均未改变；vLLM、MLC LLM、OpenVINO Model Server、
  SGLang、LMDeploy、TensorRT-LLM 继续保持 `connected`，真实服务不存在时仍默认忽略 live acceptance。

## 27. Windows 加速器候选探针（2026-08-28）

- Windows `NativeSystemProbe` 现在通过固定、非交互的 CIM `Win32_VideoController.PNPDeviceID`
  查询读取有限显卡设备标识，并只按 PCI 厂商白名单生成 CPU、CUDA、ROCm、OpenVINO 候选；未知
  厂商和重复设备不会扩大候选集合。该结果只是宿主能力线索，不等价于 CUDA/ROCm/OpenVINO
  运行时或引擎正式支持。
- 跨平台纯函数回归覆盖 NVIDIA（`VEN_10DE`）、AMD（`VEN_1002`）、Intel（`VEN_8086`）和
  未知厂商组合；现有 Linux 双证据 CUDA 规则与 macOS CPU/Metal 探针保持不变。
- 这使 Windows CUDA/ROCm/OpenVINO 支持格能够进入后续“宿主候选 → 引擎资格 → 运行方案”链路，
  但当前没有 Windows 真机引擎验收证据，因此支持矩阵与正式等级不变。

## 28. 当前暂停点与恢复清单（2026-08-28）

- 按用户要求再次暂停。当前已落地的主线是“精确支持格绑定”：运行方案、外部候选、SQLite
  schema v14、目录复验、激活授权 CAS/恢复和桌面选择均携带平台 × 架构 × 加速器 × 部署四元组；
  缺失、变更、歧义和非正式支持格都会保守拒绝，不会静默扩大授权范围。
- Windows 宿主探针已加入固定 CIM 显卡厂商映射（CPU/CUDA/ROCm/OpenVINO 候选），并通过纯函数
  回归和 `cargo check -p hal100-platform --target x86_64-pc-windows-msvc`；这只是候选能力线索，
  不等价于 Windows 引擎正式支持。
- 在当前 macOS 开发机上执行 `cargo check -p hal100-infra --tests --target
  x86_64-pc-windows-msvc` 仍受本机缺少 MSVC/Windows SDK 阻塞（`aws-lc-sys` 无法找到
  `windows.h`、`stdlib.h`）。这是交叉编译环境限制，不代表 Rust 业务代码已在 Windows 失败；
  需在 Windows CI/原生 Windows 主机补做 Infra 全目标检查。
- 最近的定向验证包括 RuntimeProfileManager 支持格漂移 CAS 回归、验收证据模块 17 项、Platform
  14 项、MLC/vLLM/LMDeploy live acceptance 编译检查、格式和 diff 检查；此前完整 `pnpm check`
  已通过，但在本暂停点不宣称新引擎已完成真实平台验收。
- 正式支持等级保持不变：llama.cpp（托管）、Ollama（macOS 外部）和 MLX-LM（Apple Silicon /
  Metal 外部）为既有正式单元；vLLM、MLC LLM、OpenVINO Model Server、SGLang、LMDeploy、
  TensorRT-LLM 仍为 `connected`。本机未运行待验收服务，`contracts/inference-engines/
  v1-acceptance-evidence.json` 仍为空，不能晋级任何新增支持格。
- 恢复时按以下顺序继续：准备代表性 macOS/Windows/Linux 引擎服务 → 运行对应 ignored live
  acceptance（身份、长时/并发、失败切换、重启补偿）→ 人工审查并导入账本 → 按精确支持格晋级
  矩阵和推荐 → 在原生 Windows CI 补齐跨目标 Infra 门禁。当前不执行 commit、merge、push，
  也不涉及签名、公证、安装包、自动更新或正式升级流程；工作区已有未提交改动全部保留。

## 29. 真实主机快照收紧（2026-08-28）

- 修正八个 live acceptance 入口的证据边界：不再构造合成 CPU/GPU/内存字段，统一调用
  `hal100-platform::NativeSystemProbe::capability_snapshot`，并强制匹配声明的平台、架构和
  加速器。缺少真实 CUDA、Metal、ROCm、Vulkan 或 OpenVINO 候选时，验收直接失败，不能靠环境
  变量伪造硬件能力。
- OpenVINO Model Server 入口新增必填 `HAL100_OPENVINO_ACCELERATOR=cpu|openvino`，将 CPU 与
  OpenVINO 设备支持格拆开验收；MLC LLM、vLLM、SGLang、LMDeploy、TensorRT-LLM 和 MLX-LM
  均通过相同的原生快照辅助函数生成生命周期与运行产物输入。
- Infra 全部集成测试目标已重新编译通过（包括七个 ignored live acceptance）；文档一致性、
  格式和 diff 检查通过。该项仍只提高证据可信度，不改变任何正式支持等级；账本保持空记录，
  真实服务和人工审查仍是后续晋级门槛。

## 30. MLC/LMDeploy 正式验收入口收紧（2026-08-28）

- MLC LLM 与 LMDeploy 的 live acceptance 现在都要求资格响应提供非空且稳定的
  `system_fingerprint`，并将其作为模型绑定的部署指纹；缺失指纹会在生成生命周期证据前直接
  失败，不再输出无法晋级的半成品运行记录。
- 两个入口均执行真实主机快照、保存运行方案、生成并应用激活计划、切换后复验和 20 次/4 并发
  稳定性探针，只有完成这些步骤才会标记 `lifecycleVerified`。LMDeploy 虽无稳定包版本端点，
  仍可用经过适配器绑定的部署指纹满足身份证据；最终晋级仍需人工审查模型不可变修订并导入账本。
- 该项只收紧验收质量门槛，未修改 checked-in 支持矩阵或账本；MLC LLM、LMDeploy 以及其余待验收
  引擎继续保持 `connected`，因为当前开发机没有对应真实服务。

## 31. 统一真实验收执行入口（2026-08-28）

- 新增 `scripts/run-engine-live-acceptance.sh`，以白名单将 `ollama`、`mlx-lm`、`mlc-llm`、`openvino`、
  `vllm`、`sglang`、`lmdeploy`、`tensorrt-llm` 映射到固定 ignored 测试与精确测试名称，避免
  不同平台手工命令漂移。
- 脚本要求操作者设置 `HAL100_RUN_REAL_ACCEPTANCE=1`，拒绝未知引擎、模糊确认和覆盖已有产物；
  它只执行已准备好的本机回环服务测试，默认把 create-new 脱敏运行产物写入
  `output/inference-acceptance/`，不会启动、安装、下载、停止、重配置引擎或自动导入账本。
- 脚本语法、帮助和未知引擎拒绝路径已验证；Infra 八个 live acceptance 全部重新编译通过。
  该项提供了后续原生 macOS/Windows/Linux 主机验收的统一入口，但尚未产生真实服务证据，支持
  矩阵与正式等级保持不变。

## 32. 本轮门禁结果与下一阶段（2026-08-28）

- 本轮在真实主机快照和指纹门槛收紧后重新运行完整 `pnpm check`，退出码为 0：文档一致性、
  Biome 108 文件、TypeScript、Agent Kernel 34、Desktop 26、Core 54、Desktop Rust 99（16
  忽略）、Infra 212（7 忽略）、Platform 14、Protocol 38、全部 doc-tests、Clippy 和 Agent
  Kernel build 均通过；`cargo fmt --all --check` 与 `git diff --check` 通过。
- 当前仍没有可用的真实 vLLM、MLC LLM、OpenVINO Model Server、SGLang、LMDeploy 或
  TensorRT-LLM 服务，验收账本保持空记录；因此不把“测试入口可编译”或“协议夹具通过”误写成
  正式支持。
- 下一阶段唯一能改变支持等级的证据是原生 macOS/Windows/Linux 主机上的真实服务运行：原生
  探针快照、引擎身份/部署指纹、模型不可变修订、协议资格、运行方案激活/复验、并发稳定性、
  失败切换和重启补偿；完成后再由人工审查导入账本，并按精确支持格晋级。

## 33. Ollama Agent 资格与全量回归（2026-08-28）

- Ollama 外部只读适配器新增共享 OpenAI Agent 资格探针：固定版本、模型目录、单工具调用、
  流式 SSE、Usage 和工具资格均需通过；可选 `system_fingerprint` 只作为模型绑定部署指纹，
  不授予额外生命周期权限。
- 新增 Apple Silicon / Metal 或 CPU 的 Ollama ignored live acceptance，使用真实
  `NativeSystemProbe`、运行方案保存/激活/复验和稳定性探针；统一脚本现覆盖全部八个规划引擎。
- `pnpm check` 在新增 Ollama 入口后重新通过：Biome、TypeScript、前端测试、Clippy、Agent
  Kernel build、Core 54、Desktop 99（16 忽略）、Infra 213（7 忽略）、Platform 14、Protocol
  38、全部 doc-tests，以及格式和 diff 检查均无失败。
- Ollama 真实服务尚未在当前开发机运行，验收账本仍为空；现有正式支持声明不扩展到 Windows/Linux
  保留单元，其他七个引擎也仍按精确支持格等待真实服务证据和人工审查导入。

## 34. 当前暂停记录（2026-08-28）

- 按用户要求在本轮再次记录暂停点。当前工作区保留所有既有未提交改动，`main` 的开发基线仍为
  `9d26d28` / `v1.0.4`；本次没有执行 commit、merge 或 push。
- Ollama 适配器已补齐正式的 OpenAI Agent 协议资格检查（版本、模型、Chat、SSE、Usage 和单工具调用），
  并新增 Apple Silicon CPU/Metal 的 ignored live acceptance；统一脚本现在覆盖规划内八个引擎。
- 八个真实验收入口均使用三平台 `NativeSystemProbe` 生成真实主机快照，并执行有界 20 次/每波最多 4
  并发稳定性探针；MLC LLM 与 LMDeploy 还要求非空 `system_fingerprint` 才能形成部署身份证据。
- 最近一次完整 `pnpm check` 退出码为 0：文档一致性、Biome 108 文件、TypeScript、Agent Kernel 34、
  Desktop 26、Core 54、Desktop Rust 99（16 忽略）、Infra 213（7 忽略）、Platform 14、Protocol 38、
  全部 doc-tests、Clippy、Agent Kernel build、格式和 diff 检查均通过。
- 当前机器没有 `ollama`、`mlx_lm`、`mlc_llm`、`vllm`、`sglang`、`lmdeploy`、`trtllm-serve` 或 `ovms`
  服务，也没有对应监听端口；`contracts/inference-engines/v1-acceptance-evidence.json` 仍为零记录。
  因此没有任何新的支持格被晋级，vLLM、MLC LLM、OpenVINO Model Server、SGLang、LMDeploy 与
  TensorRT-LLM 继续保持 `connected`。
- 恢复后的唯一有效晋级路径仍是：在原生 macOS/Windows/Linux 代表性主机准备固定服务，运行统一脚本，
  人工复核模型不可变修订并通过 `hal100-engine-acceptance-import` 导入，再按精确支持格更新正式矩阵；
  不得用协议夹具、合成硬件快照、环境变量或空账本替代真实证据。签名、公证、安装包、自动更新和正式
  升级流程继续不在范围内。

## 35. Ollama 统一资格路径修复（2026-08-28）

- 修复运行方案管理器仍对 Ollama 使用旧版“目录身份特例”的架构遗漏。现在 Ollama 与其他外部引擎
  一样，保存方案、生成激活计划、激活前复验和激活后复验都会调用适配器的实时 OpenAI Agent 资格请求，
  不再仅凭模型目录通过。
- Ollama 的协议能力指纹统一改为共享资格探针生成的 canonical hash；旧的 catalog-only hash 不再作为
  运行方案授权依据。内容 digest 仍保留为 Ollama 模型的内容身份证据，部署指纹仍按适配器报告可选绑定。
- 运行方案管理器的外部 Ollama 测试夹具已改为显式返回版本、模型和协议能力资格报告，覆盖多模型保存、
  并发保存、激活、漂移、回滚和恢复路径；`hal100-infra` 221 项库测试全部通过（7 项按设计忽略）。
- 随后项目级 `pnpm check` 全量通过；当前机器没有真实 Ollama 服务，验收账本仍为空，正式支持范围不变。

## 36. vLLM 资格版本证据闭环（2026-08-28）

- 修复 vLLM 适配器资格阶段遗漏精确版本证据的问题：`qualify` 现在在共享 OpenAI Agent 协议探针通过后，
  通过同一个受控目标读取有界 `/version`，并把版本写入 `EngineQualificationReport`。保存方案、激活前复验
  和激活后复验因此可以把资格观察与发现快照做一致性比较，服务替换或版本漂移会故障关闭。
- vLLM 单元测试新增资格版本断言；ignored 的 Linux/CUDA 真实验收入口也要求资格版本与发现版本及其显式
  期望值一致，避免仅凭协议夹具通过就形成不完整验收证据。
- 本次修复不改变 vLLM 支持等级、平台矩阵或验收账本；当前开发机仍没有真实 vLLM 服务，账本保持空记录。
- `cargo fmt --all`、vLLM 定向测试和随后完整 `pnpm check` 均已通过；Goal 继续停留在“真实服务验收与
  人工审查导入”阶段；不执行签名、公证、安装包、自动更新、正式升级流程或 Git 发布操作。

## 37. Linux 平台探针编译与加速器候选补齐（2026-08-28）

- 修复 Linux 目标编译时宿主探针缺少 `InferenceAccelerator` 引入的问题；`hal100-platform` 现在可在
  `x86_64-unknown-linux-gnu` 与 `x86_64-pc-windows-msvc` 目标通过 `cargo check`。
- Linux `NativeSystemProbe` 增加保守的 DRM/render-node 设备证据映射：NVIDIA 驱动与 PCI 厂商共同证明
  CUDA；AMD PCI 厂商加 `/dev/kfd` 证明 ROCm 候选；已知 GPU 加 render node 生成 Vulkan 候选；Intel GPU
  加 render node 生成 OpenVINO 候选。所有结果仍只是宿主候选，必须继续经过引擎资格和真实模型验收。
- 新增纯函数回归覆盖全候选组合与无设备时仅 CPU 的故障关闭行为；Platform 单元测试增至 15 项并全部通过。
- Windows 已知 PCI GPU 也会生成 Vulkan 候选，和 CUDA/ROCm/OpenVINO 一起进入同一受控链路。该项扩展了
  后续 MLC LLM、OpenVINO 等 Linux/Windows 支持格进入“宿主候选 → 引擎资格 → 运行方案”链路的能力，
  不改变当前支持矩阵或正式等级；真实服务与验收账本仍为空。

## 38. 当前暂停记录（2026-08-28，最新）

- 按用户要求在本轮暂停并记录进度。`main` 的开发基线仍为 `9d26d28` / `v1.0.4`；工作区继续
  保留既有未提交改动，本轮没有执行 commit、merge、push，也没有触碰签名、公证、安装包、自动更新
  或正式升级流程。
- 最新代码状态：八个规划引擎均已有类型化适配器、实例观察、资格探针、运行方案保存/激活/复验和
  ignored live acceptance 入口；Ollama 已统一走实时 OpenAI Agent 资格路径，vLLM 资格报告已绑定同一
  有界 `/version` 精确版本；Windows/Linux 宿主探针可按目标编译，Linux 增加了有界 DRM/render-node/
  驱动证据到 CUDA、ROCm、Vulkan、OpenVINO 候选的保守映射，Windows 已知 PCI GPU 同样生成 Vulkan 候选。
- 最新验证结果：完整 `pnpm check` 退出码为 0；文档一致性、Biome 108 文件、TypeScript、Agent Kernel
  34、Desktop 26、Core 54、Desktop Rust 99（16 忽略）、Infra 222（215 通过、7 忽略）、Platform 15、
  Protocol 38、全部 doc-tests、Clippy、Agent Kernel build、`cargo fmt --all --check` 与 `git diff --check`
  均通过。`hal100-platform` 的 Linux x86_64 与 Windows x86_64 目标 `cargo check` 通过；Infra Linux
  交叉测试仍受本机缺少 `x86_64-linux-gnu-gcc` 的 `aws-lc-sys` 工具链限制，需原生 CI/主机补齐，不是代码
  失败。
- 正式支持等级没有变化：当前正式单元仍为 Apple Silicon/Metal llama.cpp、既有 macOS Ollama 和 MLX-LM；
  vLLM、MLC LLM、OpenVINO Model Server、SGLang、LMDeploy、TensorRT-LLM 继续保持 `connected`。本机没有
  真实对应服务或监听端口，`contracts/inference-engines/v1-acceptance-evidence.json` 仍为零记录，不能
  用夹具、环境变量或候选硬件映射替代真实证据。
- 恢复入口：在原生 macOS/Windows/Linux 准备代表性服务与固定模型，使用
  `scripts/run-engine-live-acceptance.sh` 执行对应 ignored 测试；人工复核七类证据并运行
  `hal100-engine-acceptance-import` 导入账本，再按精确“平台 × 架构 × 加速器 × 部署”支持格晋级。Goal
  仍保持 active，下一阶段只聚焦真实服务验收、证据审查导入与逐格正式支持，不扩大到发行流程。

## 39. 验收证据实例绑定（2026-08-28）

- 审计发现原验收运行产物只绑定“适配器 + 支持格”，没有绑定实际服务实例；已将 Rust 合同扩展为
  必填的实例 ID、验证 origin 的 64 位指纹和配置修订。原始 API 根、凭据和命令仍不会进入产物。
- 八个 live acceptance 入口现在从不可序列化的 `VerifiedEngineTarget` 自动填充三项身份，不能由
  环境变量或人工文本伪造；运行产物和导入后的正式账本均执行实例、origin、配置修订边界校验。
- `v1-acceptance-run.schema.json` 已同步要求 `instanceId`、`originFingerprint`、`configRevision`，并新增
  Rust 回归覆盖无效指纹、越权实例字符和零配置修订。该项提高跨实例重放防护，不改变支持矩阵或账本。

## 40. 协议能力哈希结构化绑定（2026-08-28）

- 验收产物原先只在 `ProtocolQualification` 断言文本中携带能力哈希；现在增加必填的
  `protocolCapabilityHash` 字段，由 Rust 校验为 64 位十六进制值，并随正式账本记录保留。
- 八个 live acceptance 入口继续从资格报告自动填充该字段；人工无法通过修改断言文本替换协议能力
  身份。JSON Schema、导入前记录校验和回归测试已同步更新。
- 该项只增强协议资格证据的可审计性，不改变当前支持矩阵或空账本状态；真实服务验收仍是正式晋级门槛。

## 41. 当前暂停记录（2026-08-28，协议能力哈希绑定后）

- 本次按用户要求暂停开发并记录现场。工作区保留既有未提交改动，`main` 基线仍为
  `9d26d28` / `v1.0.4`；本次没有执行 commit、merge 或 push，也没有进入签名、公证、安装包、
  自动更新或正式升级流程。
- 验收合同现在要求结构化 `protocolCapabilityHash`，并由八个外部适配器公开各自的 canonical
  协议能力哈希。审查账本驱动的支持格晋级在变体哈希不匹配时故障关闭；包装后的适配器继续把
  同一哈希用于运行方案资格绑定，避免把某个引擎变体的协议证据重放到另一个变体。
- 八个 live acceptance 入口仍从 Rust 验证过的目标和资格报告自动写入实例 ID、origin 指纹、
  配置修订与协议能力哈希；产物写出前还会通过标准注册表复核目标适配器和协议哈希，运行产物及导入账本均执行边界校验。当前
  `contracts/inference-engines/v1-acceptance-evidence.json` 仍为空，真实服务未准备，不能据此
  晋级任何新的正式支持格。
- 最新门禁：完整 `pnpm check` 退出码为 0；Biome 108、TypeScript、Agent Kernel 34、Desktop 26、
  Core 54、Desktop Rust 99（16 忽略）、Infra 223（216 通过、7 忽略）、Platform 15、Protocol 38、
  全部 doc-tests、Clippy、Agent Kernel build、格式检查与 `git diff --check` 均通过。外部适配器定向
  测试为 11/11 通过。
- 正式支持等级保持不变：Apple Silicon/Metal llama.cpp、既有 macOS Ollama 和 MLX-LM 为正式单元；
  vLLM、MLC LLM、OpenVINO Model Server、SGLang、LMDeploy、TensorRT-LLM 继续为
  `connected`。没有真实 macOS/Windows/Linux 服务、固定模型修订和人工审查记录时，不把协议夹具、
  合成硬件快照、候选加速器或空账本当作正式支持证据。
- 本次恢复后已完成第一项技术任务：严格账本构造器和审查晋级构造器均要求“记录哈希必须匹配适配器
  变体”，且未声明固定协议合同的自定义适配器不能被账本晋级；新增不匹配回归覆盖两条构造路径。
- 运行方案授权也已收紧为完整适配器身份匹配：协议能力哈希预期同时检查引擎家族、变体和合同修订，
  未知变体或合同版本无法通过保存后的读取、预检或资格复验路径。
- 下一步仍是原生三平台执行统一脚本，导入经过人工复核的运行产物，按“平台 × 架构 × 加速器 × 部署”
  逐格晋级，并补齐长时稳定性、并发、取消、失败切换、回滚和重启补偿证据。

## 42. 当前暂停记录（2026-08-28 15:55 CST）

- 按用户要求在本检查点暂停后续开发。当前分支为 `main`，开发基线为 `9d26d28` / `v1.0.4`；
  工作区存在此前积累的未提交变更，本次不清理、不覆盖，也不执行 commit、merge 或 push。
- 本轮已完成的架构门禁保持有效：验收运行产物、审查账本、运行方案保存/读取/预检/资格复验
  均绑定完整适配器身份（引擎、变体、合同修订）及 `protocolCapabilityHash`；未知适配器、未知
  变体、未知合同或哈希不匹配均故障关闭。live acceptance 入口写出前还会复核目标适配器与哈希。
- 当前正式支持等级不变：Apple Silicon/Metal llama.cpp、macOS Ollama、Apple Silicon/Metal
  MLX-LM 为正式支持；vLLM、MLC LLM、OpenVINO Model Server、SGLang、LMDeploy、TensorRT-LLM
  仍为 `connected`。验收账本 `contracts/inference-engines/v1-acceptance-evidence.json`
  仍为空，当前开发机没有可用的真实引擎服务，因此没有新增可晋级支持格。
- 最近一次全量门禁保持通过：`pnpm check`、`pnpm docs:check`、`cargo fmt --all -- --check`、
  `git diff --check`；Infra 为 223 项（216 通过、7 忽略），外部适配器定向测试 11/11 通过，
  Linux 与 Windows x86_64 平台 `cargo check` 通过。
- 恢复开发的明确顺序：在原生 macOS/Windows/Linux 主机准备代表性真实服务，运行统一验收脚本，
  人工审查并导入脱敏证据；随后补齐长时稳定性、并发、取消、失败切换、回滚与重启补偿证据，
  最后按“平台 × 架构 × 加速器 × 部署”逐格晋级。长期 Goal 继续保持 active，未宣称全部引擎完成。

## 43. 继续推进记录（2026-08-28，发现服务注册表收口）

- 审计发现 `LocalBackendDiscoveryService::new()` 原先直接使用未应用审查晋级的标准注册表，
  可能让嵌入调用方看到的能力状态与桌面生产组合根、运行方案管理器不一致。
- 默认构造现改为使用 `standard_with_reviewed_acceptance_promotions()`；显式注入的发现服务仍
  复用调用方传入的同一注册表，因而桌面能力目录、候选生成和运行方案授权继续共享完整的适配器
  身份、支持格和协议能力哈希投影。
- `cargo fmt --all` 与 `cargo test -p hal100-infra --lib backend_discovery`（2 项）通过；随后
  项目级 `pnpm check` 也完整通过（Infra 223 项，216 通过、7 忽略）。正式支持矩阵和空验收
  账本未改变，三平台真实服务验收仍是新增引擎晋级的必要条件。

## 44. 适配器合同版本源收口（2026-08-28）

- 新增 Protocol 层常量 `ENGINE_ADAPTER_CONTRACT_REVISION`，作为 `engine-contract-v1` 的唯一
  Rust 合同版本源；托管 llama.cpp、八个外部适配器、目标构造、运行方案授权、桌面候选和测试
  夹具均改为引用该常量，减少裸字符串造成的合同版本漂移。
- Protocol 引擎测试（9 项）、RuntimeProfileManager 测试（21 项）和外部适配器测试（12 项）均
  通过；本段改动后的项目级 `pnpm check`、文档一致性、Rust 格式和 `git diff --check` 也全部通过。
- 此项只收紧合同版本一致性，不改变支持矩阵、验收账本或正式支持等级；真实 macOS/Windows/Linux
  服务验收、人工审查导入和逐支持格晋级仍是 Goal 的后续工作。

## 45. 当前暂停记录（2026-08-28 16:17 CST）

- 按用户要求暂停本轮 Goal 的后续实现。当前分支为 `main`，开发基线仍为 `9d26d28` / `v1.0.4`；
  工作区中的既有未提交改动全部保留，不执行清理、commit、merge 或 push，也不进入签名、公证、
  安装包、自动更新或正式升级流程。
- 截至暂停点，默认发现服务已与桌面生产组合根、运行方案管理器共享带审查晋级投影的标准引擎注册表；
  `ENGINE_ADAPTER_CONTRACT_REVISION` 已成为 Rust Protocol 层唯一的适配器合同版本源，托管与外部
  适配器、目标构造、运行方案授权、桌面候选和测试夹具均复用该常量，降低合同版本漂移风险。
- 当前正式支持等级没有变化：Apple Silicon/Metal llama.cpp、既有 macOS Ollama、Apple Silicon/Metal
  MLX-LM 为正式支持；vLLM、MLC LLM、OpenVINO Model Server、SGLang、LMDeploy、TensorRT-LLM
  仍为 `connected`。验收账本 `contracts/inference-engines/v1-acceptance-evidence.json` 仍为空，
  当前主机没有这些引擎的真实服务和固定模型验收条件，因此没有新增可晋级支持格。
- 最近一次全量门禁通过：`pnpm check`、`pnpm docs:check`、`cargo fmt --all -- --check`、
  `git diff --check`；定向回归包括 Backend Discovery 2 项、Protocol Engine 9 项、Runtime Profile
  Manager 21 项和外部适配器 12 项。平台探针的 Linux/Windows x86_64 `cargo check` 也已通过。
- 恢复开发时的首个工程入口已明确：补齐验收运行产物中的取消、失败切换回滚、重启补偿等结构化韧性证据，
  再在原生 macOS/Windows/Linux 主机运行真实服务验收脚本，人工复核并导入脱敏账本，最后按“平台 ×
  架构 × 加速器 × 部署”逐格晋级。Goal 保持 active，尚未宣称规划内引擎全部完成。

## 46. 韧性证据合同与控制面探针（2026-08-28）

- 恢复 Goal 后，验收运行产物和审查账本新增可选的 `resilience` 结构，记录取消、失败切换回滚、
  重启补偿三项布尔检查；普通部分运行仍可省略该对象，但 `verifiedExternal`/`managed` 正式记录
  必须三项均为 `true`。JSON Schema、Rust 序列化边界、正式导入门禁和回归测试已同步，缺失或任一
  失败不会晋级支持格。
- 新增共享控制面探针 `verify_control_plane_resilience`，以确定性本地流服务验证 Gateway 强制切换
  会取消旧上游并记录 `forced_route_switch`，安全切换排空超时会保留原活动路由，运行方案重启恢复会
  清理并补偿持久化 activation journal。该探针只证明 HAL100 事务边界，不伪造外部引擎版本、模型或
  平台证据；八个 live acceptance 入口在显式产物输出前调用它，并新增非忽略集成回归覆盖。
- 最近门禁：`cargo test -p hal100-infra --test resilience_control_plane -- --nocapture` 通过；
  `cargo test -p hal100-infra --tests --no-run`、完整 `pnpm check`、`cargo fmt --all`、
  `pnpm docs:check` 与 `git diff --check` 通过。当前支持矩阵和空验收账本保持不变，原生三平台
  真实服务验收仍未完成。

## 47. 当前暂停记录（2026-08-28 16:41 CST）

- 按用户要求暂停 Goal 的后续开发。当前分支为 `main`，基线仍为 `9d26d28` / `v1.0.4`；
  工作区存在此前积累的未提交变更，全部保留，不执行清理、commit、merge 或 push，也不进入签名、
  公证、安装包、自动更新或正式升级流程。
- 本次暂停前已完成验收合同的结构化韧性门禁：正式 `managed`/`verifiedExternal` 记录必须同时
  通过取消、失败切换回滚、重启补偿三项控制面检查；共享探针和非忽略集成回归已验证 HAL100
  Gateway 与运行方案事务边界。该探针只证明 HAL100 控制面语义，不替代真实引擎、模型、平台和
  加速器证据。
- 最近已知质量门禁全部通过：完整 `pnpm check`、`pnpm docs:check`、`cargo fmt --all -- --check`、
  `git diff --check`，以及 `resilience_control_plane` 集成测试；当前验收账本
  `contracts/inference-engines/v1-acceptance-evidence.json` 仍为空。
- 正式支持矩阵没有变化：Apple Silicon/Metal llama.cpp、既有 macOS Ollama、Apple Silicon/Metal
  MLX-LM 为正式支持；vLLM、MLC LLM、OpenVINO Model Server、SGLang、LMDeploy、TensorRT-LLM
  仍为 `connected`，尚无新的支持格可晋级。
- 恢复开发时从真实服务验收开始：在原生 macOS/Windows/Linux 主机运行统一脚本，人工审查并导入
  脱敏证据，补齐长时稳定性、并发及控制面证据，最后按“平台 × 架构 × 加速器 × 部署”逐格晋级。
  长期 Goal 保持 active，尚未宣称规划内引擎全部完成。

## 48. 验收导入链路回归（2026-08-28）

- 新增 `crates/hal100-infra/tests/acceptance_import.rs`，通过真实调用
  `hal100-engine-acceptance-import` 二进制覆盖“运行产物 → 候选账本”的边界，而不是只测内存
  构造器：缺失三项控制面韧性证据时导入失败、不创建输出文件、源账本保持字节不变；补齐完整
  韧性证据后，候选账本成功写出并通过标准 vLLM 适配器、支持格和协议能力哈希复核。
- 该回归证明导入 CLI 的原子性和正式晋级门禁已经接通，但使用的是脱敏构造夹具，不构成真实
  vLLM 服务、模型版本、平台或加速器验收；正式支持矩阵与空账本因此保持不变。
- 定向命令 `cargo test -p hal100-infra --test acceptance_import -- --nocapture` 通过。Goal 仍保持
  active；恢复时继续执行原生三平台真实服务验收和人工审查导入。

## 49. 正式账本 JSON Schema（2026-08-28）

- 新增 `contracts/inference-engines/v1-acceptance-evidence.schema.json`，把正式账本的适配器、支持
  格、指纹、证据来源和状态枚举固化为独立的 Draft 2020-12 合同；对 `managed`/`verifiedExternal`
  记录通过条件分支要求不可变模型修订、主机摘要、七类证据及三项均为 `true` 的韧性对象。
- `scripts/check-doc-consistency.mjs` 现在同时检查账本 schema 版本、正式韧性字段和七类证据数量，
  形成数据文件、Rust 解析器与文档 schema 的最小漂移门禁。Rust 运行时校验仍是最终权威，JSON
  Schema 不承载支持格唯一性、适配器哈希匹配等跨文件约束。
- `pnpm docs:check`、`cargo fmt --all -- --check` 与 `git diff --check` 通过；该项只加强合同和
  导入可审计性，不改变当前空账本及引擎正式支持等级。

## 50. 导入合同全量门禁（2026-08-28）

- 新增账本 schema 与 CLI 集成回归后重新执行项目级 `pnpm check`，结果通过：Biome 109 文件、前端
  TypeScript、Agent Kernel 34 项、Desktop 26 项、Core 54 项、Desktop Rust 99 项（16 忽略）、
  Infra 217 项（7 忽略）、Platform 15 项、Protocol 38 项、Clippy、构建与全部 doc-tests 均通过；
  `acceptance_import` 集成测试和 `resilience_control_plane` 集成测试均通过。
- 该门禁只证明代码合同、导入原子性和控制面探针在当前开发机可重复；由于真实引擎服务、固定模型
  修订和跨平台主机仍未准备，验收账本继续保持零记录，Goal 不宣称任何新增引擎已完成正式支持。

## 51. 真实服务预检（2026-08-28）

- 当前开发机预检结果：`ollama`、`mlx_lm`、`mlc_llm`、`vllm`、`sglang`、`lmdeploy`、
  `trtllm-serve` 和 `ovms` 均不在 PATH；约定的回环端口 `8000`、`8001`、`8080`、`11434`、
  `23333`、`30000` 全部关闭，验收账本记录数为 `0`。
- 因此本轮没有运行任何真实引擎请求，也没有生成或导入人工验收记录；这一结果确认当前阻塞是
  外部验收环境尚未准备，而不是代码门禁失败。恢复时需在原生 macOS、Windows、Linux 主机分别
  准备固定版本/模型/加速器服务，再执行统一脚本并人工审查产物。

## 52. Windows 验收入口（2026-08-28）

- 新增 `scripts/run-engine-live-acceptance.ps1`，与 Bash 白名单脚本保持同一组八个引擎映射、显式
  `HAL100_RUN_REAL_ACCEPTANCE=1` 确认、引擎专属确认变量、create-new 输出和“只运行已准备服务、不
  启动/安装/下载/停止/重配置”的边界；Windows 操作者不再必须安装 Git Bash 才能执行验收入口。
- `scripts/check-doc-consistency.mjs` 会检查 PowerShell 脚本包含完整八个引擎入口，避免 Bash 与
  PowerShell 映射漂移。当前开发机没有 PowerShell 解析器，未宣称 Windows 脚本运行验证；真实
  Windows 主机验收仍需在原生环境执行。
- `pnpm docs:check`、`pnpm lint` 与 `git diff --check` 通过。该项只完善跨平台验收操作面，不改变
  真实服务未准备、空账本和当前引擎支持等级。

## 53. 三平台入口文档收口（2026-08-28）

- `README.md`、`docs/CURRENT_STATE.md` 与 `docs/INFERENCE_ENGINE_SUPPORT_PLAN.md` 已同步说明
  macOS/Linux 使用 Bash、Windows 使用 PowerShell 的验收入口；文档一致性脚本会读取 PowerShell
  映射并校验八个引擎名称，避免只更新一套脚本造成操作路径漂移。
- 当前代码与文档门禁通过，真实服务预检仍显示无可用引擎实例；因此本项只完成跨平台验收操作面，
  不改变支持矩阵、空账本或 Goal 的未完成状态。

## 54. 全适配器审查晋级投影（2026-08-28）

- 新增 `crates/hal100-infra/tests/reviewed_registry_projection.rs`，遍历标准注册表的 8 个外部
  适配器（Ollama、vLLM、MLX-LM、MLC LLM、OpenVINO Model Server、SGLang、LMDeploy、
  TensorRT-LLM），为每个适配器构造一个脱敏审查记录并验证精确支持格投影。
- 回归确认：记录必须匹配完整适配器身份和固定协议能力哈希；命中的格才会在内存 manifest 中变为
  `verifiedExternal`，同一适配器的其他平台/架构/加速器/部署格保持原状态。夹具不写入检查账本，
  不构成真实服务验收或正式支持声明。
- `cargo test -p hal100-infra --test reviewed_registry_projection -- --nocapture` 通过；正式矩阵、
  空账本和 Goal 状态保持不变。

## 55. Bash/PowerShell 映射一致性（2026-08-28）

- 文档一致性脚本现在同时读取 Bash 和 PowerShell 验收入口，并对八个引擎名称逐项做白名单检查；
  任一脚本漏掉引擎都会在 `pnpm docs:check` 阶段失败，减少跨平台验收操作漂移。
- `pnpm docs:check`、`pnpm lint` 与 `git diff --check` 通过。该项不改变真实验收账本和支持状态，
  也不替代 Windows 原生主机上的 PowerShell 运行验证。

## 56. 全量回归确认（2026-08-28）

- 在全适配器投影测试和 PowerShell 映射门禁加入后重新执行 `pnpm check`，结果为退出码 `0`：
  前端、Agent Kernel、Core、Desktop、Infra（含 `acceptance_import`、`resilience_control_plane`、
  `reviewed_registry_projection`）、Platform、Protocol、Clippy、构建与 doc-tests 全部通过；
  所有需要真实环境的测试仍按合同保持 `ignored`。
- 全量回归再次证明的是代码路径和合同边界，不是引擎服务可用性。真实服务预检仍为空，验收账本仍
  为零记录，规划中的新增引擎继续保持 `connected`，Goal 仍未完成。

## 57. 当前暂停记录（2026-08-28 17:04 CST）

- 按用户要求暂停当前 Goal 的后续实现，并记录工作现场。当前分支为 `main`，基线仍为 `9d26d28` /
  `v1.0.4`；工作区包含此前积累的未提交变更，全部保留，不执行清理、commit、merge 或 push。
- 截至本次暂停，正式支持链路已具备：九类引擎适配器的类型化注册表与固定协议能力哈希、支持格与
  实例/来源/配置指纹、运行方案保存/读取/预检/激活/回滚/重启补偿、验收产物原子导入、审查账本
  JSON Schema、控制面韧性探针、全适配器晋级投影回归，以及 macOS/Linux Bash 和 Windows
  PowerShell 两套真实验收入口。
- 最近一次完整质量门禁为通过：`pnpm check`、`pnpm docs:check`、`pnpm lint`、`cargo fmt --all --
  --check`、`git diff --check`；相关定向回归（验收导入、控制面韧性、全适配器审查投影）均通过。
- 正式支持等级没有变化：Apple Silicon/Metal llama.cpp、既有 macOS Ollama、Apple Silicon/Metal
  MLX-LM 为正式支持；vLLM、MLC LLM、OpenVINO Model Server、SGLang、LMDeploy、TensorRT-LLM
  仍为 `connected`。`contracts/inference-engines/v1-acceptance-evidence.json` 仍为零记录，
  当前主机没有真实引擎服务，未产生可晋级的外部验收证据。
- 已确认的剩余主线：在原生 macOS、Windows、Linux 主机准备固定版本/模型/加速器，运行对应验收
  入口并人工审查脱敏产物；补齐长时稳定性、并发及控制面证据后导入账本，按“平台 × 架构 × 加速器
  × 部署”逐格晋级。恢复前不应把夹具或控制面探针结果当作真实引擎支持证明。
- Goal 继续保持 `active`，本次只是暂停，不代表规划内全部引擎已完成正式支持。

## 58. Pi 引擎能力投影（2026-08-28）

- 为补齐“Pi 能安全理解引擎选择”这一跨层断点，`AgentRuntimeCatalog` 新增 Rust 生成的
  `engineCapabilities` 脱敏投影。每个适配器包含稳定身份、所有权、协议/模型格式、平台/架构/
  加速器支持格、当前宿主兼容性、支持等级、证据进度、已保存方案数量和确定性推荐理由。
- 投影不包含 API 根、模型摘要、文件路径、命令、凭据或外部进程细节；Pi 仍只能复制精确
  `profileId` 请求 Rust 实时预检，不能凭静态摘要直接激活或改变路由。
- 运行方案最小身份同步带出适配器变体与合同修订，保证 Pi 可将能力投影与保存方案精确关联；
  Protocol 序列化回归、Desktop 全注册表投影/脱敏回归和“支持哪些推理引擎”路由回归均通过；
  随后完整 `pnpm check` 也以退出码 `0` 通过。现有 20 工具、RPC v13 和任务权限边界未扩大，
  正式支持矩阵与空验收账本保持不变。

## 59. 当前暂停记录（2026-08-28 17:31 CST）

- 按用户要求暂停“规划内全部推理引擎正式支持”Goal。暂停只冻结后续开发推进，Goal 仍保持
  `active`，不将当前阶段误报为全部完成。
- 工作区和分支现场保持不变：当前分支为 `main`，HEAD 为 `9d26d28`（`v1.0.4`，与
  `origin/main` 同步）；已有未提交变更和新增文件均属于此前开发积累，暂停期间不清理、不回滚、
  不执行 commit、merge 或 push。
- 最近一次完整质量门禁仍为通过：`pnpm check` 退出码 `0`，并包含 Biome、TypeScript、Agent
  Kernel、Core、Desktop、Infra（含验收导入/控制面韧性/全适配器审查投影）、Platform、Protocol、
  Clippy、构建和 doc-tests；`pnpm docs:check`、`pnpm lint`、`cargo fmt --all -- --check` 与
  `git diff --check` 亦通过。
- 已完成的架构主线保持可用：九类引擎适配器注册表和类型化支持格、实例/来源/配置证据模型、
  运行方案保存/读取/预检/激活/回滚/重启补偿、控制面韧性探针、脱敏验收产物原子导入、审查账本
  JSON Schema、三平台验收入口，以及 Pi 的静态引擎能力投影和精确方案身份关联。
- 当前正式支持等级不变：Apple Silicon/Metal llama.cpp、既有 macOS Ollama、Apple Silicon/Metal
  MLX-LM 为正式支持；vLLM、MLC LLM、OpenVINO Model Server、SGLang、LMDeploy、TensorRT-LLM
  仍为 `connected`。`contracts/inference-engines/v1-acceptance-evidence.json` 仍为零记录，
  当前开发机没有真实引擎服务。
- 恢复开发的第一步应是外部验收准备与执行：在原生 macOS、Windows、Linux 主机准备固定版本、
  固定模型和目标加速器，运行 `scripts/run-engine-live-acceptance.sh` 或
  `scripts/run-engine-live-acceptance.ps1`，人工审查脱敏产物，补齐长时稳定性、并发和控制面证据，
  再导入账本并按“平台 × 架构 × 加速器 × 部署”逐格晋级。夹具、静态投影和本机预检不能替代真实
  服务证据。

## 60. 支持覆盖报告与陈旧记录门禁（2026-08-28）

- 新增 `hal100-engine-support-report` 只读命令和 `build_support_coverage_report`，按八个外部适配器
  与 HAL100 托管 `llama.cpp` 的九类适配器及精确支持格汇总 manifest 状态、匹配账本状态、七类证据
  摘要、正式格缺失账本数和严格晋级信号；
  报告不包含端点、模型身份、路径、命令、凭据或运行观察值。
- 报告把匹配的正式账本记录投影为 `effectiveStatus`，但不修改注册表或授予激活权限；弱状态记录
  不会提升支持格。任何适配器或支持格未声明的账本记录都会直接失败，避免操作员看到被静默忽略的
  陈旧证据；正式记录的状态还必须与适配器所有权匹配（外部适配器只能是
  `verifiedExternal`，托管适配器只能是 `managed`），否则报告直接失败。
- `--json` 输出版本化、拒绝未知字段的机器可读报告，`--ledger` 检查候选账本，`--strict` 在仍有
  非正式支持格或正式外部格缺少账本记录时返回退出码 2。托管格由 HAL100 自有 manifest、供应链和
  生命周期证据满足，不要求伪造外部服务记录。当前标准账本仍为空，新增命令因此只提供缺口可视化
  和导入前门禁，不改变任何引擎的正式支持等级。
- 严格报告 CLI 现在从可执行外部适配器注册表取得每个变体的 canonical 协议能力哈希，并在账本
  记录命中支持格时复核；缺少基线或哈希漂移会在统计“可晋级”前失败关闭。
- `cargo test -p hal100-infra engine_support_report -- --nocapture`、`cargo fmt --all`、
  `pnpm docs:check` 与 `git diff --check` 已通过；随后完整 `pnpm check` 以退出码 `0` 通过，
  严格报告在当前空账本下按预期返回退出码 `2`（28 个支持格中 4 个正式、24 个待验收）。真实
  三平台服务验收仍是后续晋级的必要条件；报告的 7 项回归覆盖陈旧记录、所有权状态漂移、协议
  能力哈希漂移，以及缺少外部哈希基线时的故障关闭；托管格无需外部账本的语义另有回归覆盖。
- Ollama 真实验收入口已从 Apple Silicon 专用扩展为 manifest 驱动的四格入口：macOS/aarch64
  CPU/Metal、Windows x86_64 CPU、Linux x86_64 CPU。操作者必须显式选择 `cpu|metal`，测试再用
  当前编译平台、架构、原生宿主探针和 manifest 共同校验；新增非 ignored 映射回归确保四个声明格
  都有可执行入口，未声明的 Windows CUDA 或 macOS x86_64 CPU 组合被拒绝。

## 61. 当前暂停记录与 Intel Mac 探针现场（2026-08-28 18:32 CST）

- 按用户要求暂停后续开发。Goal 保持 `active`，当前分支仍为 `main`，HEAD 为 `9d26d28`
  （`v1.0.4`，与 `origin/main` 同步）；保留工作区全部既有修改和新增文件，不执行清理、回滚、
  commit、merge 或 push。
- 最近一个已经完整验证的节点是第 60 节的支持覆盖报告、协议能力哈希门禁和 Ollama 四支持格验收
  入口：完整 `pnpm check` 当时以退出码 `0` 通过；严格覆盖报告在空账本下按预期以退出码 `2`
  结束，统计仍为 9 类适配器、28 个支持格、4 个正式格、24 个待验收格。
- 暂停前刚开始修复 MLC LLM 的 macOS x86_64/Metal 支持格无法通过原生宿主探针抵达的问题。
  当前工作区已加入一版保守实现：Apple Silicon 继续确定性报告 CPU/Metal；Intel Mac 仅在有界、
  只读的 `system_profiler SPDisplaysDataType -json -detailLevel mini` 输出明确包含
  `spdisplays_mtlgpufamilysupport=spdisplays_metal*` 时报告 Metal，不根据 GPU 名称或机型猜测，
  命令失败、输出超过 256 KiB 或 JSON 异常时故障关闭；同时加入了解析级回归草稿和 macOS 目标
  `serde_json` 依赖。
- 恢复后已完成该探针的第一轮验证：`cargo fmt --all`、`cargo test -p hal100-platform --
  --nocapture`（16 项通过、1 项按合同 ignored）以及 `cargo clippy -p hal100-platform --all-targets
  -- -D warnings` 均通过；解析回归覆盖明确能力键、缺键、畸形 JSON 和 256 KiB 输出上限。
- 探针语义已同步到 `CURRENT_STATE.md`、`INFERENCE_ENGINE_SUPPORT_PLAN.md` 与 README；它只让
  预留的 MLC LLM macOS x86_64/Metal 支持格可抵达真实验收，不改变当前不支持 Intel Mac 的产品
  边界，也不能据此晋级支持格。随后 `pnpm docs:check`、`git diff --check` 与完整 `pnpm check`
  均以退出码 `0` 通过；完整门禁包含前端 26 项、Agent Kernel 34 项、Core 54 项、Desktop 100 项、
  Infra 224 项、Platform 16 项、Protocol 39 项及集成测试、Clippy、构建和 doc-tests。
- 严格覆盖报告复核结果保持为 9 类适配器、28 个支持格、4 个正式格、24 个待验收格、0 条账本
  记录与 `strictReady=false`，`--strict` 按合同返回退出码 `2`。因此仍必须取得 Intel Mac 真机和
  真实 MLC LLM 服务证据，不能用解析测试替代外部验收，也没有发生隐式支持晋级。
- 正式支持等级和验收账本没有变化：HAL100 托管 llama.cpp、既有 macOS Ollama、Apple
  Silicon/Metal MLX-LM 仍是正式支持；vLLM、MLC LLM、OpenVINO Model Server、SGLang、
  LMDeploy、TensorRT-LLM 仍为 `connected`，标准验收账本仍为零记录。长期 Goal 尚未完成。

## 62. 全外部适配器验收入口可达性门禁（2026-08-28）

- 将 Ollama、vLLM、MLX-LM、MLC LLM、OpenVINO Model Server、SGLang、LMDeploy 与
  TensorRT-LLM 八个真实服务测试统一到 `select_declared_acceptance_cell`。选择器只接受 Protocol
  已知的平台、架构和加速器存储键，并要求精确命中当前适配器 manifest 的本地支持格；未知或未声明
  坐标故障关闭，未来远程部署必须建立独立目标和证据合同。
- 新增非 ignored `acceptance_entry_coverage` 集成回归，遍历标准外部注册表的 8 个适配器与 27 个
  支持格，证明每个声明格都能由真实验收入口选择；同时覆盖未知平台、未知架构与未声明加速器拒绝。
  这消除了 manifest 扩张后静默留下“无法生成验收证据”的死格，但不证明服务或加速器实际存在。
- `cargo test -p hal100-infra --test acceptance_entry_coverage -- --nocapture` 两项通过；所有 Infra
  集成测试目标 `--no-run` 编译通过，`cargo clippy -p hal100-infra --tests -- -D warnings` 通过。
  正式支持矩阵、空验收账本与严格覆盖统计均未因该入口重构而改变。

## 63. 跨平台准备度与 Intel Mac 进程边界（2026-08-28）

- 当前主机未发现八个外部引擎的可执行文件，8000、8001、8080、11434、23333、30000 等约定回环
  端口全部关闭，验收账本仍为零记录；因此本轮没有可诚实生成的真实服务证据，也未自动安装引擎或
  下载模型。
- `hal100-platform` 的 `x86_64-unknown-linux-gnu` 与 `x86_64-pc-windows-msvc` 交叉
  `cargo check` 均通过，Bash 验收脚本语法检查通过；仓库三平台 GitHub Actions 源码门禁继续负责在
  原生 macOS、Ubuntu、Windows runner 上执行完整 `pnpm check`。
- Infra 集成测试交叉检查在 HAL100 源码编译前分别停于宿主缺少 `x86_64-linux-gnu-gcc` 与 Windows
  SDK 头文件，均由 `aws-lc-sys` 构建脚本报告；这是当前 macOS 开发机的交叉 C 工具链限制，不能
  冒充 Linux/Windows 原生通过，也不是适配器源代码失败。
- Intel Mac 的 `system_profiler` 调用从“结束后检查长度”改为真正的进程/流双重边界：5 秒超时会
  终止子进程，stdout 最多 256 KiB、stderr 最多 16 KiB，读取线程持续排空管道以避免子进程阻塞。
  成功、输出溢出与超时回归均通过；`hal100-platform` 17 项测试和 `-D warnings` Clippy 通过。

## 64. 历史无账本正式格单向收敛门禁（2026-08-28）

- 生产组合根加载审查账本进行精确格晋级，但为保留 v1 账本建立前的真实验证结果，会继续保留基础
  manifest 中 MLX-LM 的 1 个和 Ollama 的 2 个 `verifiedExternal` 格；严格覆盖报告因此诚实显示
  3 个正式外部格缺少新账本记录。
- 新增非 ignored 债务棘轮：从标准注册表、托管 manifest、canonical 协议能力哈希和标准账本构建
  覆盖报告，精确提取无账本正式外部格。集合可以在历史证据迁入后缩小，但不得超出上述三格；因此
  未来直接把其他 manifest 支持格改成 `verifiedExternal` 会在 CI 失败，必须走真实产物、人工审查、
  原子导入和协议哈希门禁。
- `acceptance_entry_coverage` 现为 3 项通过，相关定向 Clippy 以 `-D warnings` 通过。该棘轮不是
  对历史证据的替代，长期完成条件仍包括把三格补入正式账本并最终将债务集合清零。

## 65. 当前暂停记录（2026-08-28 19:08 CST）

- 按用户要求暂停“规划内全部推理引擎正式支持”Goal；Goal 状态继续保持 `active`，本次暂停不代表
  目标完成，也不把架构预留、静态可达性或测试夹具误报为真实引擎正式支持。
- 暂停现场保持在 `main` 分支，HEAD 为 `9d26d28`（`v1.0.4`，与 `origin/main` 同步）。工作区
  包含此前持续开发形成的修改和新增文件，本次不执行清理、回滚、commit、merge 或 push。
- 本轮已关闭三项架构准备缺口：27 个外部支持格统一通过 manifest 驱动的真实验收选择器抵达；
  `hal100-platform` 已通过 Linux x86_64 与 Windows x86_64 目标交叉检查；Intel Mac 的 Metal
  探针具备 5 秒进程超时、stdout 256 KiB 和 stderr 16 KiB 的流式边界，并在失败时关闭能力。
- 新增历史证据债务棘轮后，未进入 v1 账本的正式外部格只能从既有三格集合单向缩小，不能通过手工
  修改 manifest 扩张；现有三格仍须由真实历史证据迁入账本后清零，棘轮本身不是验收证据。
- 暂停前最终完整 `pnpm check` 以退出码 `0` 通过，随后 `git diff --check` 也通过。当前严格覆盖
  状态保持为 9 类适配器、28 个支持格、4 个正式格、24 个待验收格、3 个正式外部格缺少 v1 账本、
  0 条账本记录和 `strictReady=false`；没有发生支持等级晋级。
- 恢复开发时应从真实平台服务证据开始：在原生 macOS、Windows、Linux 主机按支持格准备固定
  引擎版本、固定模型和加速器，运行对应 live acceptance，人工审查脱敏产物并补齐稳定性、并发与
  控制面证据，再原子导入账本、逐格晋级。当前开发机没有外部引擎可执行文件或监听服务，因此无需
  重复静态架构建设，也不能在缺少真实服务的情况下推进正式等级。

## 66. 全支持格审查晋级管线覆盖（2026-08-28）

- 恢复后继续审计发现：既有门禁已分别证明 27 个外部支持格能抵达 live acceptance，以及单个
  vLLM 产物能通过离线导入，但尚未证明每个引擎/平台格都能贯通完整的审查晋级管线。
- `acceptance_entry_coverage` 新增全矩阵结构回归：逐格构造完整但仅存在于测试内存的运行产物，
  依次验证产物合同、人工提供的模型不可变修订、正式记录转换、原子账本追加、适配器 canonical
  协议能力哈希、严格审查注册表投影和七类证据完整性。
- 27 个外部格全部通过；再与 HAL100 托管 llama.cpp manifest 合并时，候选严格覆盖报告达到
  28/28 正式、0 待验收、0 个正式格缺账本和 `strictReady=true`。这证明未来真实产物不会因某个
  支持格缺少导入或投影接线而卡住。
- 该回归不连接服务、不读取真实硬件、不写入 `v1-acceptance-evidence.json`，因此当前正式状态仍是
  4/28、标准账本仍为 0 条、`strictReady=false`。定向测试现为 4 项通过，定向 Clippy 的
  `-D warnings` 与 `git diff --check` 均通过；真实三平台服务证据仍是唯一剩余晋级来源。

## 67. 当前暂停记录（2026-08-28 19:15 CST）

- 按用户要求再次暂停长期 Goal；Goal 状态已核对为 `active`，没有标记完成或阻塞。暂停后不再继续
  设计宿主预检/维护者入口，也未安装、启动、停止、下载或重配置任何外部推理引擎。
- 本轮实际新增内容仅为全矩阵结构门禁及其文档同步：27 个外部支持格都能贯通运行产物校验、人工
  模型修订、原子账本追加、协议能力哈希复核、严格注册表投影和候选覆盖报告；该门禁使用内存夹具，
  不会修改标准账本或授予运行权限。
- 完整 `pnpm check` 已在本轮改动后以退出码 `0` 通过，包含文档、Biome、TypeScript、前端 26 项、
  Agent Kernel 34 项、Core 54 项、Desktop 100 项、Infra 224 项及集成测试、Platform 17 项、
  Protocol 39 项、Clippy、构建和 doc-tests；最终 `git diff --check` 同样通过。
- 产品支持状态保持不变：9 类适配器、28 个支持格、4 个正式格、24 个待验收格、3 个历史正式外部格
  尚未迁入 v1 账本、标准账本 0 条、`strictReady=false`。结构候选能达到 28/28 只证明接线完整，
  不表示任何待验收格已获得真实服务证据。
- 工作现场继续保留在 `main` / `9d26d28`（`v1.0.4`，与 `origin/main` 同步）的未提交工作区；
  本次没有执行 commit、merge、push、清理或回滚。恢复时直接从原生 macOS、Windows、Linux 真机
  服务验收、人工审查和逐格账本导入继续。

## 68. Ollama macOS Metal 首条真实 v1 账本记录（2026-08-31）

- 在 Apple M1 / 16 GiB 的原生 macOS aarch64/Metal 主机上，从 Ollama 官方发布资产准备隔离的
  `0.33.2` 临时运行时；下载资产 SHA-256 与官方发布元数据一致。复用 HAL100 已验证的固定
  `Qwen3.5-2B-Q4_K_M.gguf`，文件 SHA-256 为
  `aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223`，模型仓库 revision 固定为
  `f6d5376be1edb4d416d56da11e5397a961aca8ae`，没有修改用户模型文件。
- 首轮资格检查真实暴露 Qwen3.5 默认思考会耗尽固定 64 Token 工具调用预算。依据 Ollama OpenAI
  兼容合同，资格选项新增只允许 `Disabled` 的类型化 reasoning 状态，并由 Ollama 适配器统一向
  协议和稳定性探针映射为 `reasoning_effort: "none"`。没有提高 Token 预算、降低唯一工具调用要求，
  也没有向其他引擎注入任意请求体；单元回归精确断言 unary、stream 和20次稳定性请求都携带该值。
- 临时服务最初使用 `keep-alive=0`，协议检查后并发稳定性波次在卸载/重载窗口按合同失败，未生成
  产物。改为固定保留模型10分钟后，在不改变20次、每波并发4和120秒上限的前提下重新运行，完整
  live acceptance 通过：版本/目录、单工具调用、SSE/usage、原生宿主支持格、运行方案生命周期、
  取消/失败切换回滚/重启补偿均通过，稳定性20/20，最大观测延迟445 ms。
- 脱敏产物 `acceptance-run-6ee9f5dfe2e14192802a72f33714a3eb` 经字段审阅，确认不含端点、路径、
  提示词、响应或凭据；离线导入器把模型关联摘要替换为实际不可变 revision，候选文件与最终标准
  `v1-acceptance-evidence.json` 字节一致。标准账本现有1条记录，精确覆盖
  `Ollama × macOS × aarch64 × Metal × local`；正式格仍为4/28、待验收24/28，历史无账本正式格
  从3缩小为2，`strictReady=false`，没有向 Ollama CPU、Windows 或 Linux 外推。
- 导入过程中两处“账本必须为空”的测试假设被改为增量合同：检查仓库内每条记录都映射标准适配器
  支持格和 canonical 协议哈希，导入集成测试按现有记录数追加并定位新记录。Infra 单元224项通过，
  `acceptance_entry_coverage` 4项和 `acceptance_import` 1项通过；随后完整 `pnpm check` 退出码为0，
  覆盖文档、Biome、TypeScript、前端26项、Agent Kernel 34项、Core 54项、Desktop 100项、Infra、
  Platform 17项、Protocol 39项、Clippy、构建与 doc-tests。
- 验收后临时 Ollama 服务和 runner 已停止，11434 不再监听；两次由临时运行时新建的用户身份目录
  均完整移入废纸篓并可恢复，未覆盖已有目录。详细边界与复核来源见
  `docs/benchmarks/2026-08-31-ollama-0.33.2-macos-metal.md`。当前分支/基线仍为 `main` / `9d26d28`
  （`v1.0.4`）；没有 commit、merge 或 push。

## 69. MLX-LM macOS Metal 真实 v1 账本迁入（2026-08-31）

- 在同一原生 Apple M1 / 16 GiB 主机使用完全位于系统临时目录的 Python 3.12 隔离环境，安装官方
  `mlx-lm==0.31.3` 与 MLX `0.32.2`；PyPI wheel SHA-256 记录为
  `758cfddf1180053b7613db76fad3d246a331a2a905808e1164a275621fc983b8`。固定模型
  `mlx-community/Qwen3.5-2B-4bit@674aaa7240b91e8012fcad5d791b7dfe5ba90207` 的
  `model.safetensors` 大小和 SHA-256 均与固定 revision 官方元数据一致。
- tokenizer 预检确认聊天模板、工具调用和思考控制均存在；临时服务离线、只监听回环地址并设置
  prompt/decode concurrency 为4。隔离 HF cache 根不存在时，MLX-LM `/v1/models` 暴露上游
  `CacheNotFound`；仅创建空临时 `hub` 目录后恢复，HAL100 没有放宽有效 JSON/模型目录标准。
- 最小请求和完整 live acceptance 均通过。系统指纹
  `0.31.3-0.32.2-macOS-26.5-arm64-arm-64bit-applegpu_g13g` 绑定引擎、MLX、宿主和 GPU 架构；完整
  验收覆盖健康/目录、唯一工具调用、SSE/usage、运行方案生命周期、20次/并发4稳定性和三项控制面
  韧性，最大观测延迟903 ms。
- 脱敏产物 `acceptance-run-6a5e1d232cb644b2a7ba7db85b1b910c` 经审阅并以模型固定 commit 导入
  标准账本。账本现为2条记录；正式格仍为4/28、待验收24/28，历史无账本正式格由2缩小为1，只剩
  Ollama macOS/aarch64/CPU，`strictReady=false`。详细边界见
  `docs/benchmarks/2026-08-31-mlx-lm-0.31.3-macos-metal.md`。
- 验收后8080服务已停止且无残留进程；临时运行时、模型、HOME 与 cache 未写入用户配置或项目目录。
  尚未在第二条记录导入后执行完整 `pnpm check`，下一步先完成账本/覆盖定向回归和全量门禁，再处理
  最后一格历史 Ollama CPU 证据及后续新增引擎支持格。

## 70. Ollama 实际加速器证明与历史账本债务清零（2026-08-31）

- `EngineQualificationReport` 新增向后兼容的 `observedAccelerator`。运行方案管理器不再把“宿主拥有
  某加速器”和“目标模型实际在该加速器执行”混为一谈：若同一平台、架构、部署存在多个正式
  加速器格，资格报告必须给出与用户选择完全一致的运行时观察；保存、外部重验、激活计划、执行前
  复验和切换后复验统一使用该规则。只有 manifest 在该精确坐标下唯一正式加速器时，缺少设备端点
  的引擎才可由合同唯一性证明。
- Ollama 资格请求完成后读取官方 `/api/ps` 并精确匹配仍驻留的模型；`size_vram=0` 映射为 CPU，
  回环 macOS 上非零设备内存映射为 Metal。部署指纹升级到 v2，把观察到的加速器键与系统指纹、
  模型 ID 一同哈希。单元回归覆盖 CPU、Metal、模型缺失和多正式加速器格的缺证/错配拒绝。
- 该保护真实捕获了一次调度漂移：CPU 变体仍驻留时直接请求 Metal 标签，Ollama 复用了 CPU runner；
  `/api/ps` 没有与 Metal 标签匹配的运行模型，验收按合同失败且没有产物。显式卸载 CPU runner 后，
  Metal 模型观察到 `size_vram=1,395,035,994`，才重新通过资格与完整验收。
- 使用同一官方 Ollama `0.33.2` 和固定
  `Qwen3.5-2B-Q4_K_M.gguf@f6d5376be1edb4d416d56da11e5397a961aca8ae` 完成 macOS/aarch64/CPU
  真实验收。CPU Modelfile 固定 `num_gpu=0`，`/api/ps` 为 `size_vram=0`，日志为0/25层 GPU offload；
  完整协议、运行方案生命周期、20次/并发4稳定性及三项控制面韧性通过，最大延迟365 ms，产物为
  `acceptance-run-6a23349a43514f41bf8bfa8b811ac69f`。
- Metal 在新证明合同下重新验收，20/20、并发4、最大延迟648 ms，产物为
  `acceptance-run-c3c9f2742e584ab19b613d32255bea82`。离线导入器新增显式
  `--replace-record-id`：只能用新运行替换同一适配器和完全相同支持格的旧记录，源账本不变，仍只
  写 create-new 候选；跨格替换原子拒绝。候选经字段、路径和脱敏审阅后与最终账本字节一致。
- 标准账本现有3条记录：Ollama CPU、Ollama Metal、MLX-LM Metal。覆盖报告为9类适配器、28个
  支持格、4个正式格、24个待验收格、3条外部记录、0个正式格缺账本，三个历史债务全部清零且
  `allFormalCellsLedgerBacked=true`；`strictReady=false` 仍是正确结果，因为其余24格尚未完成真机
  验收，债务清零不等于长期 Goal 完成。
- 临时 Ollama HOME、身份密钥、模型标签和运行产物均位于系统临时目录；服务与 runner 已停止，
  11434 无监听，用户 `.ollama` 不存在。完整 `pnpm check` 以退出码0通过：文档一致性、Biome
  109文件、TypeScript、Desktop前端26项、Agent Kernel 34项、Core 54项、Desktop Rust 100项
  通过/16项忽略、Infra 226项通过/7项忽略、Platform 17项、Protocol 39项、全部集成测试、
  Clippy、Agent Kernel构建与doc-tests均无失败。当前没有 commit、merge 或 push，也未触及签名、
  公证、安装包、自动更新或正式升级流程。

## 71. MLC LLM Apple Metal 真实验收与故障关闭（2026-08-31）

- 审计官方最新源码与本机服务后确认：MLC LLM REST没有稳定包版本端点，Chat响应中的
  `system_fingerprint`在当前官方实现中固定为空。原适配器只在该字段非空时生成部署指纹，使正式
  运行方案在真实官方服务上不可达；该格继续保持`connected`，没有用伪造值或模型名提前晋级。
- 在完全位于系统临时目录的Python 3.13环境实测官方macOS arm64包。官方索引默认解析出的
  `mlc-llm-nightly-cpu 0.26.dev6 + mlc-ai-nightly-cpu 0.26.dev246`还存在TVM/MLC编译器接口漂移；
  官方稳定`mlc-llm-cpu 0.20.0.dev0 + mlc-ai-cpu 0.20.0`配合`apache-tvm-ffi 0.1.11`可以正常
  导入并识别`metal:0`。固定
  `SmolLM2-360M-Instruct-q4f16_1-MLC@3a622fd89e0216e8bb10c410c007c786baa8a033`
  后，JIT生成Metal动态库并启动回环服务，真实`/v1/models`、Chat和Usage通过；该小模型只用于
  安装与Metal运行预检，不具备HAL100所需工具能力，不能生成正式验收记录。
- MLC适配器改为可达的部署内容合同：正式资格只接受服务目录暴露的绝对本地模型目录，Rust有界
  校验目录边界、最多4096个文件、4 MiB单份元数据和128 GiB部署总量，并以SHA-256流式覆盖
  `mlc-chat-config.json`、`tensor-cache.json`/`ndarray-cache.json`、每个声明权重分片及全部
  tokenizer文件。相对路径、`HF://`、目录穿越、缺失/重复分片、大小漂移或读中变化均故障关闭；
  指纹进入运行方案保存、复验和激活，不再依赖空`system_fingerprint`。
- 新增并完成6项MLC适配器回归，覆盖空官方指纹下的协议+本地部署绑定、权重内容变化导致指纹
  变化、相对/穿越路径拒绝、重复/大小不符分片拒绝、恰好一个工具模板插槽及非MLC目录所有者
  拒绝。真实部署进一步暴露并修复了多行`system_template`被通用单行文本校验误拒的问题：现在只
  允许64 KiB内的CR/LF/TAB，继续拒绝空值、NUL、其他控制字符和超限文本。
- Ministral-3-3B候选因不能稳定复制完整工具名被正式探针拒绝。第二个候选固定为
  `mlc-ai/Qwen3.5-2B-q4f16_1-MLC@dd74e9c8a20c4546df85c844103bff87b6dcacad`；31个权重分片和
  tokenizer共32个LFS对象全部校验官方SHA-256，HAL100工具模板配置SHA-256为
  `0f44e9d538e8eb94556d3b2eb834c46818f5c07822c196d542bb2b3208233cd9`，固定Metal动态库SHA-256为
  `2abf995a99a3a5cadc645a7412fb50f9044b36d7a7d5d1b58212222d1d7331e4`。
- 该固定候选真实通过单次目录、本地部署指纹、精确`hal100_protocol_probe`、普通流式、Usage和
  Gateway非流式参数标准化。随后官方0.20运行时在工具请求后的后续请求中，于串行、并发以及
  `local`/`interactive`模式均可复现scheduler rollback内部检查失败并终止后台线程；降为单并发或
  在Gateway增加单飞无法解决。官方0.26 nightly的稳定版动态库存在ABI不兼容，其自身编译器/运行包
  又有TVM接口和TIR表示漂移，因此没有安全相干的替代组合。
- 完整live acceptance按原有20次/每波4并发门槛失败，没有生成运行产物、没有导入账本、没有支持
  晋级；正式统计仍为4格、待验收24格、外部账本3条。MLC macOS/aarch64/Metal继续为`connected`。
  固定包哈希、复现面和停止决定见
  `docs/benchmarks/2026-08-31-mlc-llm-macos-metal-blocked.md`。

## 72. 跨平台真机验收请求前置证明（2026-08-31）

- 审计八个ignored live acceptance入口发现：各入口虽然都使用原生`NativeSystemProbe`，但旧顺序
  是先执行服务目录、协议资格和20次稳定性流量，最后才证明当前宿主命中声明支持格。错误平台、
  架构或加速器因此可能先向真实模型发出无效且昂贵的请求，然后才被拒绝。
- 新增共同`prepare_real_acceptance_host` preflight，把适配器manifest中的精确支持格解析与原生
  宿主平台/架构/加速器证明合并为一个步骤。Ollama、MLX-LM、MLC LLM、OpenVINO、vLLM、SGLang、
  LMDeploy和TensorRT-LLM八个入口全部在构造目标、检查服务或发送推理前调用该步骤；环境变量只能
  选择已声明支持格，不能扩张矩阵或替代硬件证明。
- 八个live acceptance集成目标全部重新编译通过。该改动不生成运行产物、不连接或启动引擎、不
  修改支持状态和账本；正式统计仍为4格、待验收24格、外部账本3条。

## 73. MLC Intel Mac 非产品支持格移除（2026-08-31）

- 复核长期Goal与现行产品边界发现：MLC manifest仍声明macOS/x86_64/Metal `connected`格，历史上
  为让Intel Mac进入真机验收曾补充有界`system_profiler`探针；但当前状态、README和产品范围均
  明确HAL100不支持Intel Mac。把该格保留在“全部规划格最终正式支持”的完成条件中会要求未来正式
  晋级一个产品明确不支持的平台，属于支持矩阵自身漂移。
- 从MLC可执行manifest和版本化`v1-support-matrix.json`同时移除macOS/x86_64/Metal格；保留通用
  macOS x86_64探针实现，不把平台探测能力等同于产品支持声明。Apple Silicon macOS/Metal、
  Windows和Linux的MLC规划不变。
- 外部支持格由27格收敛为26格；合并托管llama.cpp后总计27格，其中4格正式、23格待验收、3条
  外部账本记录且0个正式格缺账本。全矩阵结构夹具相应证明候选可达到27/27。没有删除真实证据、
  没有把待验收格晋级，也没有扩大任何平台承诺。

## 74. OVMS 引擎与设备语义解耦（2026-08-31）

- 复核OpenVINO Model Server官方文档与当前源码确认：`target_device`可在启动参数或本地模型配置中
  取`CPU`、`GPU`、`NPU`等值，但HTTP `/v2`元数据、`/v1/models`、`/v1/config`状态和`/metrics`
  均不返回实际目标设备。原`InferenceAccelerator::OpenVino`把引擎/runtime品牌误当作硬件设备，
  同时折叠Intel GPU与NPU，无法为运行方案提供精确支持格语义。
- Protocol移除该含混加速器，新增`IntelGpu`与`IntelNpu`；Windows宿主探针以有界
  `Win32_VideoController`和`ComputeAccelerator` PCI身份分别报告Intel GPU/NPU，Linux以DRM
  render node与Intel厂商身份报告GPU，并以有界`/sys/class/accel`和`/dev/accel`共同证据报告NPU。
  未知厂商、缺少设备节点或查询失败继续故障关闭为“不具备该候选”，不从环境变量推断硬件。
- OVMS从一个多设备`ovms-openai-server`拆成`ovms-openai-cpu`、`ovms-openai-intel-gpu`和
  `ovms-openai-intel-npu`三个单设备适配器变体。每个变体只声明Windows/Linux x86_64上的一种设备，
  运行方案、协议能力哈希和验收记录绑定完整变体身份；资格报告明确标记较弱的
  `adapterVariantContract`，不伪造官方服务不存在的设备回报。桌面后端配置可明确选择三种OVMS
  设备合同，默认发现只产生CPU候选，避免同一回环服务出现三个未经选择的自动候选。
- SQLite升级到schema v15：新支持格只允许`intel_gpu`/`intel_npu`，旧`openvino`支持格不会猜测
  映射到某种设备，而是整体失效为待修复；旧`ovms-openai-server`后端解除引擎绑定并增加配置修订，
  用户重新选择精确设备变体后才能复验。该开发期schema迁移不涉及安装包、自动更新或正式升级流程。
- OVMS由4个含混格重构并扩展为6个明确格。该阶段矩阵为八个外部引擎、十个外部适配器变体、28个
  外部支持格；合并托管llama.cpp后共29格，其中4格正式、25格待验收、3条外部账本记录且0个正式格
  缺账本。全矩阵结构夹具已证明候选可达到29/29；没有把任何OVMS格提前晋级。
- Protocol 39项、Platform 17项、OVMS适配器3项、schema v15定向迁移测试、4项验收入口覆盖及1项
  审查注册表投影测试均通过。完整`pnpm check`首次发现3个仍假设“每个引擎只有一个适配器”的旧
  测试；改为按完整适配器ID逐变体核对后，完整门禁以退出码0通过，包括文档一致性、Biome、
  TypeScript、前端与Agent Kernel测试、Rustfmt、全工作区Clippy、构建、全部Rust测试和doc-tests。

## 75. 原生真机验收手动编排（2026-08-31）

- 新增`.github/workflows/live-engine-acceptance.yml`，只允许`workflow_dispatch`手动触发，并以静态
  runner标签绑定隔离的`macOS/ARM64`、`Linux/X64`、`Linux/ARM64`和`Windows/X64`四类
  `hal100-acceptance`自托管主机。普通push/PR不会触发真实推理，也不会在无对应设备的托管runner
  上生成伪平台证据。
- 调度公开输入只绑定规划内八个外部引擎、目标平台和加速器storage key；回环API root、模型ID
  （MLC时包含绝对本地部署目录）、审查版本和可选vLLM密钥由对应精确支持格的受保护GitHub
  Environment secrets注入，并在checkout前做不打印值的存在性检查。工作流继续调用既有
  Bash/PowerShell白名单脚本；Rust共同preflight仍以编译平台、架构、原生设备快照和manifest精确
  支持格为最终门禁，工作流输入不能扩张支持矩阵。
- 同一目标平台的验收以concurrency组串行化，避免固定回环服务被两个运行同时使用；成功后只上传
  14天保留的create-new脱敏JSON，产物缺失即失败。流程没有安装、下载、启动、停止或重配置引擎，
  不调用离线导入器，不改账本、不晋级支持状态；25个未验收格的正式状态保持不变。
- 文档一致性检查新增工作流棘轮：固定检查八个引擎入口、三平台源码runner、四类隔离真机runner、
  Unix/Windows白名单脚本、产物缺失故障关闭，以及禁止push/PR自动触发和禁止自动账本导入。

## 76. 真机runner可用性与受保护配置边界（2026-08-31）

- 通过GitHub API只读检查`Drew1811266/HAL100`的Actions runner列表，当前`total_count=0`，尚无任何
  可执行上述工作流的自托管主机。因此本轮不能生成Windows/Linux或第二类Apple硬件证据，也不能
  把25个待验收格中的任何一格晋级；这是一项外部执行条件，不改变软件合同完成度。
- 复核workflow事件可见性后移除`api_root`、`model_id`和`engine_version`三个公开dispatch字段；否则
  MLC绝对部署路径与模型身份会持久化在GitHub事件中。现在Environment名称只由固定choice组合生成，
  目标配置使用`HAL100_ACCEPTANCE_API_ROOT`、`HAL100_ACCEPTANCE_MODEL_ID`、
  `HAL100_ACCEPTANCE_ENGINE_VERSION`和可选`HAL100_VLLM_API_KEY` secrets；除vLLM外不会注入API密钥。
- 四类job均在checkout前验证API root和模型ID非空，除LMDeploy外还要求版本非空；失败时不打印值、
  不运行仓库代码、不连接服务。原生Rust preflight继续负责值的回环URL、有界身份和精确支持格证明。
- 修正架构蓝图第20节长期未更新的勾选状态：已有代码与回归证明的引擎身份、多实例/变体隔离、
  非digest证据、目标来源、缓存授权隔离、激活修订绑定、rollback-only journal、Gateway热路径、
  Desktop/Agent共同服务、未验证平台关闭和既有引擎回归共11项标为完成；“全部文档/迁移/合同/真机/
  故障注入证据齐全”保持唯一未完成项。文档检查固定11/1计数，不能用结构夹具提前勾选真机完成。

## 77. Manifest驱动的工作流支持格预检（2026-08-31）

- 新增`scripts/engine-acceptance-coordinate.mjs`，集中定义八个workflow引擎键、四类runner平台/架构
  坐标和七个加速器storage key到Protocol wire key的映射。解析器只接受schema v1支持矩阵，并要求
  一次选择恰好匹配一个`local`支持格；未知键、矩阵外坐标或跨OVMS变体含混全部失败关闭。
- 新增`scripts/validate-engine-acceptance-coordinate.mjs`严格CLI，只接受`--engine`、
  `--target-platform`、`--accelerator`各一次，读取仓库版本化矩阵并输出不含端点、模型、路径或凭据的
  精确适配器身份。重复、缺失、多余参数和不存在的支持格返回退出码2。
- 真机workflow新增普通`ubuntu-24.04`、5分钟上限、无Environment secrets的`validate-coordinate`
  job；四类自托管job全部通过`needs`依赖它。无效组合因此不会占用真实硬件、创建目标Environment
  执行上下文或触碰已准备服务；该job仍不构成真机证据。
- 文档一致性检查从支持矩阵反向遍历全部28个外部格，逐一验证workflow选择能解析回相同适配器变体、
  合同修订和状态，并固定验证`vLLM/windows-x64/cuda`被拒绝。平台或加速器矩阵扩展若未同步编排将
  立即使本地与CI门禁失败。
- 有效`openvino/linux-x64/intel_gpu`实测解析到`ovms-openai-intel-gpu`，矩阵外
  `vllm/windows-x64/cuda`以退出码2拒绝；随后完整`pnpm check`以退出码0通过，覆盖111个Biome文件、
  TypeScript、前端与Agent Kernel测试、Rustfmt、全工作区Clippy、构建、全部Rust测试和doc-tests。

## 78. 真机目标清单与runner操作手册（2026-08-31）

- `engine-acceptance-coordinate.mjs`新增`buildAcceptanceTargets`，从版本化矩阵生成确定性目标清单；
  默认排除已正式外部格，`includeFormal`用于复验清单。每条记录绑定引擎、适配器变体、合同修订、
  runner平台、加速器、manifest状态、唯一Environment名称及必需/可选secret名称，不输出值。
- 新增只读`scripts/list-engine-acceptance-targets.mjs`；无参数列出25个待验收格，`--all`列出28个
  外部格，未知或重复参数退出码2。文档门禁固定25/28计数及28个Environment名称唯一性，支持矩阵
  变化但清单生成未同步时立即失败。
- 新增`docs/INFERENCE_ENGINE_ACCEPTANCE_RUNNERS.md`，固化四类runner标签、隔离账户与回环边界、
  精确Environment secrets、手动触发、无秘密预检、Rust原生证明、14天create-new产物、人工导入和
  清理顺序。该手册明确不是安装、打包、签名、公证、自动更新或正式升级流程。

## 79. v2原生宿主设备证据（2026-08-31）

- 审计发现v1运行产物虽然在请求前调用`NativeSystemProbe`，落盘后却只保留
  `platform/architecture/accelerator`与`macos/aarch64/metal`式摘要；同类主机之间无法复核原生
  设备类别，平台证据的持久化强度低于实际探针强度。
- 新增`InferenceEngineAcceptanceHostAttestation`。`nativeHostProbeV1`同时绑定原生
  `host-capabilities-v3`探针修订、精确平台/架构/加速器支持格和设备类别SHA-256。规范输入包含CPU
  品牌、设备型号、内存、物理/逻辑核心及排序去重后的加速器集合；模型存储路径和可用空间等易变或
  私密字段明确排除，产物也不保存序列号、端点、命令或凭据。
- 运行产物与正式账本升级到schema v2及独立JSON Schema。live acceptance只允许原生v2证明；支持
  格不匹配、探针修订未知、CPU/设备字段缺失、内存/核心无效、加速器缺失或重复、指纹畸形均故障
  关闭。平台证据断言现在携带同一设备类别指纹，不能再只声明坐标命中。
- v1数据与Schema原样保留为历史合同。标准v2账本中的Ollama CPU、Ollama Metal和MLX-LM Metal三条
  既有记录显式标记`legacyHostSummaryV1`，没有把当前机器信息伪造性回填到过去的运行；新追加与
  `--replace-record-id`重新资格验证一律拒绝legacy，只接受`nativeHostProbeV1`。Rust另以记录ID、
  引擎变体、精确支持格和原始验收时间建立三条固定迁移allowlist，不能通过手工构造v2账本增加或
  改写legacy支持声明。
- 正式支持状态仍为4格正式、25格待验收、3条外部记录；本轮只是加强未来真机证据的可复核性，未
  生成新的真实硬件结果，也未晋级任何引擎。
- 24项宿主/账本专项测试、28格结构验收管线、离线导入和审查投影全部通过；随后完整`pnpm check`
  以退出码0通过，覆盖文档一致性、115个Biome文件、TypeScript、桌面26项测试、Agent Kernel 34项
  测试、Rustfmt、全工作区Clippy、Kernel构建、全部Rust测试及doc-tests。

## 80. v2历史格复验可用性检查（2026-08-31）

- 在不启动、不安装、不下载也不重配置任何服务的前提下，只读检查当前本机监听端口；先前隔离验收
  使用的Ollama `11434`与MLX-LM `8080`均无监听，其他规划回环端口也没有对应服务。
- 因此本轮没有用合成服务或旧运行产物生成v2原生证明，三条既有记录继续保持
  `legacyHostSummaryV1`。未来重新准备相同固定版本、模型修订和设备格后，必须走v2 live
  acceptance与显式`--replace-record-id`，不能就地改写历史记录。
- 同步移除已失真的“无账本正式格可逐步缩减”测试模型：当前全部三个正式外部格均有审查账本记录，
  回归现在直接要求`formalCellsMissingLedger == 0`；宿主证明迁移债务只存在于记录内部，与缺记录
  例外严格分离。

## 81. 运行时设备证据显式化（2026-08-31）

- 审计发现运行方案授权仍存在一层隐式推断：当资格报告的`observed_accelerator`为空时，只要某个平台、
  架构和部署下当前恰好只有一个正式加速器，授权层就会把清单唯一性当作运行设备证明。这会让支持格
  晋级顺序改变同一份资格报告的含义，也无法向Pi和界面准确说明依据强度。
- Protocol以必填`EngineRuntimeDeviceEvidence`替换可空加速器，分为模型驻留实测、固定适配器变体合同
  和未解析三类。Ollama `/api/ps`使用模型驻留实测；MLX-LM、OVMS三变体、vLLM、SGLang、LMDeploy
  与TensorRT-LLM使用固定单设备变体合同；当时横跨Metal/Vulkan/CUDA/ROCm的MLC LLM先保持未解析，
  随后由第82节的变体拆分解除这一结构性阻塞。
- 运行方案保存、复验、激活前和切换后统一验证该证据。模型驻留必须精确命中所选格；固定变体合同
  只有在descriptor恰好声明一种加速器且该变体全部支持格均为同一设备时才成立；未解析永远失败，
  不再因当前只有一个正式格而升级。由此MLC LLM后续正式支持必须先拆分设备变体或取得可信服务/部署
  设备证据，不能仅凭宿主拥有GPU和操作者选择完成晋级。
- 现有4个正式格状态未改变：Ollama CPU/Metal继续由模型驻留观察证明，MLX-LM Metal由固定单设备
  变体合同与既有审查记录共同授权；其余25格仍等待真机验收。本轮Protocol/Infra共计新增和更新的
  设备证据回归已通过，未生成或改写任何真实验收记录。

## 82. MLC LLM单设备适配器变体（2026-08-31）

- 为消除第81节识别出的MLC设备授权阻塞，把`official-openai-server`拆为
  `official-openai-metal`、`official-openai-vulkan`、`official-openai-cuda`和
  `official-openai-rocm`。每个descriptor只声明一种加速器，资格报告因此可以诚实标记
  `adapterVariantContract`，但仍不会声称官方服务主动回报了设备。
- 四个变体对原有10格做无重叠分区：Metal仅Apple Silicon；Vulkan、CUDA、ROCm分别覆盖Windows
  x86_64与Linux aarch64/x86_64。支持格总数仍为29、正式4、待验收25；外部适配器变体数量从10增至13，
  没有新增平台承诺或支持晋级。版本化矩阵和文档门禁固定四个变体顺序及身份。
- 自动发现只在macOS提供唯一的Metal默认目标。Windows/Linux同一常用端口可能对应三种设备合同，
  因而必须由用户保存后端时明确选择，不能按端口或宿主GPU猜测。旧开发期
  `official-openai-server`绑定不自动映射到任一设备；它会保持待修复，用户重选精确变体后才能复验。
- 真机验收入口先把`HAL100_MLC_LLM_ACCELERATOR`解析为类型化设备，再构造对应变体和精确支持格；
  未知或非MLC设备在连接服务前失败关闭。新增分区回归证明4个变体、10个唯一格和非法CPU拒绝。

## 83. 跨引擎类型化故障语义（2026-08-31）

- 审计确认运行方案管理器内部已经区分数据库、托管引擎、后端、外部适配器、资格验证、支持格、
  激活与恢复错误，但Pi工具只保留5个特例，其余全部降级为`runtime_profile_operation_failed`；桌面
  又直接显示Rust错误字符串。不同引擎的不可达、响应不合格、适配器缺失、验收证据不足和运行设备
  未证明因而无法被稳定地解释或恢复。
- Protocol新增不含端点、模型ID、响应正文、命令或凭据的`RuntimeProfileFailure`：固定
  `code/stage/retryable/recoveryAction`四个字段，覆盖输入、持久化、发现、检查、资格、证据、复验、
  计划、激活、恢复和原生交互阶段。`RuntimeProfileManagerError`成为唯一映射权威；失败切换还根据
  rollback是否成功分别给出可重试或必须恢复的语义。
- Pi工具现在直接使用协议层稳定snake-case code，不再维护局部字符串表；设备证据不足精确返回
  `runtime_profile_runtime_device_unproven`，服务不可达返回
  `runtime_profile_engine_unreachable`。Desktop运行方案命令改为返回同一结构化失败合同，界面只按
  安全code显示固定中文说明，不再把底层错误或上游响应直接投影到WebView。
- 该变更不新增引擎支持格、不生成真机证据，也不改变29格总数、4格正式、25格待验收和3条外部
  账本记录；它补齐全部引擎后续正式支持所需的共同故障语义基础。

## 84. 跨引擎共存与失败恢复控制面（2026-08-31）

- 既有回归只覆盖单个外部引擎从空路由切换失败后恢复为空，无法证明引擎A已活动时切换到引擎B
  失败会恢复A，也没有断言B的资格检查不会错误调用A的适配器。
- 新增Ollama CPU与MLC LLM Metal两个不同引擎、不同适配器、不同后端和不同设备证据的内存纵向。
  测试先保存并激活MLC方案，再计划切换到Ollama，并只在Ollama动作后复验注入模型证据漂移。
- 回归证明Ollama计划与执行期间MLC检查计数不变；失败后Gateway内存路由和SQLite活动路由均精确
  恢复`MLC backend + MLC model`，激活journal清空，原MLC方案仍可实时证明为活动，失败的Ollama
  方案不会被标记为活动。返回语义固定为“激活失败、回滚成功、允许重新计划”，与第83节合同一致。
- 这条夹具只证明共同控制面事务和注册表隔离，不是MLC Metal真实服务证据，不会补齐任何支持格的
  平台、身份、稳定性或运行方案正式验收项。29/4/25与账本仍保持不变；双真实服务共存仍待代表性
  主机验收。

## 85. v3性能档案保真与缺口显式化（2026-08-31）

- 审计发现live acceptance虽然保存20次请求、每波4并发和最大延迟，离线转换为正式记录时却丢弃
  整个`stability`对象，只留下“稳定性通过”的文字断言。支持报告和推荐器因而无法区分“已测量”与
  “只有历史断言”，也无法安全使用性能事实。
- 运行产物与正式账本升级到schema v3。固定`openai-short-chat-v1`工作负载现在保存尝试数、并发度、
  p95/最大延迟、prompt/completion Token总量和总墙钟时间；Rust限制数值边界、要求p95不大于最大值，
  并在`into_formal_record`中原样保留档案。新v3正式记录缺少档案时故障关闭。
- v1/v2合同继续可读。标准v3账本中的Ollama CPU、Ollama Metal和MLX-LM Metal三条记录产生于保留
  数值档案之前，因此仍明确缺少`stability`，没有从文字断言或当前机器反推延迟。固定历史allowlist
  只兼容这三条记录；新增或复验记录不能沿用例外。
- 只读支持报告升级到schema v2：逐格投影精确账本档案，并汇总已审查档案数和正式外部格缺档案数。
  当前结果为14个适配器、29格、4格正式、25格待验收、3条外部记录、0个正式格缺账本、0条已审查
  性能档案、3个正式外部格缺性能档案，严格全矩阵完成仍为`false`。
- 当前确定性推荐不消费空档案，也不根据引擎名称、文字断言或坐标猜测吞吐。未来只有档案与相同
  工作负载及原生设备类别绑定且完成代表性复验后，才可把测量作为排序输入；本轮未改变任何支持
  状态，也未生成新的真机证据。

## 86. v4模型/设备作用域性能投影（2026-08-31）

- 审计确认v3性能档案虽绑定精确支持格和宿主证明，却没有可与保存方案比较的类型化模型身份；直接
  将单次模型测量作为引擎级推荐分数会跨模型泛化，因而不安全。
- 运行产物与标准账本升级到schema v4。新产物强制保存模型证据的种类、算法和域分离SHA-256，
  不保存原始模型ID、目录或路径；新正式记录缺少该字段会故障关闭。v1/v2/v3继续只读兼容，标准
  账本的三条历史记录通过精确allowlist保留，仍明确没有性能档案和模型证据。
- 外部引擎注册表新增精确方案性能匹配：适配器、平台/架构/加速器/部署、origin指纹、配置修订、
  引擎版本或部署指纹、类型化模型证据以及当前`NativeSystemProbe`设备类别必须全部一致。任一字段
  漂移都返回`None`，不会回退到同引擎其他模型、其他设备或其他实例，也不改变激活权限。
- Protocol只向Desktop与Pi投影固定工作负载修订、尝试/并发、p95/最大延迟、Token合计、墙钟时间、
  样本completion吞吐和审阅时间，不投影origin、模型证据指纹或宿主设备指纹。Desktop明确标注
  “受审阅实测/仅作方案间参考”，Pi指令只允许在相同`workloadRevision`的精确方案间使用；字段
  缺失视为未知，禁止跨模型、设备或工作负载推断。
- 回归证明精确匹配可获得档案，模型值、设备类别、origin、配置修订或引擎版本任一变化都会拒绝；
  v4脱敏指纹确定、类型绑定且序列化不包含原值，Agent投影同样不包含端点或模型摘要。
- 本轮没有生成真机测量、没有提升支持状态。统计保持14个适配器、29格、4格正式、25格待验收、
  3条外部账本记录、0条可投影性能档案；下一步仍是用v4入口重新验收既有三格及其余代表性矩阵。

## 87. 支持报告原子性能作用域（2026-08-31）

- 当前主机实查为Apple M1、16 GiB、macOS 26.5，但Ollama、MLX-LM及其余目标引擎均未安装或
  监听；因此没有运行真机验收，也没有生成或导入任何新证据。
- 审计发现支持报告schema v2会单独投影`stability`，没有把origin/配置、引擎身份、类型化模型
  证据和原生宿主证明作为同一对象交付。即使Rust运行方案匹配已经安全，独立报告消费者仍可能
  把数字从原作用域拆出后跨模型使用。
- 报告升级至schema v3，逐格只输出原子`reviewedPerformanceProfile`：origin指纹、配置修订、
  引擎版本或部署指纹、模型证据指纹、原生宿主证明、固定工作负载测量及审阅时间必须来自同一正式
  记录。宿主证明不是`nativeHostProbeV1`或任一字段缺失时，整个档案为未知。
- CLI摘要同步改为`performanceProfiles`和`formalExternalMissingPerformance`；结构回归覆盖完整档案
  投影及28个合成外部格的严格路径。标准三条历史记录仍得到0个完整档案和3个缺口，没有伪造数字。

## 88. 暂停点：v4真实复验准备与报告v3（2026-08-31）

- 已完成并验证：v4 live入口到离线导入会保存脱敏模型证据；支持报告schema v3原子绑定origin/
  配置、引擎、模型、原生宿主和测量；7项报告单元测试、4项29格结构测试、文档一致性检查、Biome、
  TypeScript、Agent Kernel 34项和Desktop 28项测试均通过。
- 只读CLI实测输出为：schema 3、14个适配器、29格、4格正式、25格待验收、3条账本记录、0个
  完整性能档案、3个正式外部格缺完整档案、严格晋级为`false`。
- 当前Apple M1/16 GiB/macOS 26.5主机没有安装或监听Ollama、MLX-LM及其余目标引擎，本轮没有
  运行真实推理请求、生成验收产物、导入候选账本或提升支持状态。
- 暂停时最新一次完整`pnpm check`已完成文档、Lint、类型、前端/Sidecar测试、Rust格式、Clippy和
  Sidecar构建；在最终`cargo test --workspace`重新链接Desktop目标时按用户要求终止，退出码130，
  不是测试失败。此前v4方案性能投影里程碑的完整`pnpm check`已通过，但报告v3变更后的全门禁仍需
  恢复后从头重跑。
- 恢复顺序：先运行完整`pnpm check`；再由操作员准备明确版本、模型和加速器的真实服务，优先用v4
  重新验收Ollama CPU/Metal与MLX-LM Metal三格；人工审阅后以create-new候选账本替换精确旧记录，
  不自动安装、下载、启动服务或伪造缺失测量。

## 89. 恢复门禁与跨平台验收环境预检（2026-09-01）

- 从第88节暂停点恢复后，报告v3变更的完整`pnpm check`从头运行并通过；这覆盖文档、Biome、
  TypeScript、Agent Kernel 34项、Desktop 28项、Rust格式、全target Clippy、Sidecar构建和Rust
  全工作区测试，替代了上次被主动终止的未完成结果。
- GitHub API只读审计显示远端仓库当前有0个自托管runner、0个受保护Environment，默认分支也尚未
  出现本地新增的live acceptance workflow，因此25个待验收格目前不能在远端调度。该事实不影响
  源码结构完成度，但是真机正式支持的明确外部前置条件，不能由托管runner或合成测试替代。
- 新增共享`validate-engine-acceptance-environment.mjs`，为8个引擎维护精确环境变量合同。Bash、
  PowerShell和GitHub workflow在真实请求前统一调用；只接受带显式端口/尾斜杠的
  `http://127.0.0.1/.../`，限制模型/版本文本边界，并验证Ollama、MLC LLM、OpenVINO、LMDeploy的
  设备键属于各自允许集合。
- 预检不连接服务且输出只包含变量名，远端URL或含凭据/查询/fragment的origin会在Cargo测试前
  失败关闭，错误不回显值；可选vLLM密钥在未提供时保持可选，在提供时同样限制长度并拒绝控制字符。
  文档一致性回归为全部8个引擎构造有效环境，并验证远端URL、畸形可选密钥拒绝与错误脱敏；Rust
  仍负责原生设备、适配器身份、真实响应、生命周期、稳定性和韧性验收。
- 同轮文档漂移审计发现当前状态与正式支持计划仍沿用MLC拆分前的“10个外部适配器变体”。两处已
  校正为矩阵当前事实：8个外部引擎、13个外部适配器变体、28个外部支持格；合并托管llama.cpp后
  为14个适配器、29格，其中4格正式、25格待验收。文档门禁现在直接从版本化矩阵计算并校验这组
  摘要，后续变体或支持格变化不能只改运行时而遗漏面向维护者的当前文档。
- 共享预检代码完成Biome复核后，项目级`pnpm check`再次全量通过，覆盖文档、Lint、类型、前端与
  Agent Kernel测试、Rust格式、全target Clippy、Sidecar构建、全工作区测试和doc-tests。

## 90. macOS专项回归与测试工具修复（2026-09-01）

- 本轮按要求只评估Apple M1、16 GiB、macOS 26.5，不把Windows/Linux待验收格计入结论。原生
  Apple Silicon/Metal探针、macOS开发沙箱、前端生产构建、Tauri原生编译和九阶段快速验收矩阵
  全部通过；矩阵包含100万Usage、1万模型目录、真实Pi Sidecar往返与25次启停、超大RPC帧故障
  关闭和Gateway本机延迟，观测到Gateway额外p95约247微秒。
- 真实32K连续任务首次在第1轮被隔离夹具阻断：临时数据库已索引真实模型，但夹具没有创建传给
  `NativeSystemProbe`的`models/`目录，macOS `df`因此按设计拒绝不存在的路径。夹具现在与产品组合
  根保持一致，在构造下载器和Agent服务前显式创建隔离模型目录；不会写入用户模型或配置。
- 后台监测原先对整个`summary_json`使用`LIKE '%prompt%'`，把安全的数值聚合键
  `continuationPrompts`误判为原始提示词。扫描现在通过SQLite `json_tree`检查结构化字段：只有
  prompt/answer/API key/authorization类键承载文本、对象或数组时才拒绝，同时仍拒绝畸形JSON、
  Agent会话密钥、Bearer授权和`x-api-key`文本。安全聚合夹具通过，真实`userPrompt`文本夹具仍按
  预期失败关闭。
- 修复后真实Qwen3.5/Pi 32K连续任务20/20通过：总耗时188643毫秒、单次最大16481毫秒、最多2个
  执行模型轮次、重复工具结果Token为0；停止后活动任务和子运行时均为0。随后开发版后台观察121秒
  通过，12个样本中Gateway失败0次、物理内存约43.7 MiB、文件/TCP/线程无增长、Agent子进程和会话
  目录均为0，审计误报清零。
- 本轮只修复测试夹具与监测判定，不改变引擎支持矩阵或正式证据账本。当前机器仍未运行Ollama、
  MLX-LM或MLC LLM服务，因此没有生成或导入新的外部引擎v4真机记录。

## 91. 1.0.5开发里程碑收口（2026-09-01）

- 当前迭代0—59与迭代60已完成部分统一标记为1.0.5开发版；该版本仍只表示可复现开发进度，
  不包含签名、公证、安装包、自动更新、正式升级或正式发布流程。
- 自1.0.4以来的运行方案、跨平台引擎架构、支持证据账本、Agent RPC v13、桌面信息架构和macOS
  专项回归统一进入同一里程碑；未完成真机证据的25个支持格继续保持非正式状态。
- 版本元数据、CHANGELOG、当前状态、产品与UI/UX文档及独立收口验收记录同步更新；历史检查点中
  记录的1.0.4基线保留为当时事实，不追溯改写。
