# 迭代54检查点：MLX-LM Apple Silicon正式外部支持

- 检查日期：2026-08-27
- 产品基线：HAL100 1.0.4开发初期
- 状态：已完成本迭代软件垂直与Apple Silicon真实验收
- 下一阶段：迭代55，MLC LLM跨平台支持

## 1. 支持单元

| 引擎 | 变体 | 平台 / 架构 / 加速器 | 部署 | 状态 |
| --- | --- | --- | --- | --- |
| MLX-LM | `official-http-server` / `engine-contract-v1` | macOS / aarch64 / Metal | 本机回环外部服务 | `verifiedExternal` |

该状态只适用于本支持单元，不扩展到 Intel Mac、Windows、Linux、远程 MLX-LM 或未验收模型。

## 2. 软件实现

- 新增 `MlxLmExternalEngineAdapter`，只接受固定验证目标并访问官方 `mlx_lm.server` 的
  `/health`、`/v1/models` 与 `/v1/chat/completions`；没有安装、启停、拉取、删除或任意命令权限。
- `/v1/models` 的发现快照明确标记 `engineVersionExact=false`，因为官方目录只提供模型 ID 和
  创建时间，不提供包版本；不会把目录身份冒充版本或内容 digest。
- 共享 `OpenAiQualificationOptions` / `qualify_openai_agent_protocol` 统一验证 unary 工具调用、
  流式 choice/结束标记、`stream_options.include_usage`、usage 数值和跨事件
  `system_fingerprint` 一致性。
- MLX-LM 资格阶段要求官方指纹形如版本前缀，并从中提取精确包版本；版本不完整、工具调用失败、
  stream/usage 不完整或指纹冲突均故障关闭。
- 通用 OpenAI 后端可显式保存 `engine` 与 `adapterVariant`；旧 `BackendKind` 只保留网关协议和
  兼容迁移默认值，不再成为新引擎身份的唯一来源。
- 运行方案保存、预检、原生确认后的激活、动作后复验、活动方案验证均使用同一精确适配器、实例、
  origin、配置修订、证据和能力指纹；MLX-LM 的有源版本验证在每次授权关键路径执行。

## 3. 真实验收证据

验收入口：
`crates/hal100-infra/tests/mlx_lm_live_acceptance.rs`（默认忽略，需显式设置环境变量，不会自动
下载或启动服务）。

真实条件：

- macOS Apple M1，aarch64，Metal，16 GiB；服务只绑定 `127.0.0.1:18080`。
- `mlx-lm==0.31.3`，`mlx==0.32.2`。
- `mlx-community/Qwen3-0.6B-4bit`，通过 `/health`、模型目录、单次工具调用、流式输出、Usage、
  指纹版本提取与协议能力检查。
- 随后在同一真实服务上完成后端显式绑定、运行方案保存、激活、切换后复验和活动方案验证。
- `mlx-community/Qwen2.5-0.5B-Instruct-4bit` 作为负例因未产生所需工具调用而被拒绝，证明资格
  门槛是模型级而非“端口可达”级。

实测结果：MLX-LM真实验收 1/1 通过；完整 `RuntimeProfileManager` 保存/激活/复验闭环通过。

## 4. 未完成与边界

- vLLM 的软件资格垂直已完成，但 Linux/CUDA 真机证据仍未取得，因此继续为 `connected`。
- MLX-LM 的大模型容量、并发/长上下文、崩溃恢复和更多模型模板仍需后续证据；不降低当前
  `verifiedExternal` 支持单元的模型级资格门槛。
- 本检查点不涉及签名、公证、安装包、自动更新或正式升级流程。
