# MLC LLM macOS Metal 正式验收阻塞记录

- 日期：2026-08-31
- 目标支持格：MLC LLM / macOS / aarch64 / Metal / external
- 宿主：Apple M1，16 GiB，macOS 26.5
- 结论：不晋级，继续保持 `connected`

本记录描述一次真实外部服务验收失败。它不是正式支持证据，不能导入
`contracts/inference-engines/v1-acceptance-evidence.json`，也不授予运行方案激活权限。

## 固定输入

- 官方模型：`mlc-ai/Qwen3.5-2B-q4f16_1-MLC`
- 固定 revision：`dd74e9c8a20c4546df85c844103bff87b6dcacad`
- 32 个 Git LFS 对象逐项与官方 SHA-256 元数据核对通过，其中包含 31 个权重分片和
  `tokenizer.json`
- HAL100 工具模板派生配置 SHA-256：
  `0f44e9d538e8eb94556d3b2eb834c46818f5c07822c196d542bb2b3208233cd9`
- 固定 Metal 动态库 SHA-256：
  `2abf995a99a3a5cadc645a7412fb50f9044b36d7a7d5d1b58212222d1d7331e4`
- 编译参数：最大上下文 4096、prefill chunk 512；动态库为 arm64 Mach-O，大小
  3,815,864 bytes
- 稳定运行时：`mlc-llm-cpu 0.20.0.dev0`、`mlc-ai-cpu 0.20.0`、
  `apache-tvm-ffi 0.1.11`
- wheel SHA-256：
  - MLC LLM：`a7a59b338a1ebebcc1c3eab6a6871ea1f937b71a814b3ab913522a2d1f7d41db`
  - MLC AI：`26e8a291ef792ca5bea48570b74b740331b7558b2168b71abafadddb0e3a23d9`
  - TVM FFI：`6ae51cc7df415b5f373a9df4baa1165a65608e519bea81e7dd23428f00eeb689`

派生配置只改变协议模板和运行所需的令牌配置，不改变权重。模板强制唯一的
`hal100_protocol_probe` Python 风格调用；`role_empty_sep`预填空思考段；`stop_str`设为空数组；
并依据固定 tokenizer 把错误的 Qwen2 令牌值修正为 Qwen3.5 的 `248044`/`248046`。

## 已验证项

- 官方服务真实暴露固定本地 MLC 部署目录。
- Rust 有界部署读取、权重清单/分片/tokenizer 哈希和恰好一个
  `{function_string}`模板插槽通过。
- 单次非流式请求精确返回一个 `hal100_protocol_probe` 调用；MLC 返回的参数对象只在已绑定
  MLC 身份的 Gateway 后端被规范化为 OpenAI JSON 字符串。
- 单次普通流式请求返回 `OK`、`finish_reason=stop`、正数 Usage 和 `[DONE]`。
- 带工具的流式请求仍按既定合同故障关闭。

## 阻塞原因

MLC LLM 0.20 在同一服务进程完成工具请求后继续处理后续请求时，后台调度线程可稳定触发：

```text
InternalError: Check failed: n <= it->second.available_history_num (...): rollback ...
```

问题在 `local` 和 `interactive` 模式、串行和并发请求中均可复现；即使工具请求后改发普通非流式
请求，后续请求仍会失败。服务会先返回 HTTP 200，再由后台调度线程退出，因此仅在 HAL100 Gateway
增加单飞或把正式验收并发从 4 降为 1 都不能恢复正确生命周期，也会掩盖真实运行时故障。完整
live acceptance 因而在 20 次/每波 4 并发稳定性和持续生命周期门槛前失败，未生成运行产物。

另验证当前官方 nightly 组合 `mlc-llm-nightly-cpu 0.26.dev6`、
`mlc-ai-nightly-cpu 0.26.dev246`、`apache-tvm-ffi 0.1.13`：稳定版动态库因 ABI 不兼容崩溃；
nightly 编译器又分别出现缺失 `tvm.tirx.is_buffer_var` 和 TIR `DeclBuffer` 表示不一致。没有找到可由
同一官方包组安全编译并运行的相干 0.26 组合，不能拼接不同代际包形成正式证据。

## HAL100 决策

- 不生成、不导入验收记录；支持矩阵与账本计数不变。
- 不用降低稳定性门槛、Gateway 单飞或伪造引擎版本绕过阻塞。
- 保留已完成的本地部署内容指纹、MLC 专属非流式响应规范化和流式工具故障关闭。
- 修复真实配置暴露出的验证缺陷：`system_template`允许 64 KiB 内的 CR/LF/TAB 多行文本，同时
  拒绝 NUL、其他控制字符、空模板和超限模板。
- 待官方提供不存在上述回滚故障、且编译/运行包相干的版本后，使用同一固定输入重新执行完整
  协议、20 次稳定性、运行方案生命周期和三项控制面韧性验收。

验收结束后本机回环服务已停止；模型、隔离 Python 环境和编译产物保留在系统临时目录用于本轮
复核，没有写入用户配置，也没有改变正式账本。
