# HAL100 Agent 第三方依赖登记

本文登记迭代 7 本地 Agent纵向闭环直接使用的上游组件。它不是整个Rust、npm和系统依赖树
的完整清单。项目当前不规划可分发构建；只有未来明确扩大到分发范围后，才需要另行生成完整
依赖清单、许可证正文和Notice，不能把该工作推定为当前任务。

| 组件 | 固定版本/修订 | 许可证 | 上游与用途 |
| --- | --- | --- | --- |
| `@earendil-works/pi-agent-core` | 0.84.2 | MIT | [earendil-works/pi](https://github.com/earendil-works/pi)，作者元数据为 Mario Zechner；仅使用 Agent 状态与工具循环 |
| `@earendil-works/pi-ai` | 0.84.2 | MIT | [earendil-works/pi](https://github.com/earendil-works/pi)，作者元数据为 Mario Zechner；仅使用 OpenAI-compatible Provider 层 |
| `@earendil-works/pi-telemetry` | 0.84.2（传递依赖） | MIT | [earendil-works/pi](https://github.com/earendil-works/pi)；由上述 Pi 包引入，HAL100 不启用远程遥测出口 |
| Node.js | 24.18.0 | Node.js 多许可证集合 | 固定的 Agent Kernel JavaScript 运行时；开发期来自 Workspace 依赖 |
| llama.cpp | `b10218` | MIT | HAL100 固定哈希的 Apple Silicon 推理运行时 |
| Qwen3.5-2B | `Qwen/Qwen3.5-2B` | Apache-2.0 | Agent 基础模型；权重许可证继续由模型作者约束 |
| Qwen3.5-2B Q4_K_M GGUF | `unsloth/Qwen3.5-2B-GGUF`，修订 `f6d5376be1edb4d416d56da11e5397a961aca8ae` | 仓库声明 Apache-2.0；同时受基础模型条款约束 | 本机按需 Agent 模型，固定文件大小与 SHA-256；当前不提交到源码仓库 |
| Rust `trash` crate | 5.2.6 | MIT | [ArturKovacs/trash](https://github.com/ArturKovacs/trash)；把 HAL100 托管模型和私有外部 Agent 运行时移入 macOS 废纸篓，并为未来 Windows 回收站保留同一抽象 |

Pi 包在 `package.json` 中声明 MIT，但 0.84.2 npm tarball的 `files` 清单不包含仓库根 LICENSE。
若未来明确进入分发范围，HAL100必须从对应固定上游修订保留MIT许可证正文，不能只依赖npm
包内文件。精确版本由`sidecars/agent-kernel/package.json`与`pnpm-lock.yaml`双重锁定；任何
开发期依赖升级仍必须重新执行协议、工具、安全、真实模型、内存和退出回归。

HAL100 应用包不嵌入完整的 `@earendil-works/pi-coding-agent`。用户独立安装的 Pi Coding
Agent 拥有自己的可执行文件、`~/.pi/agent` 配置与会话；它不是 HAL100 内置 Agent
Runtime 的一部分，也不参与上述 Sidecar 固定依赖的模块解析或升级生命周期。HAL100 可在用户
原生确认后按版本化配方把官方 0.84.2 包安装到应用数据私有目录；该副本固定顶层 SRI 和完整
npm lockfile v3 语义闭包，不写全局 prefix，也不替代用户安装。HAL100 的 Pi 专用适配器仍只
管理 `models.json` 中的 `providers.hal100` 和一个独立 Gateway 凭据。

外部兼容性验收使用 `@earendil-works/pi-coding-agent@0.84.2`；真实 CLI 测试继续下载到隔离
临时 HOME，产品私有部署则只下载到 HAL100 应用数据目录。它不进入构建期 `pnpm-lock.yaml`、
应用包或 Sidecar 模块图，但运行时配方固定归档 SRI 与 136 项非根包闭包指纹。上游包在 0.84.2 已从旧的
`@mariozechner/pi-coding-agent` 命名迁移到 `@earendil-works/pi-coding-agent`；HAL100 以
官方当前包名和 CLI 行为作为验收来源，不尝试替用户安装或迁移全局包。

OpenClaw与Hermes同样只作为外部兼容性测试依赖：

| 外部客户端 | 固定验收版本 | 获取方式 | 产品关系 |
| --- | --- | --- | --- |
| OpenClaw | `openclaw@2026.7.1-2` | 隔离临时目录中的精确npm包 | 不嵌入、不分发、不参与HAL100进程生命周期 |
| Hermes Agent | `hermes-agent==0.18.2` | `uv`创建的隔离Python 3.12环境 | 不嵌入、不分发、不读取用户真实HOME |

这些忽略型端到端测试需要网络时才下载官方包。普通构建、产品运行和非忽略测试不要求安装
OpenClaw或Hermes。上游升级必须重新核对配置文档、版本输出、命令参数、协议行为、默认模型
保留、秘密处理和Usage身份；不能只因为新版本能启动就提升兼容范围。
