# ADR-0008：Agent Sidecar进程隔离策略

- 状态：已接受
- 日期：2026-08-17

## 决策

当前未签名、未打包的内部开发版不宣称使用正式 macOS App Sandbox。所有 Agent Kernel启动必须先经过 HAL100平台启动器，执行路径约束、固定工作目录、清空继承环境、独立 HOME/TMPDIR和专用管道。

保留一个显式的 `MacOsDevelopmentSandbox`模式，仅使用系统现存但已弃用的 `sandbox-exec`做自动化拒绝测试。请求该模式时如果工具或配置不可用，启动必须失败，不能静默降级为普通进程。正常开发运行仍标记为 `ProcessBoundaryOnly`，文档必须保留“第三方依赖以当前用户权限运行”的残余风险。

进入签名阶段后，优先验证具有独立权限集合的 XPC服务或 helper app包装 Agent Kernel。不能仅依靠普通子进程继承主应用沙箱，因为 HAL100主进程未来需要模型、下载和安装能力，而 Agent Kernel应拥有更窄的权限。

## 原因

- Apple将 App Sandbox权限存储在可执行文件代码签名的 entitlements中；当前范围明确不处理签名、公证和发布。
- Apple说明直接启动的 helper会继承主应用沙箱；需要不同能力时建议使用 XPC服务、登录项或独立 helper app。
- 本机 `sandbox-exec`手册明确标记该命令已弃用，系统 `system.sb`也声明属于可能随时变化的私有接口。
- Rust Tool Broker、精确 Schema、确认令牌和确定性 Executor必须独立于 OS沙箱成立，不能把安全正确性押在平台私有配置上。

## 已实施控制

- 规范化运行时、入口、Workspace、工作目录和会话目录。
- 拒绝 Workspace之外的入口及工作目录。
- 清空父进程环境；只注入6个非凭据变量。
- 不继承 PATH、SSH_AUTH_SOCK、HTTP(S)_PROXY或 API Key。
- 独立 HOME与 TMPDIR；stdin/stdout/stderr为专用管道。
- 开发沙箱只读访问固定 Node与 HAL100 Workspace，只写会话目录，拒绝网络和任意子进程。
- 自动化验证真实 Pi工具闭环、文件读写拒绝、网络拒绝、进程拒绝和退出回收。

## 后果

- 当前稳定防线是进程边界、最小启动面和 Rust应用层权限，不是正式 OS沙箱。
- `sandbox-exec`测试可能在未来 macOS更新后失效；失效必须作为测试失败处理，不允许删除测试后继续宣称隔离。
- Workspace在开发沙箱内可读，因此依赖代码仍可读取 HAL100源码和锁定包；测试重点是阻止访问用户数据、网络和外部写入。
- 未来正式沙箱设计可能要求增加一个原生 XPC/helper包装层，并重新设计 Sidecar模型传输，使其无需通用网络客户端权限。

## 官方依据

- [Apple：App Sandbox](https://developer.apple.com/documentation/security/app-sandbox)
- [Apple：Embedding a command-line tool in a sandboxed app](https://developer.apple.com/documentation/xcode/embedding-a-helper-tool-in-a-sandboxed-app)
- [Apple：Discovering and diagnosing App Sandbox violations](https://developer.apple.com/documentation/security/discovering-and-diagnosing-app-sandbox-violations)
- [Apple：Entitlements](https://developer.apple.com/documentation/bundleresources/entitlements)
