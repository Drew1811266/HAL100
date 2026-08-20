# 更新记录

本项目仍处于早期开发阶段。版本标签用于标记可复现的开发进度，不表示已经提供签名、公证、
安装包或正式发售支持。

## 1.0.1 — 2026-08-20

- 完成模块化 HAL100 Agent 能力架构、环境诊断与受控模型发现/下载计划。
- 建立外部 Agent 通用控制面、版本化模型契约、SQLite schema v8多资源所有权和独立凭据生命周期。
- 完成 OpenCode、Pi Coding Agent、OpenClaw 与 Hermes Agent专用接入、配置预览、原生确认、
  回滚和精确断开。
- OpenClaw通过Chat Completions、OpenAI Responses和Anthropic Messages三协议官方CLI验收。
- Hermes Agent 0.18.2通过官方CLI验收，并对低于64,000 Token上下文的模型故障关闭。
- 四个外部客户端与内置HAL100 Agent的进程、配置、凭据、会话和Usage身份保持相互独立。
- 全量静态检查、单元/集成测试、四套官方客户端隔离端到端、生产构建和后台快速稳定性验收通过。
