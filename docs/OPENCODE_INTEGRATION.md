# OpenCode集成开发说明

- 状态：迭代3已完成；官方OpenCode CLI 1.18.11和1.17.9隔离验收通过
- 核对日期：2026-08-18
- 全局目标：`~/.config/opencode/opencode.json`或现有`opencode.jsonc`
- Provider ID：`hal100`
- 固定Base URL：`http://127.0.0.1:10100/v1`

## 1. 上游格式依据

实现以OpenCode当前官方文档为准：

- [Config](https://opencode.ai/docs/config/)说明JSON与JSONC均受支持；全局配置位于`~/.config/opencode/opencode.json`，项目配置优先级高于全局配置。
- [Providers](https://opencode.ai/docs/providers)说明自定义OpenAI兼容Provider使用`@ai-sdk/openai-compatible`，Chat Completions对应`/v1/chat/completions`。
- Config文档的文件变量语法允许`{file:...}`读取独立凭据文件。因此HAL100不会把OpenCode专属Key明文嵌入全局配置。

OpenCode配置格式属于外部可变契约。升级适配器前必须重新核对官方文档和Schema，并运行JSON、JSONC、工具调用、流式、取消和Usage回归。

HAL100当前自动验收下限为1.17.9。检测到更早的稳定版本时只给出升级警告，不会静默修改或安装OpenCode。1.15.10在同一隔离测试中可完成配置迁移但未产生模拟回答，因此不列入兼容范围；当前稳定1.18.11和保守下限1.17.9均完成真实CLI、SSE、工具定义和Usage闭环。

## 2. 当前Provider

HAL100只管理`provider.hal100`，不写入或修改`model`：

```json
{
  "provider": {
    "hal100": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "HAL100 · 由 HAL100 管理",
      "options": {
        "baseURL": "http://127.0.0.1:10100/v1",
        "apiKey": "{file:<HAL100应用数据目录>/credentials/opencode-gateway.key}"
      },
      "models": {
        "hal100-active": {
          "name": "HAL100 当前模型"
        }
      }
    }
  }
}
```

`hal100-active`是稳定客户端模型名。托管 llama.cpp模型健康后 Gateway动态指向随机认证回环引擎，停止时恢复此前活动后端。迭代5已完成多后端别名、安全排空与原生确认强制切换；OpenCode仍只使用稳定Base URL和模型名，无需随路由变化修改配置。

## 3. 确认和写入事务

配置使用两段式用户确认：

```text
只读检测
→ 生成5分钟有效的一次性计划
→ 仅返回HAL100将管理的语义字段，不返回现有配置内容或Key
→ 用户点击“确认并应用配置”
→ 再次核对原文件SHA-256
→ 时间戳备份
→ 同目录临时文件、fsync、原子替换
→ 重新解析并验证provider.hal100
→ SQLite事务登记所有权与Key摘要
→ 热更新Gateway凭据注册表
```

原文件在预览后发生变化、JSON与JSONC同时存在、配置超过2 MiB、路径是符号链接、`provider.hal100`已被其他软件占用或HAL100管理片段被外部修改时，操作故障关闭。写入后验证或数据库提交失败会恢复原配置并移除本次新建的Key。

## 4. 凭据与所有权

- OpenCode专属Key由两个UUID v4随机值组成，仅在确认后的Rust写入路径中出现。
- Key文件权限固定为`0600`；OpenCode配置只保存`{file:...}`引用。
- SQLite只保存SHA-256摘要、脱敏前缀和`client_app_id=opencode`。
- `integrations`表保存配置路径、凭据路径和受管Provider的语义哈希，不保存配置正文或Key。
- Gateway凭据注册表使用共享、可热更新的内存视图，应用配置后无需重启后台进程。
- 未找到HAL100安装记录时，已有`provider.hal100`和已有目标Key文件都视为他人所有，不覆盖。

## 5. 空闲性能

OpenCode检测只在软件接入页面加载或用户主动刷新时运行，不存在后台轮询、目录扫描或文件监听。CLI只从`~/.opencode/bin`、Apple Silicon Homebrew和`/usr/local/bin`三个固定候选位置发现，不执行继承`PATH`中的同名程序；版本检测清空环境、使用固定安全`PATH`、最多等待2秒并运行在阻塞任务线程。配置应用同样不占用Gateway异步I/O线程。未打开页面时，本模块没有定时器、线程或额外文件句柄。

## 6. 当前验证

自动测试全部使用随机临时目录，不修改开发机真实OpenCode配置，覆盖：

- JSONC注释、未知字段、既有Provider和默认模型保留。
- 用户占用Provider拒绝覆盖。
- 一次性确认、陈旧计划拒绝。
- 原始字节备份、`0600`Key、原子替换和成功验证。
- 强制验证失败后配置与Key回滚。
- OpenCode工具调用请求透明转发。
- 后端精确Usage归属为`opencode`。
- CredentialRegistry热更新，无需重启Gateway。

2026-08-18只读检查发现本机已有OpenCode全局JSON配置，但没有全局安装可调用的CLI。最终验收使用官方`opencode-ai@1.18.11`和`opencode-ai@1.17.9`精确版本，通过`pnpm dlx`临时执行，并设置随机临时HOME、XDG目录、项目目录、HAL100数据目录和`OPENCODE_CONFIG`；同时关闭自动更新、远程Models刷新、默认插件、LSP下载、Claude Code发现和文件监听。

真实CLI成功读取HAL100生成的`{file:...}`凭据，选择`hal100/hal100-active`，经默认`127.0.0.1:10100`Gateway完成SSE响应，向后端发送OpenCode生成的工具定义，并把精确Usage归属为`opencode`。测试结束后Gateway端口释放、无OpenCode进程残留；真实全局配置仍不含`provider`，真实HAL100数据目录仍没有OpenCode Key。该CLI只作为忽略型外部验收工具，不嵌入或分发到HAL100产品中。
