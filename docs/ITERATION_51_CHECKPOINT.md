# 迭代51检查点：正式引擎支持合同与证据模型

状态：共同合同已完成；51A—51D均已落地，迭代53继续复用本合同
日期：2026-08-27
版本：1.0.4开发基线
数据库：schema v13
运行方案：spec v3
Agent RPC：v13（未变）

## 1. 本阶段目标

迭代51先建立所有正式引擎共用的类型、注册、实例、证据和激活边界，不提前把预留引擎标为
可用。51A只进行无行为重构，确保现有llama.cpp托管链路、Ollama外部链路、Gateway、Desktop和
Pi协议保持不变。

## 2. 51A已完成

- 新增`EngineAdapterId`，把上游引擎、适配器变体和合同修订分离；同一引擎可以拥有多个经过
  独立验收的变体。
- 新增`reserved`、`connected`、`verifiedExternal`、`managed`四级状态，并以平台、架构、
  加速器和部署方式组成`InferenceEngineSupportUnit`，不再用一个全局布尔值夸大支持范围。
- 新增不可变`InferenceEngineManifest`和Rust静态`InferenceEngineManifestRegistry`；注册表拒绝
  重复适配器身份、描述符越界支持单元及所有权不匹配的正式支持声明。
- llama.cpp与Ollama适配器通过manifest投影原有`InferenceEngineDescriptor`，因此现有Desktop
  DTO和schema保持不变。当前llama.cpp只声明macOS/aarch64/Metal托管单元；Ollama的macOS开发
  单元为`verifiedExternal`，Windows/Linux单元仍为`reserved`。
- 运行方案管理器组合一个同时包含托管与外部适配器的manifest注册表；Ollama外部注册表也在
  启动时验证自己的manifest。
- 新增`RuntimeProfileRepository`，把方案列表、读取、插入、元数据更新、重验、激活标记和删除
  从策略管理器中抽离，暂时委托现有schema v12数据库方法，未修改持久化语义。
- Desktop组合根把同一个`RuntimeProfileManager`直接注入`AgentService`和`AgentToolExecutor`，
  删除工具执行器内部临时构造及随后setter覆盖的双状态源。

## 3. 51B已完成：目标与观察服务

- 新增非序列化`EngineInstance`、`ValidatedEngineOrigin`、`VerifiedEngineTarget`和
  `EngineTargetKey`。观察键同时绑定实例ID、适配器变体/合同、origin指纹和后端配置修订，不能
  由WebView或Pi提交URL直接构造。
- 本机origin只接受显式端口的`http://127.0.0.1`，拒绝`localhost`、远程主机、用户信息、查询、
  fragment和无界实例ID；派生端点始终保持同一origin。
- 新增共同`BoundedEngineHttpClient`：禁用系统代理和重定向，限制连接/总超时、Content-Length
  与流式实际响应大小。
- `EngineInspector`升级为目标感知合同；Ollama检查器从目标安全派生`/api/version`与`/api/tags`，
  不再把单一端口固化在适配器实例中。默认发现仍只使用固定11434回环目标。
- 外部注册表改按完整`EngineAdapterId`索引，允许同一引擎存在多个变体；spec v2按kind查找遇到
  多变体时故障关闭，不能随机选择。
- 新增`EngineObservationService`。显示读取使用2秒逐实例缓存和singleflight；保存、重验、计划、
  激活前及激活后复核全部调用实时授权观察。授权观察可以刷新显示缓存，但显示缓存不能反向用于
  授权。
- 运行方案目录现在按已验证目标而非engine kind批量观察；相同引擎的不同后端、端口、变体或
  配置修订不会复用快照。保存与重验的网络观察已移出全局方案写锁，写入前重新核对后端/方案
  修订。

## 4. 51C已完成：spec v3、schema v13与证据身份

- schema v13为后端保存引擎家族、适配器变体、部署形态和配置修订；运行方案保存精确适配器
  合同、后端配置修订、origin指纹、协议能力指纹和类型化证据。
- spec v3提供`contentDigest`、`repositoryRevision`、`deploymentFingerprint`和
  `catalogIdentity`。兼容`model_digest`列仍保存64位比较指纹，但非摘要证据不再被描述为内容
  完整性。
