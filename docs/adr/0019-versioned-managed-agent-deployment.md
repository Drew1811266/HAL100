# ADR-0019：版本化外部 Agent 受管部署配方

- 状态：已接受
- 日期：2026-08-21
- 决策范围：外部 Agent 安装、用户安装共存、供应链与 Agent 权限边界

> 后续状态：本文保留迭代22的决策背景；其中“传递依赖未锁定”的残余风险已由[ADR-0020](0020-managed-dependency-closure-and-removal.md)关闭。

## 背景

HAL100 已能检测、配置和断开 OpenCode、Pi Coding Agent、OpenClaw 与 Hermes Agent，但用户仍需先独立安装 CLI。为了让内置 Agent 完成“部署—配置—调试—检测”的完整任务，需要增加安装能力；直接开放 Shell、通用包管理器、全局 npm 或任意安装命令会把模型输出变成当前用户权限下的通用代码执行，不符合 Rust Core 是唯一授权权威的架构。

官方 Pi CLI 的包身份是 [`@earendil-works/pi-coding-agent`](https://www.npmjs.com/package/%40earendil-works/pi-coding-agent)，而 Pi 项目也明确说明自身没有内建权限系统，应通过扩展或 OS 级控制施加限制；HAL100 因此不能把 Pi 的模型层确认当作系统授权。[官方源码](https://github.com/earendil-works/pi)

## 决策

采用“每个外部 Agent 一条版本化部署配方”的白名单模型，不引入通用安装器。迭代22只验收 Pi Coding Agent 配方：

- 固定包名`@earendil-works/pi-coding-agent`、版本`0.84.2`、npm Registry、CLI入口和顶层SRI。
- Agent只能调用`plan_external_agent_installation`生成一次性计划；必须先完成同一`integrationId`的状态检测，且每项任务最多一个写计划。
- Rust原生确认后才执行底层计划；Sidecar不获得npm路径、安装目录、底层计划ID或执行能力。
- 只允许HAL100应用数据目录下的私有安装，不使用global prefix、不改PATH/HOME、不写系统目录，也不修改用户Pi配置。
- 用户候选路径始终排在私有副本之前；一旦检测到用户或受管Pi，就拒绝新安装。
- npm通过固定绝对路径、有界输出/超时和清空环境运行。`npm pack`与安装都使用`--ignore-scripts`；先校验归档SRI，再从本地归档安装。
- 安装发生在同一受控父目录内的UUID暂存目录。验证包名、版本、`bin.pi`和`pi --version`后原子重命名为`runtime`；失败只清理本次暂存目录。
- 安装与Gateway配置是两个独立授权。私有Pi安装完成不会自动写`~/.pi/agent/models.json`或创建凭据，后续仍需专用配置事务和第二次原生确认。
- OpenCode、OpenClaw与Hermes Agent在没有各自验收配方前固定返回`deployment_recipe_unavailable`，不得套用Pi配方。

## 模块边界

- `ManagedExternalAgentDeploymentManager`：配方、包元数据、暂存、验证、原子启用和固定审计。
- `PiCodingAgentIntegrationAdapter`：检测、用户配置、Gateway凭据、接入与断开；不承担安装过程。
- `agent_coordinator`：从用户目标推导检测与安装计划能力。
- `agent_tools`：目标绑定、前置检查、脱敏外层计划。
- `AgentService`：计划废弃、原生确认后的执行和结果审计。
- Agent Kernel Sidecar：TypeBox参数约束和工具调用编排；不运行npm。

## 被拒绝的方案

### 全局执行`npm install -g`

会修改用户prefix/PATH生态，可能覆盖官方Pi，与HAL100私有生命周期冲突；拒绝。

### 给模型开放通用Shell或包管理器工具

参数空间无界，无法证明目标、路径、脚本和副作用；拒绝。

### 把私有Pi放在用户候选之前

会让HAL100静默劫持用户明确安装和升级的官方CLI；拒绝。

### 安装后自动配置Gateway

把一次安装确认扩张成用户HOME配置写授权，破坏最小授权和事务边界；拒绝。

## 后果与残余风险

首次安装需要网络和已存在的受支持npm，但HAL100不会自行安装Node/npm。顶层包归档由固定SRI约束，生命周期脚本被禁用；然而npm仍会按顶层包声明从Registry解析传递依赖，当前配方没有提交完整依赖锁图，因此还不是完整依赖闭包的可复现部署。增加第二条部署配方前，应评估：提交已验收lock graph、离线供应已验收运行时，或为每种生态实现等价的完整依赖校验。

本决策不增加Accessibility、Screen Recording、Apple Events、root Helper、长期后台监控或桌面自动化权限。
