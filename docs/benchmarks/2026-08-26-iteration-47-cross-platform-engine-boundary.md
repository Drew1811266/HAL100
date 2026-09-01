# 迭代47：跨平台多引擎边界验收

- 日期：2026-08-26
- 范围：协议领域模型、Rust托管适配器、运行方案引擎身份与SQLite迁移
- 非范围：Windows/Linux可执行版本、新引擎下载或启动、发行流程

## 实现证据

- `hal100-protocol`独立定义9种受控引擎身份，以及所有权、部署位置、协议、平台、加速器和模型格式；序列化
  合同不再依赖混合含义的后端名称。
- 现有6类`BackendKind`保持线兼容，同时确定性映射到引擎、所有权和Gateway协议三个维度。
- `hal100-infra::InferenceEngineAdapter`为对象安全接口，覆盖描述符、状态、容量、安装/移除计划和
  启停生命周期；`LlamaCppManager`是当前唯一实现。
- `RuntimeProfileManager`改为依赖适配器并读取其引擎身份。非当前适配器方案判定为需要修复，
  不会使用当前运行时误激活。
- SQLite schema v11从v10原样保留既有方案，并只接受9个固定引擎存储键；未知身份由数据库拒绝。

## 自动化结果

- `cargo test -p hal100-protocol -p hal100-infra`：协议29项通过；Infra默认133项通过，重型/联网项
  按既有规则忽略；Gateway端到端10项通过。
- schema v11专项测试从真实v10表插入旧`llama.cpp`方案，迁移后验证数据保留、`sglang`白名单
  可写和未知引擎拒绝。
- 完整`pnpm check`与`pnpm build`结果以本迭代最终门禁记录为准。

## 结论

跨平台支持获得了可演进但不扩权的首条边界：未来引擎可以注册为独立Rust适配器，当前软件仍只
承诺已验证的Apple Silicon llama.cpp。该变化不增加Pi工具、Gateway热路径、后台轮询或任意执行面。