- v12方案在迁移中保持ID、名称、时间与原摘要；llama.cpp SHA-256和Ollama digest均迁移为
  `contentDigest`，非法或无法映射的数据故障关闭。
- 外部方案保存、重验、预检、执行前与执行后使用同一种证据比较，且绑定后端配置修订与origin，
  不能把另一个端口、实例或适配器的观察复用为授权。

## 5. 51D已完成：激活authority与崩溃恢复

- 一次性激活计划绑定方案修订、精确适配器、后端配置修订、origin指纹、证据、协议能力指纹和
  预期活动路由；方案或路由在确认前变化会在任何副作用前拒绝。
- SQLite增加单飞激活journal与CAS阶段。切换失败会补偿旧路由和旧托管状态；应用启动发现未完成
  journal时只执行恢复，不继续未完成目标，也不恢复旧确认权。
- vLLM作为第二种引擎接入固定`/version`、`/health`和`/v1/models`合同，模型以
  `catalogIdentity/vllm-model-id`表示。其支持单元保持`connected`，用于证明共同合同可复用，
  不在本迭代提前声明正式支持。

## 6. 后续已落地的共同强化

- Windows/Linux宿主探针按cfg拆分并通过目标交叉检查；Linux只有同时看到NVIDIA驱动版本和DRM
  PCI厂商`0x10de`时才加入CUDA候选，仍需引擎资格验证。
- 已保存外部后端的Bearer/API Key由BackendManager从Keychain解析成不可序列化、Debug脱敏的
  `EngineRequestAuth`，只在共同HTTP边界注入同一验证origin；WebView、Pi、数据库和日志不接触
  明文。
- 能力目录由单一可选快照升级为多实例快照，并独立绑定每个后端配置修订；默认发现目标与同origin
  已保存实例去重。
- vLLM显式资格探针以固定请求验证Chat unary、SSE、精确Usage和单命名工具调用，产生版本化能力
  指纹；目录刷新不触发推理。真实Linux/CUDA入口为默认忽略测试，必须显式提供固定模型、服务
  版本和确认环境变量。

## 7. 安全与兼容结论

- manifest只包含编译期能力声明，不包含URL、凭据、进程句柄、实时观察或激活权限。
- 当前Ollama仍只探测固定`127.0.0.1`端点；没有放宽代理、重定向、认证或外部URL策略。
- RPC v13及现有运行方案ID保持不变；schema v13执行向前迁移，不实现正式发行升级流程。
- 新增支持状态只是Rust内部正式支持合同，不把vLLM、MLX-LM、MLC LLM、OVMS、SGLang、
  LMDeploy或TensorRT-LLM提前显示为可用。
- 本阶段继续排除签名、公证、安装包、自动更新和正式升级流程。

## 8. 验证

- 当前目标回归：`hal100-infra`167项中160通过、7项显式忽略；`hal100-protocol`36项全绿；
  `hal100-platform`12项全绿。Linux x86_64与Windows x86_64的平台目标检查通过。
- 覆盖manifest越权拒绝、目标/origin隔离、HTTP大小/重定向/认证边界、多实例缓存、v12→v13迁移、
  类型化证据漂移、路由authority漂移、journal单飞/CAS/启动恢复和vLLM双请求资格合同。
- vLLM真实Linux/CUDA测试仍保持忽略，未产生真机报告前manifest不得从`connected`提升。

## 9. 下一阶段：迭代53真机资格与迭代54

迭代53还需在固定Linux/CUDA主机、固定vLLM版本和不可变模型修订上执行真实资格与Gateway纵向，
记录取消、错误映射和稳定性证据后才能提升对应支持单元。与此同时可以开始迭代54的MLX-LM
适配器合同，但不能用macOS开发机结果替代vLLM Linux/CUDA证据。

完整实施边界见[多推理引擎架构蓝图](INFERENCE_ENGINE_ARCHITECTURE_BLUEPRINT.md)和
[正式支持计划](INFERENCE_ENGINE_SUPPORT_PLAN.md)。
