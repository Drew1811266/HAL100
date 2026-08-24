# 迭代23验收：受管依赖闭包与卸载生命周期

- 日期：2026-08-21
- 平台：Apple Silicon macOS
- 版本：1.0.2开发树

## 交付范围

- Agent RPC v9与18项共享工具合同。
- Pi Coding Agent `0.84.2`完整npm lockfile v3语义闭包验证。
- npm绝对身份、最小环境、私有cwd、固定顶层归档SRI与原子runtime启用。
- HAL100私有Pi卸载预览、确认后复验、原子隔离、系统废纸篓与失败恢复。
- `managedInstallation`所有权状态，以及配置断开、私有卸载、用户卸载三种意图的明确分离。
- 用户Pi、`~/.pi`配置、凭据与会话保护。

## 真实供应链验收

使用本机`~/.local/bin/npm` 10.9.8与其相邻Node.js 22.23.1，在随机临时数据目录中执行官方Registry真实隔离安装：

| 项目 | 结果 |
| --- | --- |
| 顶层包 | `@earendil-works/pi-coding-agent@0.84.2` |
| lockfile | v3，136项非根包 |
| 规范投影大小 | 47,651字节 |
| 闭包SHA-256 | `4898c398887684b0fd367f15e75d01b98305a97db2fc805e9ebb0560d2520c37` |
| 重复解析 | 两次投影与指纹完全一致 |
| 真实安装测试 | 通过，约10.5秒 |
| 安装位置 | 随机临时HAL100私有数据目录 |
| 用户状态 | 未读取或修改真实`~/.pi`、PATH、npm prefix、配置或会话 |

真实测试依次验证包元数据、顶层归档SHA-512、完整闭包、私有入口解析和`pi --version`；只有全部通过后才启用runtime。随后卸载预览再次验证路径、版本、入口和同一闭包。

## 自动验证

| 层级 | 验证 | 结果 |
| --- | --- | --- |
| Infra配方 | 固定包、Registry、归档SRI、完整闭包、入口与版本 | 通过 |
| Infra隔离 | 清空环境、npm同目录Node最小PATH、独立cwd、owner-only暂存 | 通过 |
| 安装故障关闭 | 闭包漂移、Registry漂移、未知配方、符号链接根、用户Pi冲突 | 通过 |
| 卸载生命周期 | 单次计划、确认后lock变化、缺失runtime、废纸篓失败恢复 | 通过 |
| 共存 | 用户Pi优先，私有卸载不枚举或操作用户候选/配置/会话 | 通过 |
| Core/RPC | v9 schema、18项工具顺序/效果/前置/参数正反例 | Rust与TypeScript一致 |
| Agent | 私有卸载、配置断开和含糊用户卸载意图分别处理 | 通过 |
| Desktop | Action wire name、原生确认、安装/卸载快捷任务 | React与Rust回归通过 |

最终`pnpm check`通过：桌面20项、Sidecar 27项、Rust workspace 228项非忽略测试通过，Clippy零警告。检查流程现会在Rust真实Sidecar闭环前重建Agent Kernel，避免旧`dist`产物掩盖RPC版本漂移。`pnpm build`生产构建通过。

生产页面经真实Chromium以1440×1000打开Agent页，展开“更多计划模板”并选择“生成Pi私有卸载计划”；页面完整渲染，提示精确限定HAL100私有runtime并保留用户安装、配置和会话，控制台0错误、0警告。截图保存在开发工作区`output/playwright/iteration-23-agent-private-removal.png`。

## 仍然故障关闭的范围

- OpenCode、OpenClaw与Hermes Agent没有各自经过验收的受管安装/卸载配方。
- HAL100不替用户卸载、升级或迁移官方Pi。
- Node/npm自身的自动安装与离线依赖供应未开放。
- Sidecar没有通用Shell、包管理器、任意文件读写、Accessibility、Screen Recording、Apple Events或root Helper。

这些范围不是通过宽泛权限或通用命令临时补齐；后续每个外部Agent都必须以独立版本化配方、所有权模型和真实CLI验收进入受管生命周期。
