# ADR-0038：外部Ollama只读适配器与运行身份候选

- 状态：已接受
- 日期：2026-08-26

## 背景

现有本机后端发现只能回答默认端口是否像某类服务，外部后端目录只能回答用户保存了什么连接；
两者都不能稳定回答“这个Ollama实例当前有哪些具体模型”。如果运行方案只保存显示名称或模型
标签，同名模型被重新拉取或替换后仍可能被误认为原来验证过的组合。另一方面，外部Ollama属于
用户，HAL100不能因为能连接API就取得其安装、进程或模型生命周期权限。

Ollama官方API分别提供只读版本端点[`GET /api/version`](https://docs.ollama.com/api-reference/get-version)
和模型目录端点[`GET /api/tags`](https://docs.ollama.com/api/tags)，并提供独立的
[OpenAI兼容`/v1/`接口](https://docs.ollama.com/api/openai-compatibility)。官方文档还明确本机API
默认不要求认证；这不意味着HAL100可以扩展探测范围或调用修改接口。

## 决策

1. 新增对象安全的`ExternalInferenceEngineAdapter`，只允许返回受控引擎描述符与一次
   `ExternalEngineSnapshot`检查结果。它不继承托管`InferenceEngineAdapter`的安装、移除、启动
   和停止能力，也不接收WebView、Pi或配置文件提供的任意探测URL。
2. 首个Ollama实现只访问代码固定的IPv4回环地址：`127.0.0.1:11434/api/version`、
   `127.0.0.1:11434/api/tags`与对应OpenAI API根地址。HTTP客户端禁用系统代理，拒绝凭据、查询
   和片段，连接超时350毫秒、总超时1秒。
3. 版本响应上限为64 KiB；模型目录上限为4 MiB和4096项。每个模型必须满足`name == model`、
   固定字符串边界、无控制字符、无重复名称和64位十六进制摘要。目录任一结构不可信时整体标记
   不完整，不从部分结果生成运行方案候选；版本身份仍可如实保留。
4. 能力目录始终列出已注册的Ollama外部描述符；服务不可达时`external_runtime`为空，不把
   “平台理论支持”显示为“本机服务正在运行”。描述符依据Ollama官方
   [macOS](https://docs.ollama.com/macos)、[Windows](https://docs.ollama.com/windows)、
   [Linux](https://docs.ollama.com/linux)与[GPU支持](https://docs.ollama.com/gpu)文档表达未来
   三平台、CPU/Metal/CUDA/ROCm/Vulkan边界；宿主兼容性仍只使用HAL100实际探测到的能力。
5. 运行身份候选必须同时满足：后端由用户保存、已启用、外部所有权、引擎身份一致、API根地址
   完全一致、实时模型目录完整。候选固定包含`backend_id`、`engine_version`、`model_id`和
   `model_digest`，并可带格式、参数规模和量化信息。
6. 候选是只读DTO，不持久化，不包含API根地址、模型路径、命令、环境变量或凭据，当前不能保存
   或激活。未来外部方案必须另行实现Rust拥有的版本化验证快照、漂移复核、一次性计划、原生确认
   与失败恢复，不能把本ADR的发现结果当作执行授权。

## 影响

- 外部服务发现与托管生命周期在类型层分离，新增适配器不会自然获得系统执行权限。
- 同名Ollama模型可通过摘要区分；模型或引擎版本变化能够成为未来复验依据。
- 已保存后端是用户意图，实时快照是现实证据；只有两者相交才向界面暴露候选。
- 当前不新增SQLite迁移、Agent工具、Gateway热路径、常驻轮询或后台端口扫描。

## 非目标

本决策不安装、卸载、启动、停止或升级Ollama，不拉取、创建、复制或删除Ollama模型，不开放
外部运行方案保存/切换，也不涉及Windows/Linux桌面实现、签名、公证、安装包、自动更新或正式
升级流程。
