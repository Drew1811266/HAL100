# 迭代55检查点：MLC LLM跨平台适配器纵向

- 检查日期：2026-08-27
- 产品基线：HAL100 1.0.4开发初期
- 状态：共同软件合同已落地；真实平台与部署指纹验收进行中
- 下一阶段：先完成Apple Metal真实验收，再扩展Windows Vulkan与Linux Vulkan/CUDA

## 1. 当前支持单元

| 引擎 | 变体 | 平台 / 架构 / 加速器 | 部署 | 状态 |
| --- | --- | --- | --- | --- |
| MLC LLM | `official-openai-server` / `engine-contract-v1` | macOS / aarch64 / Metal | 本机回环外部服务 | `connected` |
| MLC LLM | `official-openai-server` / `engine-contract-v1` | Windows / x86_64 / Vulkan、CUDA、ROCm | 本机回环外部服务 | `connected` |
| MLC LLM | `official-openai-server` / `engine-contract-v1` | Linux / aarch64、x86_64 / Vulkan、CUDA、ROCm | 本机回环外部服务 | `connected` |

这些单元只表示共同适配器可识别和验证协议，不表示对应平台已有真实运行证据，也不会生成可保存
或可激活的正式运行方案。

## 2. 已完成的软件实现

- 新增 `MlcLlmExternalEngineAdapter`，固定官方 `mlc_llm serve` 的 `127.0.0.1:8000/v1/`
  目标，只访问 `/v1/models` 与 `/v1/chat/completions`，没有安装、启停、编译、模型下载或任意
  命令权限。
- `/v1/models` 严格校验 `object=list`、唯一模型 ID、模型对象和官方 `owned_by=MLC-LLM`（缺失
  时兼容旧服务），模型证据如实记录为 `catalogIdentity`，模型格式标记为新增类型 `mlc`。
- 复用共享 OpenAI 资格探针，验证单工具调用、流式 choice/结束标记、`stream_options.include_usage`
  和 Usage 数值；MLC 官方响应允许空 `system_fingerprint`，不会将空值冒充版本。
- 本机发现候选保留显式 `engine=mlcLlm` 与 `adapterVariant=official-openai-server`，用户接受候选
  时不会退化成无身份的通用 OpenAI 后端。
- `InferenceModelFormat` 增加 `mlc`，为后续编译产物/模型库部署证据提供类型位。

## 3. 资格门槛与未完成项

MLC LLM 官方 REST 服务当前没有稳定的包版本端点，官方 Chat Completion 响应的
`system_fingerprint`也可能为空。因此 HAL100 不伪造版本号、源 HF 模型摘要或部署指纹；适配器
保持 `connected`，不允许运行方案进入保存、激活和Agent推荐链路。

要升级为 `verifiedExternal`，必须在目标平台取得：

1. 官方 MLC 编译产物和模型库的不可变部署指纹；
2. 真实模型目录、Chat、流式、Usage、工具调用与取消/错误行为证据；
3. 与宿主平台/架构/加速器匹配的真实运行记录；
4. 保存、预检、原生确认、激活、切换后复验和恢复闭环。

## 4. 测试与边界

- Rust 单元夹具覆盖有效 MLC 目录、非官方目录所有者拒绝、OpenAI 协议资格和空指纹处理。
- `cargo check --workspace --all-targets`、`cargo clippy --workspace --all-targets -- -D warnings`、
  前端类型检查与既有测试持续作为门禁。
- 本检查点不涉及签名、公证、安装包、自动更新或正式升级流程；也不把官方平台宣传矩阵当作
  HAL100 的真实支持证据。
