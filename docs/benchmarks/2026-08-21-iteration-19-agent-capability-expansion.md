# 迭代19：Agent能力扩展 I 验收

日期：2026-08-21

## 交付范围

- Agent RPC v5与14项固定工具能力。
- OpenCode、Pi Coding Agent、OpenClaw和Hermes Agent的通用检测、配置计划与断开计划。
- Rust提示目标绑定、最小状态DTO、一次性外层计划和原生确认。
- 各客户端专用适配器的快照复验、独立凭据、写后验证与失败回滚继续作为唯一执行路径。

## 自动验收结果

| 验收项 | 结果 |
| --- | --- |
| Biome与TypeScript静态检查 | 通过 |
| 桌面React测试 | 20/20通过 |
| Agent Kernel Sidecar测试 | 27/27通过 |
| Rust Clippy workspace全目标 | 通过，警告视为错误 |
| Rust workspace默认测试 | 213项通过，13项需外部环境的测试保持显式忽略 |
| 生产前端、Sidecar与Rust workspace构建 | 通过 |
| 总览、软件接入与Agent页面真实渲染 | 通过，控制台无运行错误 |
| 工作区差异检查 | 通过 |

## 安全结论

- Pi Agent Core没有获得Shell、任意文件读写、Accessibility、Screen Recording或Apple Events权限。
- Rust Tool Broker仍是唯一执行权威；Sidecar只能请求固定、强类型、可审计的能力。
- `integrationId`由Rust从当前任务绑定，未知目标、跨目标调用、缺少前置检测和额外参数均故障关闭。
- 模型上下文不包含本地路径、配置正文、告警原文、凭据或底层适配器计划ID。
- 配置与断开必须经过Rust原生确认；取消、替换或任务结束会废弃未消费计划。

本迭代扩大的是受控业务能力，不是内核的环境级权限。后续需要桌面自动化时，应另建按能力授权、按对象限定、可撤销且可审计的系统权限层，不复用本轮外部Agent配置事务绕过操作系统授权。
