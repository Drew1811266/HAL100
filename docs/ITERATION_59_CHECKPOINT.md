# 迭代59检查点：TensorRT-LLM 官方 `trtllm-serve` 共同适配器纵向

- 日期：2026-08-27
- 产品版本：1.0.4开发版
- 状态：软件适配纵向已完成；Linux x86_64/aarch64/NVIDIA CUDA 真机与身份验收待进行
- 支持等级：`connected`（尚不允许运行方案保存或激活）

## 1. 官方合同基线

本轮按 NVIDIA 官方 `trtllm-serve` 文档与服务源码固定边界：

- `GET /version` 返回 TensorRT-LLM 包版本；版本字符串必须非空并完整记录。
- `GET /health` 是只读就绪检查；官方健康成功响应可以为空，适配器只把受控 HTTP 成功作为
  健康证据，不向下推断 GPU 或模型内容。
- `GET /v1/models` 返回 OpenAI 模型目录；模型 ID 只作为目录身份证据，不冒充权重摘要。
- `POST /v1/chat/completions` 复用共享资格器验证单工具调用、流式结束和 Usage；认证只由 Rust
  目标边界注入。

官方资料：[trtllm-serve 文档](https://nvidia.github.io/TensorRT-LLM/commands/trtllm-serve/trtllm-serve.html)、
[官方 OpenAI Server 源码](https://raw.githubusercontent.com/NVIDIA/TensorRT-LLM/main/tensorrt_llm/serve/openai_server.py)、
[TensorRT-LLM 支持矩阵](https://nvidia.github.io/TensorRT-LLM/reference/support-matrix.html)。

## 2. 适配器与证据

新增 `trtllm-serve-openai-server` 适配器，显式绑定 `InferenceEngineKind::TensorRtLlm`，只接受
固定本机 `http://127.0.0.1:8000/v1/` 目标。manifest 声明 Linux x86_64/aarch64、CUDA 两个支持
单元，状态均为 `connected`；模型格式首批记录为 `safetensors`，证据为
`catalogIdentity/tensorrt-llm-model-id`，不虚构权重摘要、GPU型号、backend、TP/PP/EP并行配置或
上下文容量。

官方 `trtllm-serve` 同时可以从 Hugging Face checkpoint 或预构建 TensorRT engine 目录启动。
两种形态的构建参数、TensorRT/CUDA兼容性和权重修订不同，不能由 `/v1/models` 的同一个模型 ID
互相推断；后续必须新增独立部署指纹证据或变体。

## 3. 共享闭环接入

- `ExternalInferenceEngineRegistry` 注册 TensorRT-LLM 适配器，桌面本机发现保留显式引擎/变体。
- 通用 OpenAI 后端可携带 `engine=tensorRtLlm` 与
  `adapterVariant=trtllm-serve-openai-server`；支持单元未达到正式级别前，运行方案管理器不会
  生成可激活候选。
- Pi 只读取 Rust 生成的脱敏目录与兼容性，不拥有 TensorRT-LLM URL、模型路径、启动命令、
  并行参数、凭据或 GPU 选择权。

## 4. 自动化证据

- `cargo test -p hal100-infra --lib tensorrt_llm_external_adapter`：覆盖版本、健康、目录和共享
  OpenAI 资格夹具。
- `tensorrt_llm_live_acceptance.rs` 提供 Linux x86_64/aarch64/CUDA 固定服务入口，默认忽略并
  要求显式环境确认；它验证真实版本、健康、模型目录与 OpenAI 协议资格，但不直接晋级支持单元。
- 项目级 `pnpm check`、workspace check、Clippy、前端测试和文档一致性检查必须保持通过。

## 5. 尚未完成的正式支持门槛

1. 在 Linux x86_64 与 aarch64/NVIDIA CUDA 主机运行固定 TensorRT-LLM 版本、固定 backend、固定
   模型形态和固定并行配置的真实推理纵向。
2. 通过受控本机启动器或运营者绑定的部署证据，记录包版本、CUDA/TensorRT兼容性、GPU能力、
   HF checkpoint 或 TensorRT engine 修订、backend、TP/PP/EP 参数、协议能力和并发稳定性。
3. 完成运行方案保存、预检、确认、激活、切换后复验、漂移故障关闭与回滚；完成前保持
   `connected`。
4. 多模态、Responses、Embeddings、VisualGen 和多节点/多GPU能力作为独立协议或部署支持单元，
   不从基础 Chat/OpenAI 资格自动扩展。
