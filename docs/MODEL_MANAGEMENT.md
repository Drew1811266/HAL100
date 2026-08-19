# 模型管理开发说明

本文记录 Apple Silicon Alpha 已接通的模型获取与本地目录边界。模型元数据、文件写入、校验、所有权和确认均由 Rust Core负责；React只展示状态并提交窄参数命令。

## 1. 当前能力

- 模型页按需读取 Apple Silicon芯片、机器标识、统一内存、CPU核心数和模型目录可用空间；没有后台硬件轮询。
- 默认下载源支持 Hugging Face和ModelScope，首次运行保持未选择。
- 两个来源统一映射为仓库、修订、许可证、GGUF文件、大小、SHA-256和量化信息。
- 只支持公开且具有确定大小和SHA-256的 GGUF；Alpha 不保存站点访问令牌，也不下载 gated/private仓库。
- 下载前重新读取权威元数据，检查磁盘空间，并生成5分钟有效的一次性确认计划。
- 下载任务支持 Range断点续传、取消、应用重启后暂停恢复、限频进度持久化、SHA-256和GGUF头校验，以及同卷原子安装。
- Hugging Face或ModelScope托管下载完成后，模型、位置、下载状态和审计事件在一个 SQLite事务内提交。
- 本地 GGUF可经原生文件选择器、固定头和完整SHA-256检查、语义预览、一次性计划及 Rust原生确认建立`external`索引；不复制、移动或删除源文件。
- 模型库复核已索引文件的存在性、大小、修改时间和哈希记录，显示`ready`、`missing`、`changed`或`verificationFailed`；启动前重新计算完整SHA-256。
- 真实安装、下载、导入按钮只在 Tauri运行时启用；浏览器预览不会写文件。

## 2. 远程目录适配与发布者边界

`RemoteModelCatalog`使用固定 HTTPS官方端点、无代理客户端、5秒连接超时、15秒总超时、最多3次重定向和4 MiB响应上限。查询、仓库名和文件路径在请求前校验；结果最多20个仓库、每仓库最多5000个文件。

Hugging Face使用 Hub API读取仓库搜索和带 blob信息的文件元数据；ModelScope使用官方 OpenAPI读取模型搜索/详情，并使用官方文件列表接口取得修订与文件哈希。下载计划不会信任先前页面缓存，而是再次解析同一仓库和远端路径。

“官方端点”只表示 HAL100调用 Hugging Face和ModelScope的官方目录 API，不表示目录中的每个仓库都由基础模型作者发布。GGUF可能是社区发布者基于上游模型转换或量化的衍生制品。确认界面必须展示实际发布仓库、许可证、远端修订和完整SHA-256，并明确提示用户核对发布者；HAL100的哈希校验只能证明下载内容与所选远端修订一致，不能替代发布者信任判断。

输入满足`owner/repository`格式的精确仓库名时，UI直接读取该仓库而不执行宽泛搜索；普通模型名称仍走最多20项的目录搜索。两条路径最终都必须重新生成一次性下载计划并经过 Rust原生确认。

来源契约会变化，因此升级适配器时必须同时运行映射单元测试和忽略型官方服务验收：

```bash
cargo test -p hal100-infra official_catalogs_resolve_a_public_gguf_repository -- --ignored --nocapture
```

本机 Qwen3.5-2B测试制品另有固定修订、大小和哈希的实时验收：

```bash
cargo test -p hal100-infra hugging_face_resolves_pinned_qwen35_test_artifact -- --ignored --nocapture
```

## 3. 下载状态机

```text
Pending → Downloading → Verifying → Installing → Ready
              ├→ Paused
              ├→ Cancelled
              └→ Failed
```

- 计划阶段要求确定的SHA-256，并按“文件大小 + 512 MiB”保守检查可用空间。
- 临时分片与最终文件位于同一 HAL100模型目录，避免跨卷重命名失去原子性。
- 恢复请求要求正确的`Content-Range`起点、终点和总大小；服务器返回完整`200`时安全地从零重启。
- 每写入约4 MiB才更新一次 SQLite，避免网络热路径产生高频小写入。
- 完整分片先校验SHA-256，再检查GGUF头；任一失败都不会创建`ready`模型。
- 文件重命名后数据库事务失败时，下一次恢复会识别已经存在且可校验的目标文件并完成索引。
- 应用启动会把遗留的活动任务改为`paused`，不会在后台自动恢复下载。

## 4. 硬件探测生命周期

`hal100-platform::MacOsSystemProbe::hardware_profile`只从专用 Tauri命令调用，在阻塞线程中通过绝对系统程序读取固定白名单字段：

```text
/usr/sbin/sysctl -n hw.memsize hw.physicalcpu hw.logicalcpu machdep.cpu.brand_string hw.model
/bin/df -Pk <HAL100模型目录>
```

命令不经过 Shell，模型目录由应用数据目录生成。建议只是筛选起点，不是性能承诺：默认优先 GGUF `Q4_K_M`，更高质量再评估`Q5_K_M`；上下文长度和 KV Cache仍会改变实际内存占用。

## 5. 数据与所有权

- `settings.models.default_download_source`只允许`huggingFace`或`modelScope`。
- `models`记录实际来源、仓库、修订、许可证、量化、所有权和状态。
- `model_locations`记录唯一路径、大小、修改时间和SHA-256；旧记录缺少哈希时转为`verificationFailed`，必须重新下载或导入。
- `downloads`记录来源、远端版本、进度、临时/最终路径、预期哈希和错误码。
- `audit_events`只保存结构化操作摘要，不保存提示词、回答或密钥。

HAL100托管模型位于应用数据目录的`models/managed`子目录，所有权为`managed`；外部索引始终保留原路径和`external`所有权。模型移除使用5分钟有效、只保留最新且一次消费的预览计划，并始终经过Rust原生确认：托管文件在复核所有权、活动状态、路径组件、符号链接、规范路径与文件大小后移入系统废纸篓；已缺失的托管文件只清理索引；外部模型只移除索引且保留源文件。数据库删除会校验预览时的所有权与路径并写脱敏审计。HAL100内置Agent模型是首版运行依赖，不允许移除。

## 6. Tauri命令

| 类别 | 命令 | 约束 |
| --- | --- | --- |
| 只读 | `get_hardware_profile`、`get_model_library`、`get_model_downloads` | 按需调用，无常驻扫描 |
| 来源 | `set_default_download_source`、`search_remote_models`、`get_remote_model_repository` | 搜索不写模型文件 |
| 下载 | `plan_model_download`、`start_model_download`、`resume_model_download`、`cancel_model_download` | 首次写文件必须消费一次性计划并通过 Rust原生确认 |
| 导入 | `select_and_plan_gguf_import`、`apply_gguf_import` | 专用选择器；原生确认时复核文件快照和完整哈希 |
| 移除 | `plan_model_removal`、`apply_model_removal` | 所有权分流；活动模型互斥；托管文件只进废纸篓，外部文件只移除索引 |

推理引擎与模型运行生命周期见[托管 llama.cpp开发说明](LLAMA_CPP_MANAGEMENT.md)。

## 7. 当前边界

- 不支持 gated/private仓库或站点登录态。
- 不自动删除下载失败的分片，便于用户恢复；未来的清理操作必须确认。
- 不提供不可恢复删除；托管文件只进入系统废纸篓，外部源文件不由HAL100删除。
- 不监控用户在 HAL100之外对外部模型文件的持续变化，只在读取模型库和启动前复核。
