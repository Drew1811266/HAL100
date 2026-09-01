# 迭代56检查点：OpenVINO Model Server（OVMS）共同适配器纵向

- 日期：2026-08-27
- 产品版本：1.0.4开发版
- 状态：软件适配纵向已完成；Windows/Linux x86_64 Intel真机与设备插件证据进行中
- 支持等级：`connected`（尚不允许运行方案保存或激活）

## 1. 官方合同基线

本轮按 OpenVINO Model Server 官方文档固定服务边界：

- KServe服务器元数据：`GET /v2`，要求`name`为`OpenVINO Model Server`并读取有界`version`。
- KServe健康：`GET /v2/health/live`与`GET /v2/health/ready`，只把HTTP成功状态作为健康证据。
- OpenAI模型目录：`GET /v1/models`，要求`object=list`、每个条目为`object=model`；官方`owned_by`
  为`OVMS`时必须匹配，缺省字段不被当作内容摘要。
- OpenAI GenAI：固定`POST /v1/chat/completions`，复用共享资格器验证一次工具调用、流式结束和
  Usage。所有请求都经过Rust验证的回环目标、Keychain认证边界、无代理/无重定向和响应大小限制。

官方资料：[KServe REST API](https://docs.openvino.ai/2024/openvino-workflow/model-server/ovms_docs_rest_api_kfs.html)、
[OVMS LLM QuickStart](https://docs.openvino.ai/2026/model-server/ovms_docs_llm_quickstart.html)、
[OpenAI chat/completions](https://docs.openvino.ai/2024/openvino-workflow/model-server/ovms_docs_rest_api_chat.html)。

## 2. 适配器与证据

新增`ovms-openai-server`适配器，显式绑定`InferenceEngineKind::OpenVino`，只接受固定本机
`http://127.0.0.1:8000/v1/`目标。manifest声明：

| 平台 | 架构 | 加速器 | 部署 | 状态 |
| --- | --- | --- | --- | --- |
| Windows | x86_64 | CPU | Local | connected |
| Windows | x86_64 | OpenVINO | Local | connected |
| Linux | x86_64 | CPU | Local | connected |
| Linux | x86_64 | OpenVINO | Local | connected |

模型记录使用`openVino`格式与`catalogIdentity/openvino-model-id`证据，派生摘要仅用于稳定的
目录条目身份，不能冒充OpenVINO IR权重摘要。服务器版本来自`/v2`元数据并标记为精确；真实设备、
模型修订和运行方案证据仍需单独取得。

## 3. 共享闭环接入

- `ExternalInferenceEngineRegistry`注册OVMS适配器，桌面能力目录和本机发现保留显式引擎/变体。
- 通用OpenAI后端可保存`engine=openVino`与`adapterVariant=ovms-openai-server`，但由于支持单元
  仍是`connected`，运行方案管理器不会生成可激活候选。
- Pi只能看到脱敏的引擎、平台兼容性、目录身份和准备度；不能提交OVMS URL、命令、模型路径或凭据。

## 4. 自动化证据

- `cargo test -p hal100-infra --lib openvino_external_adapter`：2/2通过。
- `cargo check --workspace --all-targets`：通过。
- `pnpm docs:check`、Biome、TypeScript、桌面/Agent Kernel测试、Clippy和workspace全量测试：通过。
- 伪服务覆盖：正确元数据/健康/目录/协议、错误OVMS名称、错误模型所有者；共享资格器继续覆盖
  超大响应、重定向、超时、Usage缺失、工具调用不符合和流式指纹一致性。

## 5. 尚未完成的正式支持门槛

1. 在Windows与Linux x86_64取得固定OVMS版本的Intel CPU真实服务与真实模型推理记录。
2. 分别取得Intel GPU/NPU插件的原生能力证据、目标设备、OVMS版本、模型修订与稳定性记录。
3. 为每种设备形态绑定部署指纹或可复验的模型仓库修订，不能只依赖模型目录ID。
4. 完成运行方案保存、预检、确认、激活、切换后复验、漂移故障关闭与回滚纵向；完成前保持
   `connected`，不在UI或Pi回答中描述为“正式支持”。
