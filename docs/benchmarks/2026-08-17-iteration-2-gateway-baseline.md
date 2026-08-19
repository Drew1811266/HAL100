# 迭代2 Gateway最小闭环基准

- 日期：2026-08-17
- 机器：Apple M1，arm64
- 系统：macOS 26.5（25F71）
- 桌面构建：Rust 1.97.1，Tauri 2.11.5，release + benchmark-hooks
- 协议测试：本机随机回环端口上的真实Axum模拟后端与真实HAL100 HTTP监听

## 1. 功能闭环

契约测试不调用Router内部函数，而是分别启动模拟OpenAI后端和HAL100 Gateway监听器，通过Reqwest客户端完成真实HTTP请求：

- `/v1/models`验证HAL100本地Key，并确认本地Authorization不会转发给后端；后端只收到自己的Bearer Key。
- 非流式Chat返回12个输入、2个缓存、5个输出、17个总Token；SQLite只写入一条`exact_backend_response`记录。
- SSE Chat分四块发送正文、最终Usage和`[DONE]`；HAL100按块转发并记录8个输入、4个输出、12个总Token。
- 客户端读取首块后Drop响应；模拟后端流被取消，Usage状态为`cancelled/client_cancelled`。
- 20个并发非流式请求全部成功，Usage记录为20条，无法持久化计数为0。
- 无Key访问`/v1/models`返回OpenAI形状的401错误；没有活动后端时认证请求返回503。

测试请求中的提示词和回答只存在于HTTP内存路径，没有进入Usage表或结构化日志。

## 2. 延迟样本

本地集成测试预热10轮后分别采样200次直接模拟后端与经HAL100转发的非流式请求：

| 路径 | p95 |
| --- | ---: |
| 直接模拟后端 | 248 μs |
| 经HAL100 Gateway | 495 μs |
| p95差值 | 246 μs |

该样本来自未优化的Rust测试构建，仍显著低于“本机同协议转发p95附加延迟≤5 ms”的预算。正式并发与不同后端的基准会继续保留，不以此单机样本替代迭代9压力测试。

## 3. 桌面后台实测

release桌面进程关闭并隐藏主窗口后：

- `127.0.0.1:10100`是唯一HAL100监听地址。
- `/healthz`返回协议版本1。
- 未配置开发Key时，`/v1/models`返回401。
- SQLite自动迁移到schema version 2，新增`client_apps`、`api_key_hashes`和`usage_requests`。
- 物理占用约41.1 MiB，峰值约42.0 MiB。
- CPU五次1秒采样为0.0%、0.0%、0.1%、0.3%、0.1%。
- 明确退出状态为0，10100端口随进程释放，无Gateway或Usage writer残留进程。

加入HTTP客户端、回环监听和Usage writer后，空闲内存与迭代1约41 MiB基线基本一致，CPU仍满足不高于0.3%的预算。

## 4. 日志复核

实机迁移时发现第三方`rusqlite_migration`的Info日志会附带依赖源码绝对路径。默认过滤器已改为只允许HAL100模块输出Info、第三方库从Warn开始，后续启动不会再写入这类本机源码路径。Gateway日志只记录监听地址和配置布尔状态，不记录本地Key、后端Key、Authorization、后端URL、提示词或回答。

## 5. 结论

迭代2完成条件已经满足：测试客户端可经HAL100完成非流式和SSE推理，精确Usage按客户端归属并单次写入SQLite；认证、取消、背压、并发和后台预算均有自动化或实机证据。Responses、Messages、模型别名和跨协议能力按路线图留在迭代5。
