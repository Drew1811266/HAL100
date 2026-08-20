# 迭代14：外部 Agent集成基础验收

- 日期：2026-08-20
- 平台：Apple Silicon，macOS 26.5
- 结论：通过

## 1. 交付边界

本迭代建立`HAL100 Agent`内置Runtime与OpenCode、Pi Coding Agent、OpenClaw、Hermes
Agent四个外部客户端的稳定身份和生命周期边界。只迁移既有OpenCode实现并增加只读生态
目录；没有写入Pi、OpenClaw或Hermes配置，没有安装、升级或卸载外部软件，没有新增
数据库迁移、Gateway热路径逻辑或后台轮询。

## 2. 共存与所有权验收

| 验收项 | 结果 |
| --- | --- |
| 内置Runtime固定为`hal100-agent-runtime`/`hal100-agent` | 通过 |
| 外部Pi固定为`pi-coding-agent`，不使用含混的`pi`身份 | 通过 |
| 四客户端Integration、Gateway客户端和凭据ID唯一 | 通过 |
| 每个外部客户端要求独立Key、托管片段和保留默认模型 | 通过 |
| OpenCode继续使用原有数据库记录、Provider片段、0600凭据和Usage身份 | 通过 |
| 官方Pi用户HOME、`~/.pi/agent`、全局命令和两个`PI_CODING_AGENT_*`目录变量不进入Sidecar | 通过 |
| Agent Kernel只依赖Pi Core/Pi AI，不嵌入完整Pi Coding Agent | 通过 |
| 固定官方`@earendil-works/pi-coding-agent@0.84.2`在一次性隔离HOME中真实执行并返回版本 | 通过 |
| 软件接入页不为三个未实现适配器提供写按钮 | 通过 |

## 3. 自动化结果

`pnpm check`完整通过：

- Biome：39个文件，无问题。
- TypeScript：桌面与Agent Kernel类型检查通过。
- Vitest：桌面16项、Agent Kernel 27项通过。
- Rust格式与Clippy：Workspace全部目标通过，警告视为错误。
- Rust：Core 17项、桌面37项非忽略、Infra 76项非忽略、Platform 8项、Protocol
  15项和共享协议Fixture 3项通过。
- Gateway真实回环契约：10项非忽略测试通过，覆盖协议、工具、流式、取消、路由、并发和
  OpenCode Usage归属。

`pnpm build`通过：Agent Kernel生产编译、React生产构建与Rust Workspace构建均成功。前端
产物为415.28 kB JavaScript（gzip 121.65 kB）和73.54 kB CSS（gzip 12.97 kB）。

## 4. 真实界面验证

Playwright在浏览器预览模式验证`/integrations`：

- 1440×1000宽屏：边界说明双列，三个后续适配器横向排列。
- 900×1000窄屏：边界说明与适配器自动切换为单列。
- 页面包含HAL100 Agent（内置）、OpenCode、Pi Coding Agent、OpenClaw和Hermes Agent的
  正确身份与状态。
- 控制台0个Error、0个Warning。

## 5. 后台稳定性

最终121秒快速观察通过，12个样本：

- CPU平均0.0167%，最大0.2%。
- 物理内存41.9→41.8 MiB，增长-0.1 MiB，最大41.9 MiB。
- 文件句柄增长0，TCP增长0，线程增长-1。
- Agent子进程和会话目录上限均为0。
- Gateway健康失败0；Usage与审计记录在观察期间无意外新增。
- 不安全审计行0，疑似含密钥日志文件0。

第一次同等窗口因线程首末样本16→19触发既有`thread_growth_above_2`门槛，但12个样本在
15—19间多次回落，其他资源无增长；追加10秒独立采样仍在16—18间往返。未放宽门槛或
修改脚本，第二次完整窗口在线程增长-1时通过，确认它是macOS线程池端点采样波动而非
单调泄漏。

## 6. 后续顺序

下一纵向迭代实现Pi Coding Agent专用适配器：只读检测、`models.json`托管Provider、独立
凭据、语义预览、原生确认、指纹复验、回滚和固定官方CLI验收。OpenClaw与Hermes保持
注册但不可写，待Pi适配器验证共享控制面后依次实现。
