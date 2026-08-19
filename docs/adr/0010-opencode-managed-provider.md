# ADR-0010：OpenCode受管Provider与独立凭据文件

- 状态：已接受
- 日期：2026-08-18

## 决策

HAL100通过OpenCode全局配置中的`provider.hal100`接入Gateway，使用`@ai-sdk/openai-compatible`和固定Base URL。OpenCode专属Key保存于HAL100应用数据目录的`0600`文件，配置通过`{file:...}`引用。

所有修改必须经过“一次性计划预览→用户明确确认→摘要复核→备份→原子替换→验证”的流程。SQLite以Provider语义哈希和目标路径记录所有权；没有匹配安装记录时不覆盖同名Provider或凭据文件。

## 原因

- 独立Key使Gateway无需读取OpenCode内部数据即可把Usage准确归属为OpenCode。
- 文件引用避免在可能被同步、复制或展示的主配置中嵌入明文Key。
- 语义预览不会把用户现有Provider中的凭据带到前端。
- 摘要复核避免覆盖预览后发生的手工编辑。
- 受管片段哈希使HAL100能区分自己的配置和同名用户配置。

## 约束

- 不自动修改OpenCode默认模型。
- 项目级配置只诊断，不自动修改。
- JSONC补丁必须保留注释、未知字段和无关Provider。
- JSON与JSONC全局文件同时存在时故障关闭。
- 验证或数据库提交失败必须回滚。
- OpenCode检测为用户触发的按需任务，不加入后台轮询。
- 真实OpenCode版本验收缺失时，不得只凭模拟协议测试宣称全部完成。
