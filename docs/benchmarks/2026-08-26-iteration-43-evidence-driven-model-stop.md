# 迭代43：证据驱动模型停止纵向

日期：2026-08-26

## 缺口证据

HAL100已有确定性的`LlamaCppManager::stop()`和模型页停止入口，但原18类Agent任务没有对应能力。
用户若在Agent页完成模型配置，仍需跨页手工停止当前模型。该缺口不需要Shell、任意文件、桌面
控制、管理员权限或新的资源所有权，因此适合作为设备感知上下文之后的首个确定性任务扩展。

## 实现边界

- Core新增`stop_model`任务、`runtime_model_stopped`谓词和`StopModel`原生动作。
- RPC v12新增`hal100.plan_model_stop`；必须先调用运行目录，只接受1—128字符的精确`modelId`，
  拒绝额外字段。
- Rust生成计划时要求该ID仍是当前活动模型；原生确认后的执行前再次读取状态，漂移即拒绝。
- 执行复用托管停止器并等待已有请求安全排空；模型文件、索引和用量记录均不删除。
- 成功只接受Rust现实复验：运行态为`Stopped`且活动模型为空。模型回答和进程退出摘要无权声明成功。

## 自动合同

- RPC工具：19/19，规范顺序、前置关系、精确参数和原生确认一致。
- Rust任务：19/19；受控任务11/11，原生动作10/10，成功谓词19/19。
- v4受控路由：14/14；v8开放中文：42场景覆盖19/19任务，确定性32/42、越权任务0。
- v9动作纵向：19/19；停止路径引用默认门禁中的真实引擎启停验收。
- 默认单元测试验证：错误活动ID、已停止状态均不能产生计划；停止谓词同时要求停止态和空活动ID。

## 真实Pi隔离验收

命令：

```text
cargo test -p hal100-desktop real_agent_plans_confirms_and_verifies_current_model_stop -- --ignored --nocapture
```

结果：

```text
AGENT_MODEL_STOP_LIVE context=32768 turns=2 final_state=stopped model_preserved=true
```

验收在临时数据库、Gateway、引擎状态和检查点中进行，只复用已验证的开发模型与llama.cpp资产。
Pi先读取当前活动模型，再生成一次性停止计划；伪造计划ID被拒绝。精确计划经确认后，Rust复验
运行态停止、活动模型为空、模型文件仍存在、数据库索引仍存在。任务检查点最终为
`completed/satisfied/runtimeRecheck`。测试结束后Agent运行时、Gateway任务和临时目录均回收。

## 结论

该能力完成“自然语言 → Rust任务 → Pi工具编排 → 一次性计划 → 原生确认 → 确定性执行 →
现实复验”的纵向闭环，没有扩大通用系统权限。本条证据形成时不单独宣告长期Goal完成；随后
[设备感知长上下文与连续任务验收](2026-08-26-iteration-43-device-aware-agent-context.md)补齐7/7
选择边界、20/20真实连续任务和零残留回收，最终全量门禁也已通过。
