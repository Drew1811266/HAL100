# 迭代48：宿主能力与引擎兼容性验收

- 日期：2026-08-26
- 范围：平台中立能力快照、引擎兼容性、运行方案门禁和桌面能力目录
- 非范围：Windows/Linux可执行版本、新推理引擎、发行流程

## 实现证据

- `NativeSystemProbe`只在调用时读取固定系统字段，生成版本化`HostCapabilitySnapshot`；当前
  Apple Silicon快照明确包含aarch64、CPU和Metal，不存在轮询器或后台GPU扫描。
- `InferenceEngineDescriptor`新增固定CPU架构要求，并通过纯函数同时检查平台、架构和加速器
  交集；不兼容原因是固定枚举，不包含命令、路径或原始系统输出。
- `RuntimeProfileManager`保存和列举方案时复用兼容性结论。引擎虽已安装但宿主不兼容时，方案
  进入`needsRepair`且不能保存或激活。
- Tauri只新增只读`get_inference_capability_catalog`命令；当前目录只列出实际注册的llama.cpp
  适配器，不把9种未来白名单身份冒充为已安装或可运行。
- 旧硬件画像和Agent系统摘要由能力快照投影，RPC v13、20项工具、SQLite schema v11及Gateway
  热路径均未变化。

## 自动化结果

- 定向协议、平台和Infra测试通过：Protocol 30项、Platform 10项、Infra默认133项；Gateway
  端到端10项通过，重型/联网项仍按既有规则忽略。
- Desktop Rust全目标检查和Desktop TypeScript类型检查通过。
- 完整`pnpm check`通过：Biome检查105个文件、Desktop 26项、Agent Kernel 34项、默认Rust
  workspace 336项测试及全目标Clippy均通过；`pnpm build`和差异卫生通过。

## 结论

HAL100现在能用同一受控事实回答“这台机器拥有什么能力”与“已注册引擎是否匹配”，运行方案不再
把安装成功等同于硬件兼容。当前结论仍严格限定于Apple Silicon/Metal，不外推Windows/Linux。
