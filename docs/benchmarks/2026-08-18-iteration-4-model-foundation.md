# 迭代4模型管理基础基准

> 历史快照：本文件记录当时实现；主WebView capability与原生确认边界已由[Alpha安全加固验收](2026-08-18-alpha-security-hardening.md)收紧。

- 日期：2026-08-18
- 机器：Apple M1，16 GiB统一内存，arm64
- 系统：macOS 26.5（25F71）
- 桌面构建：Rust 1.97.1，Tauri 2.11.5，release + benchmark-hooks
- 数据库：SQLite schema version 5

## 1. 硬件画像

实机读取到 Apple M1、`iMac21,1`、16 GiB统一内存、8个物理/逻辑核心和模型目录所在卷的可用空间。连续100次固定`sysctl + df`完整探测墙钟0.30秒，即约3毫秒/次。探测只在模型页打开或用户刷新时执行，没有计时器或隐藏窗口轮询。

## 2. GGUF导入安全回归

自动化覆盖：GGUF v3成功计划/确认、量化名识别、一次性计划、重复路径拒绝、确认前文件变化拒绝、错误扩展名、未来版本拒绝、符号链接拒绝、外部源文件保留、模型与位置事务写入、脱敏审计事件、导入后大小/修改时间变化检测。

原生选择器只在 Rust专用命令中使用。Tauri主窗口 capability仍为`core:default`，没有 Dialog或文件系统插件权限。浏览器预览中的确认按钮保持禁用。

## 3. 后台空闲

加入 Rust Dialog插件、硬件探测器、模型目录和导入管理器后，release桌面隐藏窗口样本为：

- `phys_footprint` 41 MiB，峰值43 MiB。
- `ps`五次1秒CPU样本全部为0.0%。
- RSS样本约121 MiB；预算继续以macOS物理占用口径判定。
- Gateway`/healthz`返回`ok`。
- SQLite从v3迁移到v5，现有Usage和OpenCode表未重建。
- 明确退出状态0，`127.0.0.1:10100`释放，无桌面进程残留。

物理占用与迭代3的41 MiB基线一致，仍满足隐藏窗口空闲CPU不高于0.3%、物理占用不高于80 MiB的目标。

## 4. 质量门禁

`pnpm check`通过，包含前端lint/typecheck/测试、Rust格式、全Workspace Clippy零警告、全部默认测试和协议契约。模型导入新增4个 Rust安全测试，Infra单元测试总数增至32；桌面生产前端构建和release Rust构建均通过。
