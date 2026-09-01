# 推理引擎真机验收runner手册

本文只描述开发期真实服务验收主机和证据流程，不是HAL100安装、签名、公证、打包、自动更新或
正式升级方案。runner不会由HAL100自动注册，引擎、模型和驱动也必须由操作者在隔离主机上预先
准备；工作流只连接同机回环服务。

## 1. 生成当前目标清单

支持矩阵是唯一坐标来源。以下只读命令默认列出尚未正式验收的支持格：

```bash
node scripts/list-engine-acceptance-targets.mjs
```

当前输出应为25格；`--all`会连同3个已正式验收的外部格一起列出28格。每条记录包含唯一
`environmentName`、runner平台、引擎、加速器、适配器变体、合同修订以及需要配置的secret名称，
但不包含任何secret值、端点、模型ID或本地路径。

## 2. runner标签与隔离边界

| 目标 | 必需标签 |
| --- | --- |
| Apple Silicon macOS | `self-hosted`, `macOS`, `ARM64`, `hal100-acceptance` |
| Linux x86_64 | `self-hosted`, `Linux`, `X64`, `hal100-acceptance` |
| Linux aarch64 | `self-hosted`, `Linux`, `ARM64`, `hal100-acceptance` |
| Windows x86_64 | `self-hosted`, `Windows`, `X64`, `hal100-acceptance` |

主机必须专用于受审查的验收运行，Rust/Cargo和仓库声明的Node版本已可用。runner服务账户只能访问
该验收主机需要的目录；不能把个人HOME、浏览器凭据、SSH密钥或其他项目secret暴露给任务。引擎
服务必须监听`127.0.0.1`并使用显式端口，不得用主机名、IPv6、远端URL、代理或重定向代替本机证据。

运行wrapper会先调用共享的脱敏环境预检。它只验证当前引擎所需变量存在、有界，API root严格为
带显式端口和尾部斜杠的`http://127.0.0.1/.../`，可选vLLM密钥在提供时有界且不含控制字符，以及需要显式设备的引擎使用允许的加速器键；
预检不连接服务，也不会打印端点、模型、版本或API Key。通过预检不代表真机验收成功，后续Rust
原生探针与适配器资格检查仍是唯一权威。

## 3. 受保护Environment

为目标清单中的精确`environmentName`创建GitHub Environment，并配置required reviewers。所有目标
都需要下列secrets：

- `HAL100_ACCEPTANCE_API_ROOT`：已准备服务的回环OpenAI API root；
- `HAL100_ACCEPTANCE_MODEL_ID`：精确服务模型ID；MLC LLM为该主机上的绝对部署目录。

除LMDeploy外还需要`HAL100_ACCEPTANCE_ENGINE_VERSION`。vLLM若启用认证，可额外配置
`HAL100_VLLM_API_KEY`。这些值不得放入workflow dispatch输入、runner标签、提交信息或普通仓库变量。

## 4. 执行与审查顺序

1. 在隔离主机上人工启动已固定版本、模型修订和目标设备的回环服务。
2. 手动触发`HAL100 live engine acceptance`，只选择清单中的引擎、平台和加速器。
3. 无秘密Ubuntu job先把选择解析为唯一manifest支持格；通过后才请求受保护Environment和真机。
4. 真机先检查secret存在性并运行共享的无请求环境预检，再由Rust原生探针证明平台、架构和加速器，
   之后才发送服务请求；v4
   产物会将探针修订、精确支持格和设备类别SHA-256写入`nativeHostProbeV1`，但不会保存CPU/设备
   原文、序列号、存储路径、端点、命令或凭据；模型证据同样只保存类型、算法与域分离SHA-256。
5. 成功运行只上传保留14天的create-new脱敏JSON；下载后人工复核版本、模型不可变修订、主机来源、
   七类证据和三项控制面韧性。
6. 使用`hal100-engine-acceptance-import`和标准v4账本生成新的候选账本，检查diff后再替换标准
   账本。导入和重新资格验证只接受`nativeHostProbeV1`；`legacyHostSummaryV1`只用于保留三条v1
   历史记录。工作流永远不自动导入、覆盖或晋级。
7. 停止验收服务并清理专用临时模型、密钥和运行产物；不得删除用户自己的引擎或模型。

结构夹具、源码CI、托管runner、宿主拥有某类GPU或一次普通聊天成功都不能替代以上流程。
