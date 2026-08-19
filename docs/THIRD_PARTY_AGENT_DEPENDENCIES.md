# HAL100 Agent 第三方依赖登记

本文登记迭代 7 本地 Agent 纵向闭环直接使用的上游组件。它不是整个 Rust、npm 和系统依赖树的最终发行版 notice；制作可分发构建前仍需生成完整依赖清单并随包保留许可证正文。

| 组件 | 固定版本/修订 | 许可证 | 上游与用途 |
| --- | --- | --- | --- |
| `@earendil-works/pi-agent-core` | 0.84.2 | MIT | [earendil-works/pi](https://github.com/earendil-works/pi)，作者元数据为 Mario Zechner；仅使用 Agent 状态与工具循环 |
| `@earendil-works/pi-ai` | 0.84.2 | MIT | [earendil-works/pi](https://github.com/earendil-works/pi)，作者元数据为 Mario Zechner；仅使用 OpenAI-compatible Provider 层 |
| `@earendil-works/pi-telemetry` | 0.84.2（传递依赖） | MIT | [earendil-works/pi](https://github.com/earendil-works/pi)；由上述 Pi 包引入，HAL100 不启用远程遥测出口 |
| Node.js | 24.18.0 | Node.js 多许可证集合 | 固定的 Agent Kernel JavaScript 运行时；开发期来自 Workspace 依赖 |
| llama.cpp | `b10218` | MIT | HAL100 固定哈希的 Apple Silicon 推理运行时 |
| Qwen3.5-2B | `Qwen/Qwen3.5-2B` | Apache-2.0 | Agent 基础模型；权重许可证继续由模型作者约束 |
| Qwen3.5-2B Q4_K_M GGUF | `unsloth/Qwen3.5-2B-GGUF`，修订 `f6d5376be1edb4d416d56da11e5397a961aca8ae` | 仓库声明 Apache-2.0；同时受基础模型条款约束 | 本机按需 Agent 模型，固定文件大小与 SHA-256；当前不提交到源码仓库 |
| Rust `trash` crate | 5.2.6 | MIT | [ArturKovacs/trash](https://github.com/ArturKovacs/trash)；把HAL100托管模型移入macOS废纸篓，并为未来Windows回收站保留同一抽象 |

Pi 包在 `package.json` 中声明 MIT，但 0.84.2 npm tarball的 `files` 清单不包含仓库根 LICENSE。HAL100 在发行 notice 中必须从对应固定上游修订保留 MIT 许可证正文，不能只依赖 npm 包内文件。精确版本由 `sidecars/agent-kernel/package.json` 与 `pnpm-lock.yaml` 双重锁定；任何升级必须重新执行协议、工具、安全、真实模型、内存和退出回归。
