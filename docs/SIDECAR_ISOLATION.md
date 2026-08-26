# HAL100 Agent Sidecar隔离验证

- 状态：迭代1已验证
- 日期：2026-08-17
- 验证平台：Apple Silicon，macOS 26.5（25F71）

## 1. 结论

当前阶段可以可靠实施“最小进程启动面”，但不能把未签名开发版描述为正式 App Sandbox。HAL100因此采用两层策略：

1. 所有平台必须执行的稳定基线：固定路径、固定工作目录、空继承环境、独立会话目录、私有 RPC和 Rust Tool Broker。
2. macOS未签名开发期额外执行的拒绝探针：显式通过弃用的 `sandbox-exec`运行自动化测试，证明当前系统版本能够阻止外部文件、网络和任意子进程。

第二层是测试工具，不是产品安全承诺，也不替代第一层。

## 2. 官方平台约束

[Apple App Sandbox文档](https://developer.apple.com/documentation/security/app-sandbox)说明，沙箱通过 entitlements限制文件、网络和系统资源；[Entitlements文档](https://developer.apple.com/documentation/bundleresources/entitlements)说明这些权限存储在可执行文件的代码签名中。当前项目明确不处理签名，因此不在开发版上伪装成正式 App Sandbox。

Apple的[嵌入式命令行工具指南](https://developer.apple.com/documentation/xcode/embedding-a-helper-tool-in-a-sandboxed-app)要求对 helper签名并配置继承 entitlement。Apple的[沙箱违规诊断文档](https://developer.apple.com/documentation/security/discovering-and-diagnosing-app-sandbox-violations)进一步说明，直接创建的 helper会继承主应用能力；如果组件需要不同能力，应使用 XPC服务、登录项或独立 helper app。

HAL100主进程未来需要下载、模型文件和安装管理，而 Agent Kernel不应继承这些能力，所以“主应用沙箱 + 普通 Node子进程”不是最终的最小权限方案。

## 3. 已实现的稳定启动基线

`hal100-platform`提供 `AgentKernelLaunchSpec`与 `prepare_agent_kernel_command`：

- 运行时和入口必须存在并完成规范化。
- Sidecar入口及工作目录必须位于配置的 Workspace根目录。
- 清除父进程全部环境变量。
- 只设置 `HOME`、`TMPDIR`、`LANG`、`NO_COLOR`、`NODE_NO_WARNINGS`和 `HAL100_RPC_VERSION`。
- HOME与 TMPDIR位于单次会话目录。
- 不继承 PATH、SSH Agent、HTTP代理、用户 Shell变量和任何 API Key。
- stdin、stdout和 stderr全部使用专用管道。
- 平台隔离模式显式选择；请求开发沙箱失败时不自动回退。

同机安装官方 Pi Coding Agent不会扩大该启动面：HAL100不执行全局`pi`命令，不继承
`PATH`、`PI_CODING_AGENT_DIR`或`PI_CODING_AGENT_SESSION_DIR`，临时`HOME`也不会解析
用户的`~/.pi/agent`配置、认证、扩展和会话。内置Sidecar只加载Workspace固定的 Pi Core
库；官方 Pi的安装、升级、运行和卸载具有独立生命周期。

该基线未来可在 Windows启动器上实现等价规则，不依赖 SBPL。

## 4. macOS开发拒绝探针

开发 profile只允许：

- 读取锁定的 Node运行时、HAL100 Workspace、会话目录及系统运行库。
- 写入单次会话目录。
- `sandbox-exec`替换自身为精确 Node二进制。
- 使用继承的 RPC管道。

它显式拒绝所有网络；未授予任意进程执行。测试结果：

| 验证项 | 结果 |
| --- | --- |
| 加载 Pi Agent Core并完成 ping | 通过 |
| 完成 Pi→Rust模拟工具闭环 | 通过 |
| 读取会话目录外测试文件 | `EPERM/EACCES` |
| 写入会话目录外测试目录 | `EPERM/EACCES` |
| 连接本机随机监听端口 | `EPERM/EACCES` |
| 启动 `/bin/echo` | `EPERM/EACCES` |
| 继承 PATH/SSH Agent/HTTP代理 | 未继承 |
| Sidecar退出码与遗留进程 | 0；无遗留进程 |

单次性能样本：冷启动到 pong为171.1 ms，物理内存39 MB，退出2.9 ms。相比未沙箱单次159.7 ms样本，冷启动增加约11.4 ms。

完整沙箱测试连续运行10轮全部通过；结束后匹配的会话临时目录为0、Agent Kernel遗留进程为0。测试根目录由 RAII清理器管理，断言失败或 panic时也会回收。

## 5. 残余风险

- `sandbox-exec`已被本机手册标记为弃用，SBPL和 `system.sb`属于 Apple私有接口。
- `system.sb`允许部分标准系统路径和 IPC；本测试不是完整形式化隔离证明。
- Workspace必须可读以加载 Node模块，受损依赖可以读取 HAL100源码与锁定依赖。
- 普通 `ProcessBoundaryOnly`模式仍以当前用户权限运行；最小环境不会撤销 POSIX文件权限或网络权限。
- App Sandbox的通用网络客户端 entitlement不是 HAL100专属端口白名单。若未来要求 Sidecar完全无网络，需要由 Rust通过 RPC代理模型传输。

## 6. 条件触发方向（不属于当前路线）

项目当前处于开发初期，不规划签名、公证、安装包或正式分发。本节只记录未来若产品范围
明确扩大时需要重新评审的技术方向，不能据此自动创建当前开发任务。

1. Rust Tool Broker继续作为所有平台的唯一执行与授权权威。
2. 只有未来明确进入签名与分发范围后，才验证原生 XPC服务或独立 helper app包装 Agent Kernel。
3. 优先评估 Sidecar无网络 entitlement、模型流量通过 Rust RPC传递的方案。
4. 只有未来明确启动Windows开发后，才映射到受限令牌、Job Object、显式环境块和进程生命周期控制。
5. 不论 OS沙箱是否可用，都保留当前拒绝测试和应用层故障关闭测试。
