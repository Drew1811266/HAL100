# 迭代5协议核心审查记录

- 日期：2026-08-18
- 范围：OpenAI Responses、Anthropic Messages、Usage来源语义和中文接入状态
- 结论：协议子阶段通过；迭代5整体仍在进行中

## 已完成

- `/v1/responses`复用现有有界Gateway数据面，覆盖非流式、SSE、工具结构透明转发、取消和精确Usage。
- `/v1/messages`支持Bearer和Anthropic SDK常用的`x-api-key`本地认证；冲突凭据故障关闭。
- Anthropic后端认证使用独立`x-api-key`，本地客户端Key不会转发；只允许转发`anthropic-version`和`anthropic-beta`协议标头。
- Anthropic非流式与`message_start/message_delta`累计Usage标准化输入、缓存命中、输出和总Token。
- OpenAI Responses从顶层Usage或`response.completed`事件读取输入、缓存、输出和总Token。
- SQLite schema v6允许`exact_backend_event`，迁移测试证明v5既有Usage记录保持不变。
- Anthropic本地错误使用标准`type/error/request_id`结构，同时保留`x-hal100-request-id`。
- 软件接入页已把Responses与Messages显示为可用协议，不再展示占位状态。

## 性能边界

本阶段没有增加后台线程、轮询、健康检查或定时任务。三类推理入口共享现有64请求并发上限、16流式槽位、2 MiB请求、16 MiB非流式响应和64 MiB流式响应边界。SSE普通文本事件在发现`usage`字段前不进行JSON反序列化。

## 自动化证据

- Rust协议类型单元测试覆盖Responses缓存Usage与Anthropic缓存分类。
- Gateway单元测试覆盖两类SSE Usage解析、Anthropic错误外壳和冲突凭据拒绝。
- 真实回环HTTP测试覆盖Chat、Responses、Messages、工具调用、SSE、取消、20并发和客户端归属。
- schema v5→v6迁移测试覆盖既有Usage保留和事件精确值写入。
- React测试覆盖软件接入页的协议启用状态。

## 剩余项

- 模型别名、多后端路由、持久化后端配置、Keychain凭据、请求排空和原子切换已进入后续路由核心记录。
- 强制切换确认。
- 后端错误映射、有限重试与熔断。
- 外部Ollama、vLLM、llama.cpp Server发现与连接。
