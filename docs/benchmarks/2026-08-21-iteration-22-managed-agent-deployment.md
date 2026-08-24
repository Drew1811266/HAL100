# 迭代22验收：版本化受管部署配方

- 日期：2026-08-21
- 平台：Apple Silicon macOS
- 版本：1.0.2开发树

## 交付范围

- Agent RPC v8与17项共享工具合同。
- `plan_external_agent_installation`的同目标检测前置、一次性计划和原生确认链。
- Pi Coding Agent `0.84.2` HAL100私有部署配方。
- 固定官方包/Registry/SRI、禁用脚本、归档复核、本地归档安装、owner-only暂存、验证与原子启用。
- 用户Pi优先于HAL100私有Pi；不修改PATH、HOME、用户配置、全局prefix或系统目录。
- 安装计划快捷入口、固定审计与文档。

## 自动验证

| 层级 | 验证 | 结果 |
| --- | --- | --- |
| Infra | 精确配方计划、单次消费、pack SRI、本地安装、入口/版本验证 | 通过 |
| Infra安全 | Registry漂移、未知配方、已安装冲突、符号链接部署根 | 故障关闭 |
| 共存 | 用户候选全部先于HAL100私有候选 | 通过 |
| Core/RPC | v8 schema、17项工具顺序/效果/前置/参数正反例 | Rust与TypeScript一致 |
| Agent | 提示目标绑定、先检测后计划、结果无本地路径 | 通过 |
| Desktop | 新Action wire name、确认文案、快捷任务 | 类型检查与React回归通过 |

最终门禁：`pnpm check`通过；前端20项、Sidecar 27项、Rust workspace全部非忽略测试通过，Clippy零警告。`pnpm build`通过。生产页面经真实Chromium打开Agent页、展开并选择“生成Pi私有安装计划”，页面完整渲染且控制台0错误、0警告。

## 尚未声称完成

- OpenCode、OpenClaw、Hermes Agent的受管安装配方。
- npm传递依赖闭包的完整锁定或离线供应。
- Node/npm自身的自动安装。
- 系统级Helper、root权限或通用Shell。

这些项目不属于本轮验收范围；未验收的安装目标固定故障关闭。
