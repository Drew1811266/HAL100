import { readFileSync } from "node:fs";
import { join } from "node:path";
import {
  acceptanceAcceleratorKeys,
  acceptanceEngineKeys,
  acceptanceEnvironmentSpec,
  acceptanceTargetPlatforms,
  buildAcceptanceTargets,
  resolveAcceptanceCoordinate,
  validateAcceptanceEnvironment,
  workflowAcceleratorFromWire,
  workflowEngineFromWire,
  workflowTargetFromSupportCell,
} from "./engine-acceptance-coordinate.mjs";

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

function expectNotIncludes(content, unexpected, label) {
  if (content.includes(unexpected)) {
    failures.push(`${label}仍包含过时描述：${unexpected}`);
  }
}

const rootPackage = readJson("package.json");
const desktopPackage = readJson("apps/desktop/package.json");
const sidecarPackage = readJson("sidecars/agent-kernel/package.json");
const tauriConfig = readJson("apps/desktop/src-tauri/tauri.conf.json");
const cargo = read("Cargo.toml");
const database = read("crates/hal100-infra/src/database.rs");
const inferenceEngineSupportReport = read("crates/hal100-infra/src/engine_support_report.rs");
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
const inferenceEngineArchitectureBlueprint = read(
  "docs/INFERENCE_ENGINE_ARCHITECTURE_BLUEPRINT.md",
);
const inferenceEngineSupportPlan = read("docs/INFERENCE_ENGINE_SUPPORT_PLAN.md");
const inferenceEngineAcceptanceRunnerGuide = read("docs/INFERENCE_ENGINE_ACCEPTANCE_RUNNERS.md");
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
const inferenceEngineSupportMatrix = readJson("contracts/inference-engines/v1-support-matrix.json");
const inferenceEngineAcceptanceEvidence = readJson(
  "contracts/inference-engines/v4-acceptance-evidence.json",
);
const inferenceEngineAcceptanceEvidenceSchema = readJson(
  "contracts/inference-engines/v4-acceptance-evidence.schema.json",
);
const inferenceEngineAcceptanceRunSchema = readJson(
  "contracts/inference-engines/v4-acceptance-run.schema.json",
);
const inferenceEngineAcceptancePowerShell = read("scripts/run-engine-live-acceptance.ps1");
const inferenceEngineAcceptanceBash = read("scripts/run-engine-live-acceptance.sh");
const inferenceEngineAcceptanceEnvironmentCheck = read(
  "scripts/validate-engine-acceptance-environment.mjs",
);
const inferenceEngineSourceCheckWorkflow = read(".github/workflows/source-check.yml");
const inferenceEngineLiveAcceptanceWorkflow = read(".github/workflows/live-engine-acceptance.yml");
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
const expectedInferenceEngines = [
  "llamaCpp",
  "ollama",
  "mlxLm",
  "vllm",
  "sglang",
  "tensorRtLlm",
  "openVino",
  "mlcLlm",
  "lmDeploy",
];
const expectedAcceptanceEngines = acceptanceEngineKeys;
const expectedOpenVinoAdapterVariants = [
  "ovms-openai-cpu",
  "ovms-openai-intel-gpu",
  "ovms-openai-intel-npu",
];
const expectedMlcLlmAdapterVariants = [
  "official-openai-metal",
  "official-openai-vulkan",
  "official-openai-cuda",
  "official-openai-rocm",
];
const externalInferenceEngineEntries = inferenceEngineSupportMatrix.engines.filter(
  (entry) => entry.engine !== "llamaCpp",
);
const externalInferenceEngineCount = new Set(
  externalInferenceEngineEntries.map((entry) => entry.engine),
).size;
const externalInferenceEngineSupportCellCount = externalInferenceEngineEntries.reduce(
  (total, entry) => total + entry.supportUnits.length,
  0,
);
const totalInferenceEngineSupportCellCount = inferenceEngineSupportMatrix.engines.reduce(
  (total, entry) => total + entry.supportUnits.length,
  0,
);
const formalInferenceEngineSupportCellCount = inferenceEngineSupportMatrix.engines.reduce(
  (total, entry) =>
    total +
    entry.supportUnits.filter((unit) => ["managed", "verifiedExternal"].includes(unit.status))
      .length,
  0,
);
const pendingInferenceEngineSupportCellCount =
  totalInferenceEngineSupportCellCount - formalInferenceEngineSupportCellCount;
