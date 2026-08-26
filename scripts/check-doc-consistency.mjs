import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = process.cwd();
const failures = [];

function read(relativePath) {
  return readFileSync(join(root, relativePath), "utf8");
}

function readJson(relativePath) {
  return JSON.parse(read(relativePath));
}

function capture(content, pattern, label) {
  const match = content.match(pattern);
  if (!match) {
    failures.push(`无法从${label}读取事实`);
    return undefined;
  }
  return match[1];
}

function expectEqual(actual, expected, label) {
  if (actual !== expected) {
    failures.push(`${label}不一致：期望 ${expected}，实际 ${actual ?? "未找到"}`);
  }
}

function expectIncludes(content, expected, label) {
  if (!content.includes(expected)) {
    failures.push(`${label}缺少：${expected}`);
  }
}

const rootPackage = readJson("package.json");
const desktopPackage = readJson("apps/desktop/package.json");
const sidecarPackage = readJson("sidecars/agent-kernel/package.json");
const tauriConfig = readJson("apps/desktop/src-tauri/tauri.conf.json");
const cargo = read("Cargo.toml");
const database = read("crates/hal100-infra/src/database.rs");
const agentRpc = read("crates/hal100-protocol/src/agent_rpc.rs");
const agentProtocol = read("crates/hal100-protocol/src/agent.rs");
const agentCapabilities = read("crates/hal100-core/src/agent_capability.rs");
const agentIntent = read("crates/hal100-core/src/agent_intent.rs");
const agentTasks = read("crates/hal100-core/src/agent_task.rs");
const agentTaskGraph = read("crates/hal100-core/src/agent_task_graph.rs");
const coreSummary = read("crates/hal100-core/src/lib.rs");
const desktopApi = read("apps/desktop/src/lib/desktop-api.ts");
const currentState = read("docs/CURRENT_STATE.md");
const readme = read("README.md");
const roadmap = read("docs/ROADMAP.md");
const agentEvaluation = readJson("contracts/agent-evals/v1-config-tasks.json");
const piIntentEvaluation = readJson("contracts/agent-evals/v2-pi-intent-adjudication.json");
const livePiIntentEvaluation = readJson("contracts/agent-evals/v3-pi-live-intent.json");
const controlledRoutingEvaluation = readJson("contracts/agent-evals/v4-controlled-routing.json");
const taskCheckpointEvaluation = readJson("contracts/agent-evals/v5-task-checkpoints.json");
const successPredicateEvaluation = readJson("contracts/agent-evals/v6-success-predicates.json");
const boundedClarificationEvaluation = readJson(
  "contracts/agent-evals/v7-bounded-clarification.json",
);
const openChineseEvaluation = readJson("contracts/agent-evals/v8-open-chinese-inputs.json");
const controlledActionEvaluation = readJson(
  "contracts/agent-evals/v9-controlled-action-verticals.json",
);
const compositeTaskGraphEvaluation = readJson(
  "contracts/agent-evals/v10-composite-task-graphs.json",
);
const compositeRecoveryEvaluation = readJson("contracts/agent-evals/v11-composite-recovery.json");
const deviceContextStabilityEvaluation = readJson(
  "contracts/agent-evals/v12-device-context-stability.json",
);
const agentIntentSchema = readJson("contracts/agent-intent/v1-schema.json");

const version = rootPackage.version;
const workspacePackage = capture(
  cargo,
  /\[workspace\.package\]([\s\S]*?)(?=\n\[|$)/,
  "Cargo workspace package段",
);
const cargoVersion = workspacePackage
  ? capture(workspacePackage, /^version = "([^"]+)"$/m, "Cargo workspace版本")
  : undefined;

expectEqual(desktopPackage.version, version, "Desktop package版本");
expectEqual(sidecarPackage.version, version, "Agent Kernel package版本");
expectEqual(tauriConfig.version, version, "Tauri版本");
expectEqual(cargoVersion, version, "Cargo workspace版本");

