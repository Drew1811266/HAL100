# 迭代58检查点：LMDeploy 官方 OpenAI Server 共同适配器纵向

- 日期：2026-08-27
- 产品版本：1.0.4开发版
- 状态：软件适配纵向已完成；Linux/Windows x86_64/NVIDIA CUDA 真机与身份验收待进行
- 支持等级：`connected`（尚不允许运行方案保存或激活）

## 1. 官方合同基线

本轮按 LMDeploy 官方安装矩阵与 `api_server` 文档固定边界：

- `GET /health`：读取官方服务健康监视器返回的 `healthy` 或 `sleeping` 状态；HTTP 错误和
  `unhealthy` 均故障关闭。
- `GET /v1/models`：读取 OpenAI 模型目录，条目必须是 `object=model`，目录 ID 只作为目录
  身份证据，不冒充权重摘要。
- `POST /v1/chat/completions`：复用共享资格器验证单工具调用、流式结束和 Usage；认证只由
  Rust 目标边界注入。

官方资料：[LMDeploy 安装矩阵](https://lmdeploy.readthedocs.io/en/stable/get_started/installation.html)、
[OpenAI Compatible Server](https://github.com/InternLM/lmdeploy/blob/main/docs/en/llm/api_server.md)、
[LMDeploy API Server 源码](https://github.com/InternLM/lmdeploy/blob/main/lmdeploy/serve/openai/api_server.py)。

## 2. 适配器与证据

新增 `official-openai-server` 适配器，显式绑定 `InferenceEngineKind::LmDeploy`，只接受固定本机
`http://127.0.0.1:23333/v1/` 目标。manifest 声明 Linux/Windows、x86_64、CUDA 两个支持单元，
状态均为 `connected`；模型格式记录为 `safetensors`，证据为
`catalogIdentity/lmdeploy-model-id`，不虚构引擎版本、后端变体、权重摘要或上下文容量。

LMDeploy 官方 `api_server` 当前公开 OpenAI 模型与聊天接口、健康端点，但没有稳定的机器可读
包版本端点；TurboMind/PyTorch 变体由启动参数决定而非目录身份。因此本适配器明确返回
`qualification-required`，不会仅凭“能调用”授予 `verifiedExternal`。

## 3. 共享闭环接入

- `ExternalInferenceEngineRegistry` 注册 LMDeploy 适配器，桌面本机发现保留显式引擎/变体。
- 通用 OpenAI 后端可携带 `engine=lmDeploy` 与 `adapterVariant=official-openai-server`；在支持
  单元未达到正式级别前，运行方案管理器不会生成可激活候选。
- Pi 只读取脱敏目录与兼容性，不拥有 LMDeploy URL、模型路径、启动命令、凭据或设备选择权。

## 4. 自动化证据

- `cargo test -p hal100-infra --lib lmdeploy_external_adapter`：2/2通过。
- `lmdeploy_live_acceptance.rs` 提供 Linux/Windows x86_64/CUDA 固定服务入口，默认忽略并要求
  显式环境确认；它验证真实健康、模型目录与 OpenAI 协议资格，但不伪造缺失的版本证据。
- 项目级 `pnpm check`、workspace check、Clippy、前端测试和文档一致性检查必须保持通过。

## 5. 尚未完成的正式支持门槛

1. 在 Linux 与 Windows x86_64/NVIDIA CUDA 主机运行固定 LMDeploy 版本、固定 TurboMind 或
   PyTorch 后端和固定模型的真实推理纵向。
2. 通过受控本机启动器或运营者绑定的部署证据，记录包版本、后端变体、模型/权重修订、GPU
   能力、协议能力和并发稳定性；不能从显示名称或启动命令自由推断。
3. 完成运行方案保存、预检、确认、激活、切换后复验、漂移故障关闭与回滚；完成前保持
   `connected`。
4. ROCm、aarch64、其他加速器以及 Anthropic/Responses 兼容端点作为独立支持单元追加，不从
   CUDA/OpenAI 证据扩展。
