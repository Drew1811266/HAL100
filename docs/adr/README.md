# 架构决策记录

本目录记录已经确认、会显著影响后续实现的决策。决策需要变更时，新建 ADR说明替代关系，不直接抹去历史原因。

- [ADR-0001：桌面与核心技术栈](0001-desktop-stack.md)
- [ADR-0002：本地推理网关](0002-local-ai-gateway.md)
- [ADR-0003：后台生命周期与进程模型](0003-background-lifecycle.md)
- [ADR-0004：受控 HAL100 Agent](0004-controlled-agent.md)
- [ADR-0005：平台、开源与发布范围](0005-platform-and-release-scope.md)
- [ADR-0006：采用 Pi Agent Core 作为 HAL100 Agent 内核](0006-pi-agent-core.md)
- [ADR-0007：工程基线、代码布局与通信协议](0007-engineering-baseline.md)
- [ADR-0008：Agent Sidecar进程隔离策略](0008-agent-sidecar-isolation.md)
- [ADR-0009：主 WebView隐藏复用](0009-main-webview-reuse.md)
- [ADR-0010：OpenCode受管Provider与独立凭据文件](0010-opencode-managed-provider.md)
- [ADR-0011：按需硬件画像与统一模型目录](0011-on-demand-hardware-and-model-catalog.md)
- [ADR-0012：外部GGUF使用专用选择命令和确认索引](0012-confirmed-external-gguf-import.md)
- [ADR-0013：模块化单体、应用边界与 Agent 能力架构](0013-modular-monolith-and-agent-capabilities.md)
- [ADR-0014：Agent RPC v4 与受控模型下载能力](0014-agent-rpc-v4-model-download.md)
- [ADR-0015：内置 Agent Runtime 与外部 Agent 集成边界](0015-built-in-runtime-and-external-agent-integrations.md)
- [ADR-0016：Agent RPC v5 与外部 Agent 事务能力](0016-agent-rpc-v5-external-agent-transactions.md)
- [ADR-0017：受控运维观察与用户级修复边界](0017-controlled-operations-observation.md)
- [ADR-0018：部署就绪检查与有界短时观测](0018-bounded-deployment-readiness-observation.md)
- [ADR-0019：版本化外部 Agent 受管部署配方](0019-versioned-managed-agent-deployment.md)
- [ADR-0020：受管依赖闭包与可恢复卸载生命周期](0020-managed-dependency-closure-and-removal.md)
