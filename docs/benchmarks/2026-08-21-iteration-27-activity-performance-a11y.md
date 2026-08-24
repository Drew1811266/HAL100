# 迭代27：活动、性能与无障碍收口

- 日期：2026-08-21
- 版本：1.0.2 开发线
- 结论：通过

## 活动页收口

- Usage 与 Audit 共用 `ActivityPageShell`、页签和标题布局，但继续使用独立 query key、DTO、保留策略与数据库所有权。
- 用量有数据时默认只显示总 Token、请求数、最近客户端和一张趋势图；输入/缓存/输出构成、计量说明与请求明细在展开后才挂载。
- 用量无数据时只显示一个空状态，不再同时渲染空指标、空图表与空表格。
- 操作记录筛选默认收起；列表只显示时间、可读动作、对象和成功/失败。event type、target ID 与白名单字段进入详情抽屉。
- “不轮询”“精确 usage”“脱敏白名单”等解释不再常驻主列表，改在二级详情中按需说明。

## 分包与启动链

- 根 `App.tsx` 只保留主题、基础设置摘要、外壳与路由；首页、模型、运行/服务、软件接入、Agent、活动和设置均使用 `React.lazy` 路由分包。
- 生产构建生成独立 `OverviewPage`、`ModelsPage`、`BackendsPage`、`IntegrationsPage`、`AgentPage`、`UsagePage`、`AuditPage` 和 `SettingsPage` chunk。
- 首页页面 chunk 为 4.14 kB（gzip 1.77 kB）；Agent、活动图表、远程模型搜索与后端编辑器不在该页面 chunk 中。
- Error Boundary、启动超时恢复和 Rust 开发前端守卫继续位于最小启动链，不参与懒加载。

## 浏览器与无障碍回归

- 9 个一级/子页面在 1440×920、1280×800、980×680、880×620 共 36 个组合中全部有标题、有效内容且无横向溢出。
- 以 1024×640 和 853×533 模拟 125% 与 150% 有效视口，共 12 个关键页面组合全部无横向溢出。
- 深色与浅色均验证设置和 Agent 页面可读；抽屉初始焦点落在关闭按钮，Tab/Shift+Tab 保持在对话框内，Escape 关闭后焦点回到触发按钮。
- Drawer 背景不再形成第二个读屏“关闭”按钮；对话框具有 `aria-modal`、可读标题和焦点循环。
- 重启开发服务后的浏览器控制台只有 Vite/React 开发信息，错误与警告均为 0。

## Tauri 与白屏恢复

- 真实 Tauri 1.0.1 启动完成：SQLite schema v8、Gateway `127.0.0.1:10100` 和托盘均成功就绪。
- 未携带客户端 Key 访问 `/v1/models` 返回 401，证明界面拆分未放宽 Gateway 认证。
- 使用非 HAL100 HTML 模拟开发前端异常时，Rust 记录 `desktop_frontend_startup_blocked`，随后记录 `desktop_frontend_failure_view_shown`；用户得到带诊断码和重新加载动作的失败页，而不是白屏。
- `ApplicationErrorBoundary` 和 `startup-recovery` 自动测试继续覆盖渲染异常与启动超时两条恢复链。

## 最终质量闸门

- `pnpm check`：Biome 71 个文件通过；Desktop 类型检查与 23 项测试通过；Agent Kernel 类型检查与 27 项测试通过；Cargo fmt、Clippy `-D warnings` 通过；Rust workspace 228 项非忽略测试通过。
- `pnpm --filter @hal100/desktop build` 与生产 Vite 分包构建通过。