const schemaVersion = String((database.match(/\bM::up\(/g) ?? []).length);
const rpcVersion = capture(agentRpc, /pub const AGENT_RPC_VERSION: u16 = (\d+);/, "Agent RPC版本");
const toolCount = capture(
  agentCapabilities,
  /const AGENT_CAPABILITIES: \[AgentCapabilityDescriptor; (\d+)\]/,
  "Agent工具数量",
);
const taskKindCount = capture(
  agentTasks,
  /pub const AGENT_TASK_KIND_COUNT: usize = (\d+);/,
  "Agent任务类型数量",
);
const evaluationScenarioCount = String(agentEvaluation.scenarios.length);
const piIntentScenarioCount = String(piIntentEvaluation.scenarios.length);
const livePiIntentScenarioCount = String(livePiIntentEvaluation.scenarios.length);
const livePiIntentRunCount = String(
  livePiIntentEvaluation.scenarios.length * livePiIntentEvaluation.runsPerScenario,
);
const controlledRoutingScenarioCount = String(controlledRoutingEvaluation.scenarios.length);
const taskCheckpointScenarioCount = String(taskCheckpointEvaluation.scenarios.length);
const successPredicateWorkflowCount = String(successPredicateEvaluation.workflows.length);
const successPredicateFaultCount = String(successPredicateEvaluation.faultInjections.length);
const boundedClarificationScenarioCount = String(boundedClarificationEvaluation.scenarios.length);
const openChineseScenarioCount = String(openChineseEvaluation.scenarios.length);
const openChinesePiScenarioCount = String(openChineseEvaluation.piScenarios.length);
const openChinesePiRunCount = String(
  openChineseEvaluation.piScenarios.length * openChineseEvaluation.piRunsPerScenario,
);
const controlledActionPathCount = String(controlledActionEvaluation.actionPaths.length);
const compositeTaskGraphScenarioCount = String(compositeTaskGraphEvaluation.scenarios.length);
const compositeRecoveryScenarioCount = String(compositeRecoveryEvaluation.scenarios.length);
const deviceSelectionCaseCount = String(deviceContextStabilityEvaluation.selectionCases.length);
const repeatedStandardRunCount = String(
  deviceContextStabilityEvaluation.thresholds.minimumRepeatedStandardRuns,
);
const taskCheckpointSchemaVersion = capture(
  agentProtocol,
  /pub const AGENT_TASK_CHECKPOINT_SCHEMA_VERSION: u8 = (\d+);/,
  "Agent任务检查点schema版本",
);
const taskGraphSchemaVersion = capture(
  agentTaskGraph,
  /pub const AGENT_TASK_GRAPH_SCHEMA_VERSION: u8 = (\d+);/,
  "Agent复合图schema版本",
);
const taskGraphMaxNodes = capture(
  agentTaskGraph,
  /pub const AGENT_TASK_GRAPH_MAX_NODES: usize = (\d+);/,
  "Agent复合图节点上限",
);
const taskGraphMaxDependencies = capture(
  agentTaskGraph,
  /pub const AGENT_TASK_GRAPH_MAX_DEPENDENCIES: usize = (\d+);/,
  "Agent复合图依赖上限",
);
expectEqual(
  String(compositeTaskGraphEvaluation.schemaVersion),
  taskGraphSchemaVersion,
  "Agent复合图合同版本",
);
expectEqual(
  String(compositeTaskGraphEvaluation.thresholds.maxNodes),
  taskGraphMaxNodes,
  "Agent复合图合同节点上限",
);
expectEqual(
  String(compositeTaskGraphEvaluation.thresholds.maxDependenciesPerNode),
  taskGraphMaxDependencies,
  "Agent复合图合同依赖上限",
);
expectEqual(
  String(deviceContextStabilityEvaluation.thresholds.selectionCaseRate),
  "1",
  "Agent设备档选择合同正确率",
);
expectEqual(
  String(deviceContextStabilityEvaluation.closedTiers[0].contextWindowTokens),
  "65536",
  "Agent未验收上下文关闭档",
);
expectEqual(
  String(successPredicateEvaluation.checkpointSchemaVersion),
  taskCheckpointSchemaVersion,
  "Agent成功谓词检查点版本",
);
expectEqual(
  String(boundedClarificationEvaluation.checkpointSchemaVersion),
  taskCheckpointSchemaVersion,
  "Agent有界澄清检查点版本",
);
const intentSchemaVersion = capture(
  agentIntent,
  /pub const AGENT_TASK_INTENT_SCHEMA_VERSION: u32 = (\d+);/,
  "Agent结构化意图schema版本",
);
expectEqual(
  String(agentIntentSchema.$defs.schemaVersion.const),
  intentSchemaVersion,
  "Agent结构化意图合同版本",
);
const piCoreVersion = sidecarPackage.dependencies["@earendil-works/pi-agent-core"];
const piAiVersion = sidecarPackage.dependencies["@earendil-works/pi-ai"];
expectEqual(piAiVersion, piCoreVersion, "Pi Agent Core与Pi AI版本");

expectEqual(capture(currentState, /^- 当前版本：(\S+)$/m, "当前状态版本"), version, "当前状态版本");
expectEqual(
  capture(currentState, /\| 数据库 \| SQLite WAL，schema v(\d+) \|/, "当前状态Schema"),
  schemaVersion,
  "当前状态Schema",
);
expectEqual(
  capture(currentState, /\| Agent 私有协议 \| RPC v(\d+)，/, "当前状态RPC"),
  rpcVersion,
  "当前状态RPC",
);
expectEqual(
  capture(currentState, /\| Agent 私有协议 \| RPC v\d+，(\d+) 个固定工具/, "当前状态工具数量"),
  toolCount,
  "当前状态工具数量",
);
expectEqual(
  capture(currentState, /Pi Agent Core\/Pi AI ([^；|]+)；/, "当前状态Pi版本"),
  piCoreVersion,
  "当前状态Pi版本",
);
expectEqual(
  capture(currentState, /Core已定义(\d+)类任务/, "当前状态Agent任务类型数量"),
  taskKindCount,
  "当前状态Agent任务类型数量",
);
expectEqual(
  capture(currentState, /首版配置评测(\d+)场景/, "当前状态Agent评测场景数量"),
  evaluationScenarioCount,
  "当前状态Agent评测场景数量",
);
expectEqual(
  capture(currentState, /双路裁决评测(\d+)\/\d+/, "当前状态Pi裁决评测场景数量"),
  piIntentScenarioCount,
  "当前状态Pi裁决评测场景数量",
);
expectEqual(
  capture(currentState, /真实Pi意图评测(\d+)场景/, "当前状态真实Pi意图场景数量"),
  livePiIntentScenarioCount,
  "当前状态真实Pi意图场景数量",
);
expectEqual(
  capture(currentState, /真实Pi意图评测\d+场景×\d+轮为(\d+)\/\d+/, "当前状态真实Pi意图运行数量"),
  livePiIntentRunCount,
  "当前状态真实Pi意图运行数量",
);
expectEqual(
  capture(currentState, /受控路由评测(\d+)\/\d+/, "当前状态受控路由评测场景数量"),
  controlledRoutingScenarioCount,
  "当前状态受控路由评测场景数量",
);
expectEqual(
  capture(currentState, /脱敏检查点生命周期评测(\d+)\/\d+/, "当前状态任务检查点评测场景数量"),
  taskCheckpointScenarioCount,
  "当前状态任务检查点评测场景数量",
);
expectEqual(
  capture(currentState, /schema v(\d+)脱敏检查点/, "当前状态任务检查点schema版本"),
  taskCheckpointSchemaVersion,
  "当前状态任务检查点schema版本",
);
expectEqual(
  capture(currentState, /成功谓词评测(\d+)\/\d+/, "当前状态成功谓词工作流数量"),
  successPredicateWorkflowCount,
  "当前状态成功谓词工作流数量",
);
expectEqual(
  capture(currentState, /证据故障注入(\d+)\/\d+/, "当前状态证据故障场景数量"),
  successPredicateFaultCount,
  "当前状态证据故障场景数量",
);
expectEqual(
  capture(currentState, /有界澄清评测(\d+)\/\d+/, "当前状态有界澄清场景数量"),
  boundedClarificationScenarioCount,
  "当前状态有界澄清场景数量",
);
expectEqual(
  capture(currentState, /开放中文评测(\d+)场景/, "当前状态开放中文场景数量"),
  openChineseScenarioCount,
  "当前状态开放中文场景数量",
);
expectEqual(
  capture(currentState, /真实Pi开放子集(\d+)场景/, "当前状态开放Pi场景数量"),
  openChinesePiScenarioCount,
  "当前状态开放Pi场景数量",
);
expectEqual(
  capture(currentState, /真实Pi开放子集\d+场景×\d+轮为(\d+)\/\d+/, "当前状态开放Pi运行数量"),
  openChinesePiRunCount,
  "当前状态开放Pi运行数量",
);
expectEqual(
  capture(currentState, /动作纵向矩阵(\d+)条路径/, "当前状态动作纵向路径数量"),
  controlledActionPathCount,
  "当前状态动作纵向路径数量",
);
expectEqual(
  capture(currentState, /结构化意图schema v(\d+)/, "当前状态Agent结构化意图schema版本"),
  intentSchemaVersion,
  "当前状态Agent结构化意图schema版本",
);
expectEqual(
  capture(currentState, /复合图schema v(\d+)与v10/, "当前状态Agent复合图schema版本"),
  taskGraphSchemaVersion,
  "当前状态Agent复合图schema版本",
);
expectEqual(
  capture(currentState, /与v10的(\d+)类语义/, "当前状态Agent复合图场景数量"),
  compositeTaskGraphScenarioCount,
  "当前状态Agent复合图场景数量",
);
expectEqual(
  capture(currentState, /v11恢复合同的(\d+)类语义/, "当前状态Agent复合恢复场景数量"),
  compositeRecoveryScenarioCount,
  "当前状态Agent复合恢复场景数量",
);
expectEqual(
  capture(currentState, /复合图检查点只保存最多(\d+)个节点/, "当前状态Agent复合图节点上限"),
  taskGraphMaxNodes,
  "当前状态Agent复合图节点上限",
);
expectEqual(
  capture(currentState, /每节点最多(\d+)个依赖/, "当前状态Agent复合图依赖上限"),
  taskGraphMaxDependencies,
  "当前状态Agent复合图依赖上限",
);
expectEqual(
  capture(currentState, /设备选择边界矩阵(\d+)\/\d+/, "当前状态Agent设备选择矩阵"),
  deviceSelectionCaseCount,
  "当前状态Agent设备选择矩阵",
);
expectEqual(
  capture(currentState, /32K连续任务(\d+)\/\d+/, "当前状态Agent 32K连续任务"),
  repeatedStandardRunCount,
  "当前状态Agent 32K连续任务",
);

const completedIteration = capture(currentState, /^- 已完成迭代：0—(\d+)$/m, "当前状态已完成迭代");
if (completedIteration) {
  const nextIteration = String(Number(completedIteration) + 1);
  const currentIteration = currentState.match(/^- 当前迭代：(\d+)，/m)?.[1];
  const plannedIteration = currentState.match(/^- 下一迭代：(\d+)，/m)?.[1];
  if (currentIteration) {
    expectEqual(currentIteration, nextIteration, "当前迭代连续性");
    expectIncludes(roadmap, `| 迭代 ${currentIteration}：`, "路线图当前迭代");
  } else if (plannedIteration) {
    expectEqual(plannedIteration, nextIteration, "下一迭代连续性");
    expectIncludes(roadmap, `| 迭代 ${plannedIteration}：`, "路线图下一迭代");
    expectIncludes(currentState, "（尚未启动）", "当前状态下一迭代状态");
  } else {
    expectIncludes(currentState, "- 下一迭代：尚未定义", "当前状态下一迭代");
    expectIncludes(roadmap, `当前尚未定义迭代${nextIteration}`, "路线图下一迭代");
  }
}

const phase = `${version} · 开发初期`;
expectIncludes(coreSummary, `phase: "${phase}"`, "Rust系统摘要阶段");
expectIncludes(desktopApi, `phase: "${phase}"`, "浏览器预览系统摘要阶段");
expectIncludes(readme, `HAL100 当前版本为 \`${version}\` 开发初期版本`, "README当前版本");
expectIncludes(readme, "[当前开发状态](docs/CURRENT_STATE.md)", "README当前状态入口");
expectIncludes(roadmap, "[当前开发状态](CURRENT_STATE.md)", "路线图当前状态入口");
expectIncludes(
  currentState,
  "签名、公证、应用商店、安装包、自动更新、正式升级流程和正式发布流水线均不属于当前开发",
  "当前开发范围边界",
);

if (failures.length > 0) {
  console.error("文档一致性检查失败：");
  for (const failure of failures) {
    console.error(`- ${failure}`);
  }
  process.exit(1);
}

const currentIteration = currentState.match(/^- 当前迭代：(\d+)，/m)?.[1];
const plannedIteration = currentState.match(/^- 下一迭代：(\d+)，/m)?.[1];
const iterationSummary = currentIteration
  ? `迭代0—${completedIteration}已完成，迭代${currentIteration}进行中`
  : plannedIteration
    ? `迭代0—${completedIteration}已完成，迭代${plannedIteration}尚未启动`
    : `迭代0—${completedIteration}已完成`;
console.log(
  `文档一致性检查通过：v${version}，schema v${schemaVersion}，Agent RPC v${rpcVersion}，意图schema v${intentSchemaVersion}，任务检查点schema v${taskCheckpointSchemaVersion}，复合图schema v${taskGraphSchemaVersion}（最多${taskGraphMaxNodes}节点/${taskGraphMaxDependencies}依赖，${compositeTaskGraphScenarioCount}类语义，恢复${compositeRecoveryScenarioCount}类语义），${toolCount}个工具，${taskKindCount}类任务，${evaluationScenarioCount}个配置场景，${piIntentScenarioCount}个Pi裁决场景，${livePiIntentRunCount}次真实Pi意图运行，${controlledRoutingScenarioCount}个受控路由场景，${taskCheckpointScenarioCount}个任务检查点场景，${successPredicateWorkflowCount}类成功谓词，${successPredicateFaultCount}个证据故障场景，${boundedClarificationScenarioCount}个有界澄清场景，${openChineseScenarioCount}个开放中文场景，${openChinesePiRunCount}次开放Pi运行，${controlledActionPathCount}条动作纵向路径，${deviceSelectionCaseCount}个设备选择边界，${repeatedStandardRunCount}次真实32K连续任务，${iterationSummary}。`,
);
