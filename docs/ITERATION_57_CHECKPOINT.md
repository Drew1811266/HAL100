# 迭代57检查点：SGLang 官方 OpenAI Server 共同适配器纵向

- 日期：2026-08-27
- 产品版本：1.0.4开发版
- 状态：软件适配纵向已完成；Linux x86_64/NVIDIA CUDA 真机验收进行中
- 支持等级：`connected`（尚不允许运行方案保存或激活）

## 1. 官方合同基线

本轮按 SGLang 官方 Quickstart 与服务源码固定边界：

- `GET /server_info`：读取由官方服务发布的`version`，绑定精确引擎版本；不从启动命令或进程名推断。
- `GET /health`：读取服务就绪状态；真实生成能力由后续资格探针证明。
- `GET /v1/models`：读取 OpenAI 模型目录，条目必须是`object=model`，目录ID只作为目录身份证据。
- `POST /v1/chat/completions`：复用共享资格器验证单工具调用、流式结束和 Usage；认证只由Rust
  目标边界注入。

官方资料：[SGLang Quickstart](https://github.com/sgl-project/sglang/blob/main/docs/docs/get-started/quickstart.mdx)、
[SGLang HTTP Server](https://github.com/sgl-project/sglang/blob/main/python/sglang/srt/entrypoints/http_server.py)、
[SGLang Server Arguments](https://github.com/sgl-project/sglang/blob/main/docs/advanced_features/server_arguments.md)。

## 2. 适配器与证据

新增`official-openai-server`适配器，显式绑定`InferenceEngineKind::Sglang`，只接受固定本机
`http://127.0.0.1:30000/v1/`目标。manifest声明Linux/x86_64/CUDA一个支持单元，状态为
`connected`；模型格式记录为`safetensors`，证据为`catalogIdentity/sglang-model-id`，不虚构
权重摘要或上下文容量。

## 3. 共享闭环接入

- `ExternalInferenceEngineRegistry`注册SGLang适配器，桌面本机发现保留显式引擎/变体。
- 通用OpenAI后端可保存`engine=sglang`与`adapterVariant=official-openai-server`，但支持单元
  未达到正式级别时，运行方案管理器不会生成可激活候选。
- Pi只读取脱敏目录与兼容性，不拥有SGLang URL、模型路径、启动命令、凭据或设备选择权。

## 4. 自动化证据

- `cargo test -p hal100-infra --lib sglang_external_adapter`：2/2通过。
- `sglang_live_acceptance.rs`提供Linux x86_64/CUDA固定服务入口，默认忽略并要求显式环境确认。
- 项目级`pnpm check`、workspace check、Clippy、前端测试和文档一致性检查通过。

## 5. 尚未完成的正式支持门槛

1. 在Linux x86_64/NVIDIA CUDA主机运行固定SGLang版本与固定模型的真实推理纵向。
2. 记录模型/权重修订、GPU能力、服务版本、协议能力和并发稳定性证据。
3. 完成运行方案保存、预检、确认、激活、切换后复验、漂移故障关闭与回滚；完成前保持
   `connected`。
4. AMD、Intel XPU、CPU及高级缓存/多模态/结构化输出作为独立支持单元追加，不从CUDA证据扩展。
