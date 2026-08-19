# 迭代3 OpenCode核心链路基准

- 日期：2026-08-18
- 机器：Apple M1，arm64
- 系统：macOS 26.5（25F71）
- 桌面构建：Rust 1.97.1，Tauri 2.11.5，release + benchmark-hooks
- OpenCode状态：存在全局JSON配置；官方CLI 1.18.11通过`pnpm dlx`隔离运行

## 1. 配置安全回归

随机临时目录测试覆盖JSON/JSONC解析、注释和未知字段保留、默认模型保留、同名未受管Provider拒绝、项目配置只读诊断、一次性确认计划、预览后文件变化拒绝、原始字节备份、`0600`独立Key、原子替换、写后验证、强制失败回滚、SQLite所有权事务和Gateway凭据热更新。

开发机真实`~/.config/opencode/opencode.json`在测试后仍只有原有顶层字段，不包含`provider`；HAL100真实应用数据目录中不存在`opencode-gateway.key`。没有使用真实配置写入验证代替临时目录测试。

## 2. OpenCode形态请求

真实回环HTTP测试使用OpenCode常见Chat Completions工具调用结构，经HAL100透明转发到模拟后端。响应中的`tool_calls`完整返回；21个输入、9个输出、30个总Token以`exact_backend_response`写入，并归属`client_app_id=opencode`。已有SSE、客户端取消和20并发回归继续通过。

随后使用官方`opencode-ai@1.18.11`精确版本进行真实CLI验收。CLI运行在随机临时HOME、XDG目录和项目目录，成功读取HAL100生成的Provider与`{file:...}`Key，选择`hal100/hal100-active`，通过默认Gateway完成SSE调用；真实OpenCode请求包含内置工具定义，响应内容正确，精确Usage归属为`opencode`。CLI和Gateway退出后无进程或端口残留。

## 3. Gateway延迟

预热10轮、分别采样200次：

| 路径 | p95 |
| --- | ---: |
| 直接模拟后端 | 244 μs |
| 经HAL100 Gateway | 487 μs |
| p95差值 | 242 μs |

共享可热更新CredentialRegistry没有造成可见延迟回退，仍远低于5 ms预算。

## 4. 后台空闲

release桌面关闭并隐藏主窗口后：

- `footprint`物理占用41 MiB，峰值42 MiB。
- `ps`五次1秒样本为0.4%、0.0%、0.0%、0.0%、0.1%；均值0.1%。
- Gateway健康检查正常，SQLite自动迁移到schema version 3。
- 明确退出状态为0，`127.0.0.1:10100`释放，无残留桌面进程。
- OpenCode模块没有后台线程、轮询器、文件监听或常驻CLI进程。

物理占用和迭代2约41 MiB基线一致，仍满足隐藏窗口空闲CPU不高于0.3%、物理内存不高于80 MiB的预算。

## 5. 结论

迭代3完成条件已满足。真实OpenCode CLI能够使用HAL100管理的配置和独立Key经Gateway完成流式调用；工具定义、Usage归属、配置保护、后台预算和资源回收均有自动化或实机证据。CLI仅用于忽略型外部验收，不作为HAL100运行依赖或分发组件。