const inferenceEngineMatrixSummary =
  `${externalInferenceEngineCount}个外部引擎、${externalInferenceEngineEntries.length}个外部适配器变体、` +
  `${externalInferenceEngineSupportCellCount}个外部支持格；合并托管llama.cpp后为` +
  `${inferenceEngineSupportMatrix.engines.length}个适配器、${totalInferenceEngineSupportCellCount}个支持格，` +
  `其中${formalInferenceEngineSupportCellCount}个正式、${pendingInferenceEngineSupportCellCount}个待验收`;
expectEqual(String(inferenceEngineSupportMatrix.schemaVersion), "1", "推理引擎支持矩阵schema版本");
expectEqual(
  String(inferenceEngineAcceptanceEvidence.schemaVersion),
  "4",
  "推理引擎验收证据账本schema版本",
);
expectEqual(
  String(Array.isArray(inferenceEngineAcceptanceEvidence.records)),
  "true",
  "推理引擎验收证据账本记录形状",
);
expectEqual(
  String(inferenceEngineAcceptanceEvidenceSchema.properties.schemaVersion.const),
  "4",
  "推理引擎验收证据账本JSON Schema版本",
);
const acceptanceLedgerRecordSchema = inferenceEngineAcceptanceEvidenceSchema.$defs?.record;
const formalLedgerGate = acceptanceLedgerRecordSchema?.allOf?.find((entry) =>
  entry.if?.properties?.status?.enum?.includes("verifiedExternal"),
);
expectEqual(
  String(formalLedgerGate?.then?.required?.includes("resilience")),
  "true",
  "推理引擎验收账本正式韧性字段门禁",
);
expectEqual(
  String(formalLedgerGate?.then?.required?.includes("hostAttestation")),
  "true",
  "推理引擎验收账本正式宿主证据门禁",
);
expectEqual(
  String(
    inferenceEngineAcceptanceEvidence.records.every(
      (record) =>
        record.hostAttestation?.platform === record.platform &&
        record.hostAttestation?.architecture === record.architecture &&
        record.hostAttestation?.accelerator === record.accelerator,
    ),
  ),
  "true",
  "推理引擎验收账本宿主证据支持格绑定",
);
expectEqual(
  String(formalLedgerGate?.then?.properties?.evidence?.minItems),
  "7",
  "推理引擎验收账本正式证据数量门禁",
);
expectEqual(
  String(inferenceEngineAcceptanceRunSchema.properties.schemaVersion.const),
  "4",
  "推理引擎验收运行产物schema版本",
);
expectEqual(
  String(inferenceEngineAcceptanceRunSchema.required.includes("hostAttestation")),
  "true",
  "推理引擎验收运行产物原生宿主证据门禁",
);
expectEqual(
  String(inferenceEngineAcceptanceRunSchema.required.includes("hostSummary")),
  "true",
  "推理引擎验收运行产物规范宿主摘要门禁",
);
expectEqual(
  String(inferenceEngineAcceptanceRunSchema.required.includes("modelEvidence")),
  "true",
  "推理引擎验收运行产物类型化模型证据门禁",
);
expectEqual(
  String(
    inferenceEngineAcceptanceRunSchema.properties.modelEvidence.required.includes(
      "valueFingerprint",
    ),
  ),
  "true",
  "推理引擎验收运行产物模型指纹门禁",
);
expectEqual(
  inferenceEngineAcceptanceRunSchema.properties.hostAttestation.properties.kind.const,
  "nativeHostProbeV1",
  "推理引擎验收运行产物宿主证据类型",
);
expectEqual(
  inferenceEngineAcceptanceRunSchema.properties.stability.properties.workloadRevision.const,
  "openai-short-chat-v1",
  "推理引擎验收性能工作负载修订",
);
for (const field of [
  "p95LatencyMs",
  "maxLatencyMs",
  "totalPromptTokens",
  "totalCompletionTokens",
  "wallTimeMs",
]) {
  expectEqual(
    String(inferenceEngineAcceptanceRunSchema.properties.stability.required.includes(field)),
    "true",
    "推理引擎验收性能字段门禁",
  );
}
expectEqual(
  String(inferenceEngineAcceptanceEvidence.records.filter((record) => record.stability).length),
  "0",
  "历史正式记录不得伪造性能测量",
);
expectEqual(
  String(inferenceEngineAcceptanceEvidence.records.filter((record) => record.modelEvidence).length),
  "0",
  "历史正式记录不得伪造类型化模型证据",
);
expectEqual(
  String(inferenceEngineAcceptanceEvidence.records.length),
  "3",
  "推理引擎历史正式记录数量",
);
expectEqual(
  capture(
    inferenceEngineSupportReport,
    /INFERENCE_ENGINE_SUPPORT_REPORT_SCHEMA_VERSION: u16 = (\d+);/,
    "推理引擎支持报告schema版本",
  ),
  "3",
  "推理引擎支持报告schema版本",
);
expectEqual(
  JSON.stringify([...new Set(inferenceEngineSupportMatrix.engines.map((engine) => engine.engine))]),
  JSON.stringify(expectedInferenceEngines),
  "推理引擎支持矩阵顺序与完整性",
);
expectEqual(
  JSON.stringify(
    inferenceEngineSupportMatrix.engines
      .filter((engine) => engine.engine === "openVino")
      .map((engine) => engine.adapterVariant),
  ),
  JSON.stringify(expectedOpenVinoAdapterVariants),
  "OpenVINO单设备适配器变体完整性",
);
expectEqual(
  JSON.stringify(
    inferenceEngineSupportMatrix.engines
      .filter((engine) => engine.engine === "mlcLlm")
      .map((engine) => engine.adapterVariant),
  ),
  JSON.stringify(expectedMlcLlmAdapterVariants),
  "MLC LLM单设备适配器变体完整性",
);
expectEqual(
  String(
    new Set(
      inferenceEngineSupportMatrix.engines.map(
        (engine) => `${engine.engine}/${engine.adapterVariant}/${engine.contractRevision}`,
      ),
    ).size,
  ),
  String(inferenceEngineSupportMatrix.engines.length),
  "推理引擎支持矩阵适配器身份唯一性",
);
for (const engine of expectedAcceptanceEngines) {
  expectIncludes(inferenceEngineAcceptancePowerShell, `'${engine}'`, "Windows推理引擎验收脚本入口");
  expectIncludes(inferenceEngineAcceptanceBash, engine, "macOS/Linux推理引擎验收脚本入口");
  expectIncludes(
    inferenceEngineLiveAcceptanceWorkflow,
    `          - ${engine}`,
    "手动真机验收工作流引擎入口",
  );
}
for (const sourceRunner of ["macos-14", "ubuntu-24.04", "windows-latest"]) {
  expectIncludes(inferenceEngineSourceCheckWorkflow, sourceRunner, "三平台源码检查工作流runner");
}
for (const acceptanceRunner of [
  "[self-hosted, macOS, ARM64, hal100-acceptance]",
  "[self-hosted, Linux, X64, hal100-acceptance]",
  "[self-hosted, Linux, ARM64, hal100-acceptance]",
  "[self-hosted, Windows, X64, hal100-acceptance]",
]) {
  expectIncludes(
    inferenceEngineLiveAcceptanceWorkflow,
    acceptanceRunner,
    "隔离自托管真机验收runner",
  );
}
for (const workflowChoice of [...acceptanceTargetPlatforms, ...acceptanceAcceleratorKeys]) {
  expectIncludes(
    inferenceEngineLiveAcceptanceWorkflow,
    `          - ${workflowChoice}`,
    "真机验收工作流支持格选择",
  );
}
let workflowMappedSupportCells = 0;
for (const entry of inferenceEngineSupportMatrix.engines) {
  if (entry.engine === "llamaCpp") {
    continue;
  }
  const workflowEngine = workflowEngineFromWire(entry.engine);
  for (const unit of entry.supportUnits) {
    const selection = {
      engine: workflowEngine,
      targetPlatform: workflowTargetFromSupportCell(unit.platform, unit.architecture),
      accelerator: workflowAcceleratorFromWire(unit.accelerator),
    };
    const resolved = resolveAcceptanceCoordinate(inferenceEngineSupportMatrix, selection);
    expectEqual(resolved.adapterVariant, entry.adapterVariant, "真机验收工作流适配器变体映射");
    expectEqual(resolved.contractRevision, entry.contractRevision, "真机验收工作流合同修订映射");
    expectEqual(resolved.manifestStatus, unit.status, "真机验收工作流支持状态映射");
    workflowMappedSupportCells += 1;
  }
}
expectEqual(String(workflowMappedSupportCells), "28", "真机验收工作流外部支持格覆盖");
for (const [document, label] of [
  [currentState, "当前状态"],
  [inferenceEngineSupportPlan, "推理引擎正式支持计划"],
]) {
  expectIncludes(document, inferenceEngineMatrixSummary, `${label}推理引擎矩阵摘要`);
}
const pendingAcceptanceTargets = buildAcceptanceTargets(inferenceEngineSupportMatrix);
const allAcceptanceTargets = buildAcceptanceTargets(inferenceEngineSupportMatrix, {
  includeFormal: true,
});
expectEqual(String(pendingAcceptanceTargets.totalTargets), "25", "待验收runner目标数量");
expectEqual(String(allAcceptanceTargets.totalTargets), "28", "全部外部runner目标数量");
expectEqual(
  String(new Set(allAcceptanceTargets.targets.map((target) => target.environmentName)).size),
  "28",
  "真机验收Environment名称唯一性",
);
for (const engine of acceptanceEngineKeys) {
  const spec = acceptanceEnvironmentSpec(engine);
  const environment = {
    [spec.apiRoot]: "http://127.0.0.1:18080/v1/",
    [spec.modelId]: "acceptance-fixture-model",
  };
  if (spec.engineVersion) environment[spec.engineVersion] = "1.2.3-fixture";
  if (spec.accelerator) environment[spec.accelerator] = spec.allowedAccelerators[0];
  const report = validateAcceptanceEnvironment(engine, environment);
  expectEqual(report.engine, engine, "真机验收环境预检引擎绑定");
  expectEqual(String(report.loopbackConfigurationValidated), "true", "真机验收环境预检回环绑定");
  expectEqual(
    String(report.requiredVariables.every((name) => !report.optionalVariables.includes(name))),
    "true",
    "真机验收环境预检必需/可选字段隔离",
  );
}
const sensitivePreflightValue = "must-not-appear-in-preflight-errors";
try {
  validateAcceptanceEnvironment("vllm", {
    HAL100_VLLM_API_ROOT: `https://example.com/${sensitivePreflightValue}`,
    HAL100_VLLM_MODEL_ID: sensitivePreflightValue,
    HAL100_VLLM_EXPECTED_VERSION: sensitivePreflightValue,
  });
  failures.push("真机验收环境预检未拒绝远端API root");
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  expectNotIncludes(message, sensitivePreflightValue, "真机验收环境预检错误脱敏");
}
try {
  validateAcceptanceEnvironment("vllm", {
    HAL100_VLLM_API_ROOT: "http://127.0.0.1:18080/v1/",
    HAL100_VLLM_MODEL_ID: "acceptance-fixture-model",
    HAL100_VLLM_EXPECTED_VERSION: "1.2.3-fixture",
    HAL100_VLLM_API_KEY: `${sensitivePreflightValue}\ninvalid`,
  });
  failures.push("真机验收环境预检未拒绝含控制字符的可选密钥");
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  expectNotIncludes(message, sensitivePreflightValue, "真机验收可选密钥错误脱敏");
}
for (const requiredGuideText of [
  "node scripts/list-engine-acceptance-targets.mjs",
  "HAL100_ACCEPTANCE_API_ROOT",
  "HAL100_ACCEPTANCE_MODEL_ID",
  "HAL100_ACCEPTANCE_ENGINE_VERSION",
  "HAL100_VLLM_API_KEY",
  "标准v4账本",
]) {
  expectIncludes(inferenceEngineAcceptanceRunnerGuide, requiredGuideText, "真机验收runner手册");
}
for (const [document, label] of [
  [currentState, "当前状态"],
  [roadmap, "路线图"],
  [inferenceEngineArchitectureBlueprint, "推理引擎架构蓝图"],
]) {
  expectIncludes(document, "v3", `${label}验收性能合同`);
}
expectIncludes(currentState, "0条已审查性能档案、3个正式外部格缺性能档案", "当前性能档案缺口");
try {
  resolveAcceptanceCoordinate(inferenceEngineSupportMatrix, {
    engine: "vllm",
    targetPlatform: "windows-x64",
    accelerator: "cuda",
  });
  failures.push("真机验收工作流未拒绝矩阵外坐标");
} catch {
  // Expected: vLLM has no declared Windows support cell.
}
expectIncludes(inferenceEngineSourceCheckWorkflow, "run: pnpm check", "三平台源码全量门禁");
expectEqual(
  String(inferenceEngineLiveAcceptanceWorkflow.match(/needs: validate-coordinate/g)?.length ?? 0),
  "4",
  "真机runner依赖无秘密支持格预检",
);
expectIncludes(inferenceEngineLiveAcceptanceWorkflow, "workflow_dispatch:", "手动真机验收触发器");
expectIncludes(
  inferenceEngineLiveAcceptanceWorkflow,
  "scripts/run-engine-live-acceptance.sh",
  "Unix真机验收工作流入口",
);
expectIncludes(
  inferenceEngineLiveAcceptanceWorkflow,
  "scripts/run-engine-live-acceptance.ps1",
  "Windows真机验收工作流入口",
);
for (const [source, label] of [
  [inferenceEngineAcceptanceBash, "Unix真机验收wrapper"],
  [inferenceEngineAcceptancePowerShell, "Windows真机验收wrapper"],
  [inferenceEngineLiveAcceptanceWorkflow, "真机验收workflow"],
]) {
  expectIncludes(source, "validate-engine-acceptance-environment.mjs", `${label}精确环境预检`);
}
expectIncludes(
  inferenceEngineAcceptanceEnvironmentCheck,
  "validateAcceptanceEnvironment",
  "真机验收环境预检CLI",
);
expectIncludes(
  inferenceEngineLiveAcceptanceWorkflow,
  "if-no-files-found: error",
  "真机验收产物缺失故障关闭",
);
expectIncludes(
  inferenceEngineLiveAcceptanceWorkflow,
  `name: hal100-acceptance-\${{ inputs.target_platform }}-\${{ inputs.engine }}-\${{ inputs.accelerator }}`,
  "真机验收受保护环境绑定",
);
for (const protectedValue of [
  "secrets.HAL100_ACCEPTANCE_API_ROOT",
  "secrets.HAL100_ACCEPTANCE_MODEL_ID",
  "secrets.HAL100_ACCEPTANCE_ENGINE_VERSION",
]) {
  expectIncludes(inferenceEngineLiveAcceptanceWorkflow, protectedValue, "真机验收受保护配置");
}
for (const publicTargetField of [
  "      api_root:\n",
  "      model_id:\n",
  "      engine_version:\n",
]) {
  expectNotIncludes(
    inferenceEngineLiveAcceptanceWorkflow,
    publicTargetField,
    "真机验收公开目标字段",
  );
}
const formalSupportChecklist = capture(
  inferenceEngineArchitectureBlueprint,
  /## 20\. 架构验收清单([\s\S]*)$/,
  "推理引擎架构验收清单",
);
expectEqual(
  String(formalSupportChecklist?.match(/^- \[x\] /gm)?.length ?? 0),
  "11",
  "推理引擎共同架构已完成门槛数量",
);
expectEqual(
  String(formalSupportChecklist?.match(/^- \[ \] /gm)?.length ?? 0),
  "1",
  "推理引擎仍待真机门槛数量",
);
expectNotIncludes(
  inferenceEngineLiveAcceptanceWorkflow,
  "hal100-engine-acceptance-import",
  "真机验收工作流不得自动导入账本",
);
expectNotIncludes(
  inferenceEngineLiveAcceptanceWorkflow,
  "\n  push:",
  "真机验收工作流不得自动push触发",
);
expectNotIncludes(
  inferenceEngineLiveAcceptanceWorkflow,
  "\n  pull_request:",
  "真机验收工作流不得自动PR触发",
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
expectIncludes(
  readme,
  "Windows 10/11 | 源码构建与宿主能力探针基线已建立；完整桌面纵向和具体引擎支持仍需逐格验收",
  "README Windows平台状态",
);
expectIncludes(
  readme,
  "Linux | 源码构建与宿主能力探针基线已建立；完整桌面纵向和具体引擎支持仍需逐格验收",
  "README Linux平台状态",
);
expectNotIncludes(
  readme,
  "Windows 10/11 | 未来目标平台；架构边界已建立，尚未开始实现",
  "README Windows平台状态",
);
expectNotIncludes(
  readme,
  "Linux | 未来目标平台；架构边界已建立，尚未开始实现",
  "README Linux平台状态",
);
expectNotIncludes(readme, "Windows 版本尚未开发。", "README平台限制");
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
  `文档一致性检查通过：v${version}，schema v${schemaVersion}，Agent RPC v${rpcVersion}，意图schema v${intentSchemaVersion}，任务检查点schema v${taskCheckpointSchemaVersion}，复合图schema v${taskGraphSchemaVersion}（最多${taskGraphMaxNodes}节点/${taskGraphMaxDependencies}依赖，${compositeTaskGraphScenarioCount}类语义，恢复${compositeRecoveryScenarioCount}类语义），${toolCount}个工具，${taskKindCount}类任务，${evaluationScenarioCount}个配置场景，${piIntentScenarioCount}个Pi裁决场景，${livePiIntentRunCount}次真实Pi意图运行，${controlledRoutingScenarioCount}个受控路由场景，${taskCheckpointScenarioCount}个任务检查点场景，${successPredicateWorkflowCount}类成功谓词，${successPredicateFaultCount}个证据故障场景，${boundedClarificationScenarioCount}个有界澄清场景，${openChineseScenarioCount}个开放中文场景，${openChinesePiRunCount}次开放Pi运行，${controlledActionPathCount}条动作纵向路径，${deviceSelectionCaseCount}个设备选择边界，${repeatedStandardRunCount}次真实32K连续任务，推理引擎${inferenceEngineMatrixSummary}，${iterationSummary}。`,
);
