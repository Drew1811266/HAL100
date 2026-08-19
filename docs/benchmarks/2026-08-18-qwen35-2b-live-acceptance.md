# Qwen3.5-2B 本机部署与真实 Gateway 验收

- 日期：2026-08-18
- 平台：Apple M1、16 GiB统一内存、Apple Silicon macOS
- 验收入口：`crates/hal100-infra/tests/qwen35_live_acceptance.rs`
- 结果：通过；引擎和模型保留，验收结束后推理进程已停止

## 1. 固定制品

| 项目 | 实测值 |
| --- | --- |
| 上游基础模型 | `Qwen/Qwen3.5-2B`，Apache-2.0，Transformers/Safetensors |
| GGUF发布仓库 | `unsloth/Qwen3.5-2B-GGUF`，社区量化衍生仓库 |
| GGUF文件 | `Qwen3.5-2B-Q4_K_M.gguf` |
| 远端修订 | `f6d5376be1edb4d416d56da11e5397a961aca8ae` |
| 文件大小 | `1,280,835,840`字节 |
| 文件SHA-256 | `aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223` |
| llama.cpp | 官方 Apple Silicon构建`b10218` |
| 归档SHA-256 | `f3e87f1664c09183a861f16758c55a5adc925672705cd3a47e3dc4444504c914` |
| `llama-server` SHA-256 | `ff0e2445d93e2d6305c44cce6386db1020385194261dc184deaf0f37c7148d85` |

基础模型官方仓库不是 GGUF，因此本次在用户确认实际发布者、大小和哈希后使用 Unsloth量化制品。HAL100确认页已区分目录服务、GGUF发布仓库与基础模型作者，不能把目录 API或文件哈希解释为上游作者背书。

## 2. 受控安装证据

验收测试默认`ignore`，且只有同时提供以下显式开关和真实应用数据目录才会执行副作用：

```bash
HAL100_QWEN35_DEPLOY_CONFIRMED=confirmed \
HAL100_ACCEPTANCE_DATA_DIR="$HAL100_DATA_DIR" \
cargo test -p hal100-infra --test qwen35_live_acceptance \
  confirmed_qwen35_deployment_produces_exact_gateway_usage \
  -- --ignored --nocapture
```

执行复用了生产`LlamaCppManager`、`RemoteModelCatalog`、`ModelDownloadManager`、`GatewayState`和`UsageWriter`，没有使用独立下载脚本或直接插入伪造模型记录。模型经历`Downloading → Verifying → Installing → Ready`；SQLite中的下载大小、模型大小和磁盘文件大小均为`1,280,835,840`字节。系统`shasum -a 256`独立复核模型和引擎二进制，与固定清单一致。

托管模型 ID：

```text
managed-aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223
```

## 3. 首次真实请求发现的问题

首个请求成功经过 Gateway和 llama.cpp，数据库也精确记录`33`输入、`64`输出、`97`总 Token，但 OpenAI响应的`message.content`为空。Qwen3.5在 llama.cpp自动 reasoning模式下把整个64 Token输出预算用于`reasoning_content`，触发 HAL100模型测试对空正文的故障关闭。

修复：标准托管运行参数增加`--reasoning off`。首版 OpenAI/Anthropic兼容客户端要求可见答案位于`message.content`；未来若提供思考模式，必须做成明确运行配置，并定义 reasoning Token和正文展示策略。假引擎回归测试会拒绝缺失此参数的启动命令。

## 4. 修复后真实推理与精确 Token

修复后用同一模型、同一路由重跑，得到：

```json
{
  "response": "HAL100 的 Qwen3.5 本地推理测试已成功完成。",
  "elapsedMs": 723,
  "inputTokens": 35,
  "outputTokens": 19,
  "totalTokens": 54,
  "requestId": "a6a283fa-39a3-41c4-9ae7-1c1ddb91b732",
  "clientAppId": "qwen35-live-acceptance",
  "backendId": "managed-llama-cpp",
  "usageAccuracy": "exact_backend_response",
  "status": "succeeded"
}
```

验收同时断言：

- HTTP响应 Usage满足`35 + 19 = 54`。
- SQLite同一请求 ID中的输入、输出、总 Token与后端响应逐项相等。
- 客户端归属为`qwen35-live-acceptance`，后端为`managed-llama-cpp`。
- `usage_accuracy`为`exact_backend_response`，没有字符估算或缺失补值。
- 测试结束后`LlamaCppManager::stop`返回`Stopped`，系统中不再存在`llama-server`进程。

## 5. 当前结论

Qwen3.5-2B Q4_K_M已经部署到 HAL100真实应用数据目录，并完成“远端元数据复核 → 可恢复下载 → 完整哈希 → GGUF校验 → 原子索引 → 模型启动 → Gateway中转 → SQLite精确 Token → 进程停止”的闭环。引擎和模型可由 HAL100后续直接使用，空闲时不会保留推理进程。
