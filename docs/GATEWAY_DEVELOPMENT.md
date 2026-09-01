# HAL100 Gateway开发说明

- 当前易变实现摘要：[当前开发状态](CURRENT_STATE.md)
- 状态：迭代5协议、路由、故障策略、固定回环按需发现和强制切换闭环均已实现
- 默认地址：`127.0.0.1:10100`
- 当前协议：OpenAI Models、Chat Completions、Responses与Anthropic Messages

## 1. 当前入口

| 方法 | 路径 | 认证 | 状态 |
| --- | --- | --- | --- |
| GET | `/healthz` | 无 | 已实现 |
| GET | `/v1/models` | HAL100本地Bearer Key | 已实现，透明转发 |
| POST | `/v1/chat/completions` | HAL100本地Bearer Key | 已实现，非流式与SSE |
| POST | `/v1/responses` | HAL100本地Bearer Key | 已实现，非流式与SSE |
| POST | `/v1/messages` | HAL100本地Bearer或`x-api-key` | 已实现，非流式与SSE |

Gateway只绑定IPv4回环地址，不接受配置为 `0.0.0.0`。健康检查不需要凭据；所有模型请求必须认证。Bearer与`x-api-key`同时存在时必须相同，否则故障关闭。客户端Key只在SQLite保存SHA-256摘要、显示前缀和客户端归属，明文Key、Authorization、提示词和回答均不进入数据库或日志。

桌面“软件接入”页可为OpenAI兼容或Anthropic Messages客户端签发独立Key。Base URL固定为`http://127.0.0.1:10100/v1`或不带`/v1`的Anthropic入口，模型别名使用`hal100-active`。明文Key只显示一次；客户端列表只能查看显示名、前缀和创建时间。撤销操作经过Rust原生确认并立即从Gateway运行时认证注册表移除摘要，无需重启Gateway。

## 2. 开发版配置

桌面“推理后端”页已经可以登记外部OpenAI/Anthropic兼容服务、Ollama、vLLM和llama.cpp Server，配置活动后端与模型别名。后端API Key只写入macOS Keychain；SQLite只保存服务地址、类型、认证方式和Keychain引用。以下环境变量仍保留给隔离测试和内部开发：

```bash
export HAL100_DEV_CLIENT_KEY='至少24字节的高熵开发Key'
export HAL100_DEV_BACKEND_URL='http://127.0.0.1:8000/v1'
export HAL100_DEV_BACKEND_API_KEY='可选的后端Key'
export HAL100_DEV_GATEWAY_PORT='10100' # 可选，始终绑定127.0.0.1
pnpm tauri dev
```

这些变量只用于当前内部开发版。后端Key不会传给AI客户端或Agent Sidecar；正式产品配置改用系统凭据库。日志只记录后端和客户端凭据“是否已配置”，不记录值、URL或请求正文。

`HAL100_DEV_GATEWAY_PORT`改变端口时，同一个经过校验的回环地址会同时进入桌面设置、通用客户端目录和新生成的OpenCode Provider计划，避免开发配置仍指向10100。正式默认地址不变。

调用示例：

```bash
curl http://127.0.0.1:10100/v1/models \
  -H "Authorization: Bearer $HAL100_DEV_CLIENT_KEY"

curl http://127.0.0.1:10100/v1/chat/completions \
  -H "Authorization: Bearer $HAL100_DEV_CLIENT_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"model":"your-model","messages":[{"role":"user","content":"你好"}],"stream":true,"stream_options":{"include_usage":true}}'
```

## 3. 数据面约束

- 请求正文上限2 MiB。
- 非流式后端响应上限16 MiB。
- 同时进入Gateway的请求上限64。
- SSE不缓存完整回答，只有下游继续读取时才轮询上游流。
- 客户端断开会Drop上游HTTP响应并记录`cancelled`。
- 后端非流式JSON存在Usage时记录`exact_backend_response`，SSE最终事件存在Usage时记录`exact_backend_event`；不存在时标记`unavailable`，当前阶段不伪造估算值。
- 每个Chat请求只产生一条Usage记录，经容量1024的专用单写入线程写入SQLite。
- Usage队列满、写入器失效或SQLite写入失败都会增加无法持久化计数，并按60秒窗口聚合错误日志。
- HAL100生成的请求ID通过`x-hal100-request-id`返回，可与本地Usage记录关联。
- Anthropic响应还会提供标准`request-id`；HAL100产生的Anthropic错误使用`type/error/request_id`外壳。
- 同协议请求保持工具结构原样。Anthropic只向上游转发`anthropic-version`和`anthropic-beta`，不会把本地客户端Key泄露给后端。
- SSE Usage解析会先检查事件是否包含Usage字段，普通文本增量不进行JSON反序列化；该能力没有增加轮询、定时器或后台任务。
- `hal100-active`读取原子的`ActiveGatewayRoute`：托管/普通路由可保持模型名，外部运行方案则把它解析为方案绑定的真实模型。没有显式别名的其他模型名仍走活动后端并保持原模型ID；命中显式别名时，Gateway选择指定后端并只重写请求中的`model`字段。
- 路由读取使用短时共享读锁，活动请求数使用原子计数；没有新增后台线程、轮询或定时任务。
- 安全切换先把旧后端标记为排空状态，新请求立即获得`route_draining`错误；已有推理和Models请求归零后原子替换。30秒内无法排空则撤销排空状态并保留旧路由。
- 强制切换为每个后端旋转请求代次取消令牌，只取消旧代请求，再原子替换活动后端。非流式请求会返回`503 forced_route_switch`；已经交付响应头的SSE会以流错误结束。两者都只写一条`failed/forced_route_switch` Usage，且不会把用户强制操作计入后端熔断失败。
- 外部后端强制激活、托管llama.cpp强制模型切换和强制停止只由独立Tauri命令暴露，并在Rust层调用原生确认；前端不能传入“已确认”布尔值。普通切换与停止仍默认排空。
- 非敏感后端、别名、完整活动`backend + resolved model`路由与spec v3个人运行方案由当前SQLite schema v15恢复。活动路由与旧兼容后端字段在同一事务写入；方案切换另由持久化journal记录非授权恢复状态。托管会话临时接管前保存完整路由，并在停止、强制停止或崩溃后恢复。Keychain凭据缺失、无效或不可读时，该后端、外部引擎复验和引用它的运行态别名均故障关闭。
- 每个后端有独立的无定时器熔断状态。连接、响应流和5xx连续失败3次后暂停新请求15秒；冷却是否到期只在下次请求或状态读取时判断。
- `/v1/models`与用户主动执行的连接诊断属于幂等GET，遇到传输错误或502/503/504时最多重试1次、间隔25毫秒。三类推理POST绝不自动重试，避免重复生成、重复工具调用或重复计费。
- “发现本机服务”使用固定已知回环入口；能力目录还会逐实例检查用户已保存且Rust验证过的本机后端，并从Keychain注入同origin认证。不会枚举进程、扫描端口范围或访问局域网。目录检查有独立响应上限且不会进入Gateway请求热路径。

## 4. Gateway数据面不负责

- 跨协议转换和Token估算。日/小时Usage聚合由SQLite统一范围查询负责，不进入Gateway热路径。
- 局域网广播发现、任意端口扫描和更完整的后端能力矩阵。
- 四类外部Agent配置由各专用适配器负责，不进入Gateway数据面。
- 云端Agent Provider授权由Agent服务负责；Gateway只代理已明确选择的运行路由。

OpenCode、Pi Coding Agent、OpenClaw、Hermes Agent、通用本地客户端与托管llama.cpp的现有纵向能力由各专题文档和路线图验收记录覆盖。
