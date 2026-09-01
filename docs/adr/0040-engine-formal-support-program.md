# ADR-0040：推理引擎正式支持分级与实施顺序

- 状态：已接受
- 日期：2026-08-27

## 背景

协议层已经预留九种引擎身份；当前 Apple Silicon llama.cpp、Ollama 与 MLX-LM Apple Silicon/Metal
回环单元达到正式支持，vLLM和MLC LLM等共同适配器可以连接但仍未达到正式支持，不代表HAL100能证明服务身份、模型内容、
容量或全部OpenAI兼容行为。直接把预留枚举显示为“支持”会让平台营销范围、真实硬件能力和运行
方案证据混为一谈。

候选引擎也不是同类产品：MLX-LM是Apple Silicon本机运行时；vLLM、SGLang和TensorRT-LLM主要
是Linux GPU服务；OpenVINO以Intel硬件与OVMS元数据为重点；MLC LLM强调跨平台编译产物；
LMDeploy又存在不同执行后端。它们公开的版本、健康和模型身份证据不一致。

## 决策

1. 正式支持按`engine × platform × architecture × accelerator × deployment variant`验收，不按
   引擎枚举一次性开放。
2. 支持状态固定为`reserved`、`connected`、`verifiedExternal`和`managed`。只有后两者算正式
   支持；通用协议能调用不等于具体引擎已验证。
3. 外部适配器升级为目标感知、多实例合同，但目标只能来自Rust读取的已保存后端。适配器不能从
   Pi/WebView接收任意URL、命令、路径或凭据。
4. 运行方案不再假定所有模型都有digest。spec v3使用`contentDigest`、`repositoryRevision`、
   `deploymentFingerprint`和`catalogIdentity`四类证据，并在界面与Agent中如实显示强度。
5. 没有官方唯一身份端点的服务只能保持`connected`，除非HAL100拥有固定依赖闭包和受控启动器；
   不允许用显示名称、端口或OpenAI响应形状冒充引擎身份。
6. Windows和Linux先建立从源码运行、原生能力探测和真机验收基线，不制作安装包。每个GPU或NPU
   能力必须有原生证据。
7. 实施顺序为共同合同、平台基线、vLLM、MLX-LM、MLC LLM、OpenVINO、SGLang、LMDeploy、
   TensorRT-LLM，最后收口Rust确定性智能选择。
8. Pi权限保持不变：只能读取脱敏目录和申请一次性计划；安装、生命周期、证据重验、路由和成功
   谓词继续由Rust拥有。

完整分解、官方资料与验收矩阵见
[推理引擎正式支持计划](../INFERENCE_ENGINE_SUPPORT_PLAN.md)。

## 影响

- 新引擎可以复用同一运行方案和Gateway闭环，同时保留真实证据差异。
- “支持Windows/Linux”必须有对应源码运行和真机结果，不再从引擎官网的理论平台列表推断。
- MLX-LM、MLC LLM或LMDeploy若缺少强服务身份，可能需要受控本机运行时，实施成本高于普通后端。
- 计划跨越迭代51—60，但任一迭代仍只交付一条可验收纵向，不并行制造半成品。

## 非目标

本决策不立即安装或运行任何新引擎，不承诺全部硬件变体同步支持，不开放任意插件执行，也不引入
签名、公证、安装包、自动更新、正式升级或发布流程。
