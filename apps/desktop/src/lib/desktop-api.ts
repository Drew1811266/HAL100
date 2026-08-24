import { invoke } from "@tauri-apps/api/core";

export interface PlatformSummary {
  os: string;
  architecture: string;
  supported: boolean;
}

export interface AppOverview {
  appName: string;
  version: string;
  phase: string;
  gatewayState: "未启动" | "运行中" | "异常";
  databaseState: "未连接" | "已就绪" | "异常";
  platform: PlatformSummary;
}

export type DownloadSource = "huggingFace" | "modelScope";

export interface HardwareRecommendation {
  summary: string;
  parameterRange: string;
  quantization: string;
  conservativeModelBytes: number;
  notes: string[];
}

export interface HardwareProfile {
  chip: string;
  modelIdentifier: string;
  totalUnifiedMemoryBytes: number;
  physicalCpuCores: number;
  logicalCpuCores: number;
  modelStoragePath: string;
  modelStorageAvailableBytes: number;
  recommendation: HardwareRecommendation;
}

export type ModelSource = "huggingFace" | "modelScope" | "localFile";
export type ModelOwnership = "managed" | "external";
export type LocalModelState = "ready" | "missing" | "changed" | "verificationFailed";

export interface LocalModelSummary {
  id: string;
  displayName: string;
  format: string;
  quantization: string | null;
  source: ModelSource;
  repository: string | null;
  revision: string | null;
  fileName: string;
  ownership: ModelOwnership;
  license: string | null;
  state: LocalModelState;
  path: string;
  sizeBytes: number;
}

export interface ModelLibrary {
  defaultDownloadSource: DownloadSource | null;
  modelStoragePath: string;
  models: LocalModelSummary[];
}

export interface RemoteModelSearchItem {
  source: DownloadSource;
  repository: string;
  displayName: string;
  license: string | null;
  downloads: number;
  likes: number;
  parameterCount: number | null;
  repositorySizeBytes: number | null;
  gated: boolean;
  private: boolean;
}

export interface RemoteModelSearchResults {
  source: DownloadSource;
  query: string;
  items: RemoteModelSearchItem[];
}

export interface RemoteGgufFile {
  path: string;
  sizeBytes: number;
  sha256: string | null;
  revision: string;
  quantization: string | null;
}

export interface RemoteModelRepository {
  source: DownloadSource;
  repository: string;
  displayName: string;
  license: string | null;
  gated: boolean;
  private: boolean;
  files: RemoteGgufFile[];
}

export type ModelDownloadState =
  | "pending"
  | "downloading"
  | "paused"
  | "verifying"
  | "installing"
  | "ready"
  | "failed"
  | "cancelled";

export interface ModelDownloadPlan {
  planId: string;
  expiresAtMs: number;
  source: DownloadSource;
  repository: string;
  displayName: string;
  license: string | null;
  file: RemoteGgufFile;
  availableStorageBytes: number;
  requiredStorageBytes: number;
  actionSummary: string;
  requiresConfirmation: boolean;
}

export interface ModelDownloadSnapshot {
  downloadId: string;
  source: DownloadSource;
  repository: string;
  fileName: string;
  state: ModelDownloadState;
  downloadedBytes: number;
  expectedSizeBytes: number;
  errorCode: string | null;
  canResume: boolean;
  model: LocalModelSummary | null;
}

export type EngineInstallState = "notInstalled" | "installed" | "verificationFailed";
export type EngineRuntimeState = "stopped" | "starting" | "running" | "error";

export interface LlamaCppStatus {
  version: string;
  installState: EngineInstallState;
  runtimeState: EngineRuntimeState;
  activeModelId: string | null;
  activeModelName: string | null;
  port: number | null;
  lastErrorCode: string | null;
}

export interface GatewayModelRoute {
  alias: string;
  backendId: string;
  resolvedModel: string;
}

export interface GatewayRoutingSnapshot {
  activeBackendId: string | null;
  backendIds: string[];
  modelRoutes: GatewayModelRoute[];
  backendHealth: Array<{
    backendId: string;
    consecutiveFailures: number;
    circuitOpen: boolean;
  }>;
}

export type BackendKind =
  | "managedLlamaCpp"
  | "externalOpenAi"
  | "externalAnthropic"
  | "externalOllama"
  | "externalVllm"
  | "externalLlamaCpp";

export type BackendAuthMethod = "none" | "bearer" | "anthropicApiKey";

export interface BackendSummary {
  id: string;
  displayName: string;
  kind: BackendKind;
  apiRoot: string;
  authMethod: BackendAuthMethod;
  credentialConfigured: boolean;
  enabled: boolean;
  runtimeAvailable: boolean;
  isActive: boolean;
  consecutiveFailures: number;
  circuitOpen: boolean;
}

export interface BackendRouteSummary {
  alias: string;
  backendId: string;
  resolvedModel: string;
  runtimeAvailable: boolean;
}

export interface BackendCatalog {
  activeBackendId: string | null;
  backends: BackendSummary[];
  modelRoutes: BackendRouteSummary[];
}

export interface BackendDraft {
  id: string | null;
  displayName: string;
  kind: BackendKind;
  apiRoot: string;
  authMethod: BackendAuthMethod;
  apiKey: string | null;
}

export interface BackendRouteDraft {
  alias: string;
  backendId: string;
  resolvedModel: string;
}

export interface LocalBackendCandidate {
  kind: BackendKind;
  displayName: string;
  apiRoot: string;
  evidence: string;
  version: string | null;
}

export interface LocalBackendDiscovery {
  candidates: LocalBackendCandidate[];
  checkedTargets: number;
}

export type BackendProbeStatus =
  | "healthy"
  | "authenticationFailed"
  | "upstreamError"
  | "invalidResponse"
  | "unreachable";

export interface BackendProbeResult {
  backendId: string;
  status: BackendProbeStatus;
  httpStatus: number | null;
  latencyMs: number;
  modelCount: number | null;
}

export interface EngineInstallPlan {
  planId: string;
  expiresAtMs: number;
  engine: string;
  version: string;
  archiveSizeBytes: number;
  publisher: string;
  actionSummary: string;
  requiresConfirmation: boolean;
}

export interface EngineRemovePlan {
  planId: string;
  expiresAtMs: number;
  engine: string;
  version: string;
  installPath: string;
  actionSummary: string;
  requiresConfirmation: boolean;
}

export interface GgufImportPlan {
  planId: string;
  expiresAtMs: number;
  sourcePath: string;
  displayName: string;
  fileName: string;
  sizeBytes: number;
  ggufVersion: number;
  tensorCount: number;
  metadataCount: number;
  quantization: string | null;
  ownership: "external";
  actionSummary: string;
  requiresConfirmation: boolean;
}

export interface GgufImportResult {
  imported: boolean;
  model: LocalModelSummary;
}

export type ModelRemovalKind =
  | "moveManagedFileToTrash"
  | "removeMissingManagedIndex"
  | "removeExternalIndex";

export interface ModelRemovalPlan {
  planId: string;
  expiresAtMs: number;
  modelId: string;
  displayName: string;
  ownership: ModelOwnership;
  sizeBytes: number;
  removalKind: ModelRemovalKind;
  actionSummary: string;
  sourceFilePreserved: boolean;
  requiresConfirmation: boolean;
}

export interface ModelRemovalResult {
  removed: boolean;
  modelId: string;
  displayName: string;
  ownership: ModelOwnership;
  removalKind: ModelRemovalKind;
  sourceFilePreserved: boolean;
}

export type ExternalAgentIntegrationAvailability = "available" | "planned";
export type ExternalAgentGatewayProtocol =
  | "openAiChatCompletions"
  | "openAiResponses"
  | "anthropicMessages";

export interface BuiltInAgentRuntimeSummary {
  runtimeId: string;
  clientAppId: string;
  displayName: string;
  engineName: string;
  isolationSummary: string;
}

export interface ExternalAgentIntegrationSummary {
  integrationId: string;
  clientAppId: string;
  displayName: string;
  availability: ExternalAgentIntegrationAvailability;
  supportedProtocols: ExternalAgentGatewayProtocol[];
  verifiedProtocols: ExternalAgentGatewayProtocol[];
  preservesDefaultModel: boolean;
  usesIsolatedCredential: boolean;
}

export interface AgentEcosystemCatalog {
  builtInRuntime: BuiltInAgentRuntimeSummary;
  integrations: ExternalAgentIntegrationSummary[];
}

export type ExternalAgentIntegrationState =
  | "notInstalled"
  | "installedNotConfigured"
  | "configured"
  | "needsRefresh"
  | "conflict"
  | "modifiedOutsideHal100"
  | "unsupportedVersion"
  | "blocked";

export interface ExternalAgentDetection {
  integrationId: string;
  displayName: string;
  installed: boolean;
  version: string | null;
  binaryPath: string | null;
  configPath: string;
  configExists: boolean;
  integrationState: ExternalAgentIntegrationState;
  configuredProtocol: ExternalAgentGatewayProtocol | null;
  modelProfileRevision: string;
  warnings: string[];
}

export interface ExternalAgentConfigurationChange {
  path: string;
  value: string;
}

export interface ExternalAgentConfigurationPlan {
  planId: string;
  integrationId: string;
  expiresAtMs: number;
  configPath: string;
  credentialPath: string;
  changes: ExternalAgentConfigurationChange[];
  gatewayProtocol: ExternalAgentGatewayProtocol;
  createsBackup: boolean;
  preservesDefaultModel: boolean;
  requiresConfirmation: boolean;
  modelProfileRevision: string;
  warnings: string[];
}

export interface ExternalAgentConfigurationResult {
  configured: boolean;
  integrationId: string;
  configPath: string;
  backupPath: string | null;
  credentialPrefix: string;
  modelProfileRevision: string;
}

export type ExternalAgentManagedChangeAction = "removeManagedFragment" | "removeManagedCredential";

export interface ExternalAgentManagedChange {
  path: string;
  action: ExternalAgentManagedChangeAction;
}

export interface ExternalAgentDisconnectPlan {
  planId: string;
  integrationId: string;
  expiresAtMs: number;
  configPath: string;
  credentialPath: string;
  changes: ExternalAgentManagedChange[];
  createsBackup: boolean;
  revokesCredential: boolean;
  requiresConfirmation: boolean;
}

export interface ExternalAgentDisconnectResult {
  disconnected: boolean;
  integrationId: string;
  configPath: string;
  backupPath: string | null;
  credentialRevoked: boolean;
}

export type OpenCodeIntegrationState =
  | "notConfigured"
  | "configured"
  | "conflict"
  | "modifiedOutsideHal100";

export interface OpenCodeDetection {
  installed: boolean;
  version: string | null;
  binaryPath: string | null;
  configPath: string;
  configExists: boolean;
  configFormat: "json" | "jsonc";
  integrationState: OpenCodeIntegrationState;
  warnings: string[];
}

export interface OpenCodeConfigChange {
  path: string;
  value: string;
}

export interface OpenCodeConfigPlan {
  planId: string;
  expiresAtMs: number;
  configPath: string;
  credentialPath: string;
  changes: OpenCodeConfigChange[];
  createsBackup: boolean;
  preservesDefaultModel: boolean;
  requiresConfirmation: boolean;
}

export interface OpenCodeApplyResult {
  configured: boolean;
  configPath: string;
  backupPath: string | null;
  credentialPrefix: string;
}

export interface UsageTotals {
  requestCount: number;
  inputTokens: number;
  cachedTokens: number;
  outputTokens: number;
  totalTokens: number;
}

export interface UsageRequestSummary {
  requestId: string;
  clientAppId: string;
  clientDisplayName: string;
  requestedModel: string;
  resolvedModel: string;
  backendId: string;
  startedAtMs: number;
  completedAtMs: number;
  inputTokens: number | null;
  cachedTokens: number | null;
  outputTokens: number | null;
  totalTokens: number | null;
  status: string;
  usageAccuracy: string;
}

export interface UsageDashboard {
  totals: UsageTotals;
  recentRequests: UsageRequestSummary[];
}

export interface DesktopSettings {
  onboardingCompleted: boolean;
  onboardingStep: number;
  launchAtLoginAsked: boolean;
  launchAtLoginEnabled: boolean;
  usageRetentionDays: number | null;
  auditRetentionDays: number | null;
  gatewayBaseUrl: string;
  closeBehavior: string;
}

export interface OnboardingCompletion {
  launchAtLogin: boolean;
}

export interface RetentionSettingsDraft {
  usageRetentionDays: number | null;
  auditRetentionDays: number | null;
}

export interface AuditDetail {
  key: string;
  value: string;
}

export interface AuditEventSummary {
  id: string;
  eventType: string;
  targetType: string;
  targetId: string;
  details: AuditDetail[];
  createdAtMs: number;
}

export interface AuditLog {
  totalCount: number;
  events: AuditEventSummary[];
}

export interface DataCleanupPreview {
  usageRequestCount: number;
  auditEventCount: number;
  usageRetentionDays: number | null;
  auditRetentionDays: number | null;
}

export interface DataCleanupResult {
  usageRequestsDeleted: number;
  auditEventsDeleted: number;
}

export interface GenericClientSummary {
  clientAppId: string;
  displayName: string;
  displayPrefix: string;
  createdAtMs: number;
}

export interface GenericClientCatalog {
  gatewayBaseUrl: string;
  clients: GenericClientSummary[];
}

export interface GenericClientCredential {
  client: GenericClientSummary;
  apiKey: string;
}

export interface ModelTestResult {
  content: string;
  model: string;
  inputTokens: number | null;
  cachedTokens: number | null;
  outputTokens: number | null;
  totalTokens: number | null;
  elapsedMs: number;
  requestId: string | null;
}

export type AgentComponentState = "unavailable" | "stopped" | "starting" | "running" | "error";

export interface AgentStatus {
  kernelState: AgentComponentState;
  modelRuntimeState: AgentComponentState;
  piVersion: string;
  modelName: string;
  modelPrepared: boolean;
  modelSizeBytes: number;
  idleTimeoutSeconds: number;
  activeRunId: string | null;
  cancellationRequested: boolean;
  lastErrorCode: string | null;
}

export type EnvironmentHealthStatus = "healthy" | "needsAttention" | "error";
export type DiagnosticSeverity = "info" | "warning" | "error";
export type DiagnosticComponent =
  | "gateway"
  | "inferenceEngine"
  | "modelLibrary"
  | "openCode"
  | "piCodingAgent"
  | "openClaw"
  | "hermesAgent";
export type DiagnosticRepairKind =
  | "installLlamaCpp"
  | "configureExternalAgent"
  | "removeModelIndex";

export interface EnvironmentDiagnosticFinding {
  findingId: string;
  code: string;
  component: DiagnosticComponent;
  severity: DiagnosticSeverity;
  title: string;
  summary: string;
  targetId: string | null;
  repairKind: DiagnosticRepairKind | null;
  repairSummary: string | null;
}

export interface EnvironmentDiagnosticReport {
  reportId: string;
  generatedAtMs: number;
  status: EnvironmentHealthStatus;
  engineInstallState: EngineInstallState;
  engineRuntimeState: EngineRuntimeState;
  readyModelCount: number;
  unhealthyModelCount: number;
  configuredBackendCount: number;
  openCodeInstalled: boolean;
  openCodeIntegrationState: OpenCodeIntegrationState;
  installedExternalAgentCount: number;
  configuredExternalAgentCount: number;
  attentionExternalAgentCount: number;
  warningCount: number;
  errorCount: number;
  omittedFindingCount: number;
  findings: EnvironmentDiagnosticFinding[];
}

export interface AgentPromptRequest {
  prompt: string;
  cloudTarget: AgentCloudTarget | null;
}

export type AgentProviderProtocol = "localOpenAi" | "cloudOpenAi" | "cloudAnthropic";

export interface AgentCloudTarget {
  backendId: string;
  model: string;
}

export interface AgentCloudRunPreview {
  backendId: string;
  backendName: string;
  backendKind: BackendKind;
  apiRoot: string;
  model: string;
  promptBytes: number;
  sendsSystemInstructions: boolean;
  maySendToolResults: boolean;
  sendsCredentialsToSidecar: boolean;
  sendsLocalPaths: boolean;
  confirmationSummary: string;
}

export interface AgentCloudSessionPreview {
  backendId: string;
  backendName: string;
  backendKind: BackendKind;
  apiRoot: string;
  model: string;
  sendsFuturePrompts: boolean;
  sendsSystemInstructions: boolean;
  maySendToolResults: boolean;
  storesConversationHistory: boolean;
  sendsCredentialsToSidecar: boolean;
  sendsLocalPaths: boolean;
  confirmationSummary: string;
}

export interface AgentCloudSessionStatus {
  active: boolean;
  available: boolean;
  backendId: string | null;
  backendName: string | null;
  backendKind: BackendKind | null;
  apiRoot: string | null;
  model: string | null;
  providerProtocol: AgentProviderProtocol | null;
  activatedAtMs: number | null;
  lastErrorCode: string | null;
}

export interface AgentToolEvent {
  toolCallId: string;
  toolName: string;
  label: string;
  status: string;
  summary: string;
}

export interface AgentRunResult {
  runId: string;
  answer: string;
  toolEvents: AgentToolEvent[];
  actionPlans: AgentActionPlan[];
  modelName: string;
  startedAtMs: number;
  completedAtMs: number;
}

export type AgentActionKind =
  | "startOrSwitchModel"
  | "downloadModel"
  | "removeModel"
  | "installLlamaCpp"
  | "removeLlamaCpp"
  | "installExternalAgent"
  | "removeExternalAgent"
  | "configureExternalAgent"
  | "disconnectExternalAgent";

export interface AgentActionPlan {
  planId: string;
  runId: string;
  actionKind: AgentActionKind;
  targetId: string;
  targetName: string;
  currentState: string | null;
  details: string[];
  expiresAtMs: number;
  actionSummary: string;
  requiresNativeConfirmation: boolean;
}

export interface AgentActionResult {
  planId: string;
  actionKind: AgentActionKind;
  targetId: string;
  targetName: string;
  outcomeSummary: string;
  runtimeState: EngineRuntimeState | null;
  diagnosticReport: EnvironmentDiagnosticReport | null;
}

const developmentOverview: AppOverview = {
  appName: "HAL100",
  version: "1.0.2",
  phase: "迭代 22 · 版本化受管部署配方",
  gatewayState: "运行中",
  databaseState: "已就绪",
  platform: {
    os: "macOS",
    architecture: "Apple Silicon",
    supported: true,
  },
};

const browserAgentEcosystemCatalog: AgentEcosystemCatalog = {
  builtInRuntime: {
    runtimeId: "hal100-agent-runtime",
    clientAppId: "hal100-agent",
    displayName: "HAL100 Agent",
    engineName: "Pi Agent Core",
    isolationSummary: "HAL100 私有的按任务进程、临时 HOME、会话和凭据",
  },
  integrations: [
    {
      integrationId: "opencode",
      clientAppId: "opencode",
      displayName: "OpenCode",
      availability: "available",
      supportedProtocols: ["openAiChatCompletions"],
      verifiedProtocols: ["openAiChatCompletions"],
      preservesDefaultModel: true,
      usesIsolatedCredential: true,
    },
    {
      integrationId: "pi-coding-agent",
      clientAppId: "pi-coding-agent",
      displayName: "Pi Coding Agent",
      availability: "available",
      supportedProtocols: ["openAiChatCompletions"],
      verifiedProtocols: ["openAiChatCompletions"],
      preservesDefaultModel: true,
      usesIsolatedCredential: true,
    },
    {
      integrationId: "openclaw",
      clientAppId: "openclaw",
      displayName: "OpenClaw",
      availability: "available",
      supportedProtocols: ["openAiChatCompletions", "openAiResponses", "anthropicMessages"],
      verifiedProtocols: ["openAiChatCompletions", "openAiResponses", "anthropicMessages"],
      preservesDefaultModel: true,
      usesIsolatedCredential: true,
    },
    {
      integrationId: "hermes-agent",
      clientAppId: "hermes-agent",
      displayName: "Hermes Agent",
      availability: "available",
      supportedProtocols: ["openAiChatCompletions"],
      verifiedProtocols: ["openAiChatCompletions"],
      preservesDefaultModel: true,
      usesIsolatedCredential: true,
    },
  ],
};

const browserOpenCodeDetection: OpenCodeDetection = {
  installed: false,
  version: null,
  binaryPath: null,
  configPath: "~/.config/opencode/opencode.json",
  configExists: false,
  configFormat: "json",
  integrationState: "notConfigured",
  warnings: ["当前是浏览器预览模式，不会读取或修改本机配置。"],
};

const browserPiCodingAgentDetection: ExternalAgentDetection = {
  integrationId: "pi-coding-agent",
  displayName: "Pi Coding Agent",
  installed: false,
  version: null,
  binaryPath: null,
  configPath: "~/.pi/agent/models.json",
  configExists: false,
  integrationState: "notInstalled",
  configuredProtocol: null,
  modelProfileRevision: "managed-route-v1",
  warnings: ["当前是浏览器预览模式，不会读取或修改本机配置。"],
};

const browserOpenClawDetection: ExternalAgentDetection = {
  integrationId: "openclaw",
  displayName: "OpenClaw",
  installed: false,
  version: null,
  binaryPath: null,
  configPath: "~/.openclaw/openclaw.json",
  configExists: false,
  integrationState: "notInstalled",
  configuredProtocol: null,
  modelProfileRevision: "managed-route-v1",
  warnings: ["当前是浏览器预览模式，不会读取或修改本机配置。"],
};

const browserHermesAgentDetection: ExternalAgentDetection = {
  integrationId: "hermes-agent",
  displayName: "Hermes Agent",
  installed: false,
  version: null,
  binaryPath: null,
  configPath: "~/.hermes/config.yaml",
  configExists: false,
  integrationState: "notInstalled",
  configuredProtocol: null,
  modelProfileRevision: "managed-route-v1",
  warnings: ["当前是浏览器预览模式，不会读取或修改本机配置。"],
};

const browserHardwareProfile: HardwareProfile = {
  chip: "Apple M1（浏览器预览）",
  modelIdentifier: "iMac21,1",
  totalUnifiedMemoryBytes: 16 * 1024 ** 3,
  physicalCpuCores: 8,
  logicalCpuCores: 8,
  modelStoragePath: "~/Library/Application Support/HAL100/models",
  modelStorageAvailableBytes: 98.5 * 1024 ** 3,
  recommendation: {
    summary: "适合日常本地推理",
    parameterRange: "3B–8B",
    quantization: "优先 GGUF Q4_K_M；需要质量时再评估 Q5_K_M",
    conservativeModelBytes: 9 * 1024 ** 3,
    notes: [
      "建议值为保守起点，实际占用还取决于上下文长度和 KV Cache。",
      "当前是浏览器预览数据，Tauri 开发版会读取真实硬件。",
    ],
  },
};

const browserModelLibrary: ModelLibrary = {
  defaultDownloadSource: null,
  modelStoragePath: browserHardwareProfile.modelStoragePath,
  models: [],
};

const browserUsageDashboard: UsageDashboard = {
  totals: {
    requestCount: 0,
    inputTokens: 0,
    cachedTokens: 0,
    outputTokens: 0,
    totalTokens: 0,
  },
  recentRequests: [],
};

const browserUsagePreviewRequests: UsageRequestSummary[] = [
  [760, 0, 82],
  [1120, 360, 214],
  [680, 0, 154],
  [1840, 720, 338],
  [940, 0, 126],
  [1320, 480, 286],
  [820, 0, 94],
  [2140, 960, 418],
  [1080, 0, 206],
  [1540, 640, 332],
  [890, 0, 118],
  [2480, 1120, 486],
].map(([inputTokens, cachedTokens, outputTokens], index, values) => {
  const agentRequest = index % 4 === 1;
  const testRequest = index % 5 === 2;
  const startedAtMs = Date.now() - (values.length - index) * 8 * 60 * 1000;
  return {
    requestId: `browser-usage-${index + 1}`,
    clientAppId: agentRequest ? "hal100-agent" : testRequest ? "hal100-model-test" : "opencode",
    clientDisplayName: agentRequest ? "HAL100 Agent" : testRequest ? "模型测试" : "OpenCode",
    requestedModel: "hal100-active",
    resolvedModel: "Qwen3.5-2B-GGUF",
    backendId: "llama-cpp-managed",
    startedAtMs,
    completedAtMs: startedAtMs + 1_400,
    inputTokens,
    cachedTokens,
    outputTokens,
    totalTokens: inputTokens + outputTokens,
    status: "succeeded",
    usageAccuracy: "exact_backend_response",
  };
});

const browserUsagePreviewDashboard: UsageDashboard = {
  totals: browserUsagePreviewRequests.reduce<UsageTotals>(
    (totals, request) => ({
      requestCount: totals.requestCount + 1,
      inputTokens: totals.inputTokens + (request.inputTokens ?? 0),
      cachedTokens: totals.cachedTokens + (request.cachedTokens ?? 0),
      outputTokens: totals.outputTokens + (request.outputTokens ?? 0),
      totalTokens: totals.totalTokens + (request.totalTokens ?? 0),
    }),
    { requestCount: 0, inputTokens: 0, cachedTokens: 0, outputTokens: 0, totalTokens: 0 },
  ),
  recentRequests: [...browserUsagePreviewRequests].reverse(),
};

const browserDesktopSettings: DesktopSettings = {
  onboardingCompleted: true,
  onboardingStep: 5,
  launchAtLoginAsked: false,
  launchAtLoginEnabled: false,
  usageRetentionDays: 90,
  auditRetentionDays: 365,
  gatewayBaseUrl: "http://127.0.0.1:10100/v1",
  closeBehavior: "隐藏窗口并保持后台运行",
};

const browserGenericClientCatalog: GenericClientCatalog = {
  gatewayBaseUrl: browserDesktopSettings.gatewayBaseUrl,
  clients: [],
};

const browserAuditLog: AuditLog = {
  totalCount: 0,
  events: [],
};

const browserAgentStatus: AgentStatus = {
  kernelState: "stopped",
  modelRuntimeState: "stopped",
  piVersion: "0.84.2",
  modelName: "Qwen3.5-2B Q4_K_M",
  modelPrepared: true,
  modelSizeBytes: 1_280_835_840,
  idleTimeoutSeconds: 120,
  activeRunId: null,
  cancellationRequested: false,
  lastErrorCode: null,
};

const browserEnvironmentDiagnostics: EnvironmentDiagnosticReport = {
  reportId: "diagnostic-browser-preview",
  generatedAtMs: Date.now(),
  status: "healthy",
  engineInstallState: "installed",
  engineRuntimeState: "stopped",
  readyModelCount: 1,
  unhealthyModelCount: 0,
  configuredBackendCount: 1,
  openCodeInstalled: false,
  openCodeIntegrationState: "notConfigured",
  installedExternalAgentCount: 0,
  configuredExternalAgentCount: 0,
  attentionExternalAgentCount: 0,
  warningCount: 0,
  errorCount: 0,
  omittedFindingCount: 0,
  findings: [
    {
      findingId: "finding-preview-1",
      code: "external_agent_not_installed",
      component: "openCode",
      severity: "info",
      title: "尚未检测到 OpenCode",
      summary: "这不会影响 HAL100；安装 OpenCode 后可再次按需诊断接入状态。",
      targetId: "opencode",
      repairKind: null,
      repairSummary: null,
    },
  ],
};

const browserAgentCloudSession: AgentCloudSessionStatus = {
  active: false,
  available: false,
  backendId: null,
  backendName: null,
  backendKind: null,
  apiRoot: null,
  model: null,
  providerProtocol: null,
  activatedAtMs: null,
  lastErrorCode: null,
};

export function isTauriRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

export async function getAppOverview(): Promise<AppOverview> {
  if (!isTauriRuntime()) {
    return developmentOverview;
  }

  return invoke<AppOverview>("get_app_overview");
}

export async function getAgentStatus(): Promise<AgentStatus> {
  if (!isTauriRuntime()) return browserAgentStatus;
  return invoke<AgentStatus>("get_agent_status");
}

export async function getEnvironmentDiagnostics(): Promise<EnvironmentDiagnosticReport> {
  if (!isTauriRuntime()) return { ...browserEnvironmentDiagnostics, generatedAtMs: Date.now() };
  return invoke<EnvironmentDiagnosticReport>("get_environment_diagnostics");
}

export async function runAgentPrompt(request: AgentPromptRequest): Promise<AgentRunResult> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会启动本地 Agent");
  return invoke<AgentRunResult>("run_agent_prompt", { request });
}

export async function previewAgentCloudRun(
  request: AgentPromptRequest,
): Promise<AgentCloudRunPreview> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会连接云端 Agent");
  return invoke<AgentCloudRunPreview>("preview_agent_cloud_run", { request });
}

export async function getAgentCloudSession(): Promise<AgentCloudSessionStatus> {
  if (!isTauriRuntime()) return browserAgentCloudSession;
  return invoke<AgentCloudSessionStatus>("get_agent_cloud_session");
}

export async function previewAgentCloudSession(
  target: AgentCloudTarget,
): Promise<AgentCloudSessionPreview> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会启用云端 Agent 会话");
  return invoke<AgentCloudSessionPreview>("preview_agent_cloud_session", { target });
}

export async function startAgentCloudSession(
  target: AgentCloudTarget,
): Promise<AgentCloudSessionStatus> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会启用云端 Agent 会话");
  return invoke<AgentCloudSessionStatus>("start_agent_cloud_session", { target });
}

export async function stopAgentCloudSession(): Promise<AgentCloudSessionStatus> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式没有云端 Agent 会话");
  return invoke<AgentCloudSessionStatus>("stop_agent_cloud_session");
}

export async function cancelAgentRun(): Promise<AgentStatus> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式没有活动 Agent 任务");
  return invoke<AgentStatus>("cancel_agent_run");
}

export async function applyAgentActionPlan(planId: string): Promise<AgentActionResult> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会执行 Agent 操作计划");
  return invoke<AgentActionResult>("apply_agent_action_plan", { planId });
}

export async function stopAgentRuntime(): Promise<AgentStatus> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式没有运行中的 Agent 模型");
  return invoke<AgentStatus>("stop_agent_runtime");
}

export async function getDesktopSettings(): Promise<DesktopSettings> {
  if (!isTauriRuntime()) {
    const onboardingPreview =
      new URLSearchParams(window.location.search).get("preview") === "onboarding";
    const onboardingCompleted =
      !onboardingPreview &&
      window.localStorage.getItem("hal100-preview-onboarding") !== "incomplete";
    return {
      ...browserDesktopSettings,
      onboardingCompleted,
      onboardingStep: onboardingCompleted ? browserDesktopSettings.onboardingStep : 1,
    };
  }
  return invoke<DesktopSettings>("get_desktop_settings");
}

export async function setOnboardingStep(step: number): Promise<void> {
  if (!isTauriRuntime()) return;
  return invoke<void>("set_onboarding_step", { step });
}

export async function completeOnboarding(
  completion: OnboardingCompletion,
): Promise<DesktopSettings> {
  if (!isTauriRuntime()) {
    window.localStorage.removeItem("hal100-preview-onboarding");
    return {
      ...browserDesktopSettings,
      onboardingCompleted: true,
      onboardingStep: 5,
      launchAtLoginAsked: true,
      launchAtLoginEnabled: completion.launchAtLogin,
    };
  }
  return invoke<DesktopSettings>("complete_onboarding", { completion });
}

export async function setLaunchAtLogin(enabled: boolean): Promise<DesktopSettings> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会修改系统登录项");
  return invoke<DesktopSettings>("set_launch_at_login", { enabled });
}

export async function updateRetentionSettings(
  draft: RetentionSettingsDraft,
): Promise<RetentionSettingsDraft> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会保存数据保留设置");
  return invoke<RetentionSettingsDraft>("update_retention_settings", { draft });
}

export async function getDataCleanupPreview(): Promise<DataCleanupPreview> {
  if (!isTauriRuntime()) {
    return {
      usageRequestCount: 0,
      auditEventCount: 0,
      usageRetentionDays: browserDesktopSettings.usageRetentionDays,
      auditRetentionDays: browserDesktopSettings.auditRetentionDays,
    };
  }
  return invoke<DataCleanupPreview>("get_data_cleanup_preview");
}

export async function applyDataRetention(): Promise<DataCleanupResult> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会删除历史数据");
  return invoke<DataCleanupResult>("apply_data_retention");
}

export async function getAuditLog(): Promise<AuditLog> {
  if (!isTauriRuntime()) return browserAuditLog;
  return invoke<AuditLog>("get_audit_log");
}

export async function getGenericClientCatalog(): Promise<GenericClientCatalog> {
  if (!isTauriRuntime()) return browserGenericClientCatalog;
  return invoke<GenericClientCatalog>("get_generic_client_catalog");
}

export async function createGenericClient(displayName: string): Promise<GenericClientCredential> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会签发本地客户端 Key");
  return invoke<GenericClientCredential>("create_generic_client", { displayName });
}

export async function revokeGenericClient(clientAppId: string): Promise<GenericClientCatalog> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会撤销本地客户端 Key");
  return invoke<GenericClientCatalog>("revoke_generic_client", { clientAppId });
}

export async function getHardwareProfile(): Promise<HardwareProfile> {
  if (!isTauriRuntime()) {
    return browserHardwareProfile;
  }
  return invoke<HardwareProfile>("get_hardware_profile");
}

export async function getModelLibrary(): Promise<ModelLibrary> {
  if (!isTauriRuntime()) {
    return browserModelLibrary;
  }
  return invoke<ModelLibrary>("get_model_library");
}

export async function setDefaultDownloadSource(source: DownloadSource): Promise<ModelLibrary> {
  if (!isTauriRuntime()) {
    return { ...browserModelLibrary, defaultDownloadSource: source };
  }
  return invoke<ModelLibrary>("set_default_download_source", { source });
}

export async function searchRemoteModels(
  source: DownloadSource,
  query: string,
): Promise<RemoteModelSearchResults> {
  if (!isTauriRuntime()) {
    return {
      source,
      query: query.trim(),
      items: [
        {
          source,
          repository: "unsloth/Qwen3.5-2B-GGUF",
          displayName: "Qwen3.5-2B-GGUF",
          license: "apache-2.0",
          downloads: 18432,
          likes: 126,
          parameterCount: 600_000_000,
          repositorySizeBytes: 1.9 * 1024 ** 3,
          gated: false,
          private: false,
        },
      ],
    };
  }
  return invoke<RemoteModelSearchResults>("search_remote_models", { source, query });
}

export async function getRemoteModelRepository(
  source: DownloadSource,
  repository: string,
): Promise<RemoteModelRepository> {
  if (!isTauriRuntime()) {
    return {
      source,
      repository,
      displayName: "Qwen3.5-2B-GGUF",
      license: "apache-2.0",
      gated: false,
      private: false,
      files: [
        {
          path: "Qwen3.5-2B-Q4_K_M.gguf",
          sizeBytes: 1_280_835_840,
          sha256: "aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223",
          revision: "f6d5376be1edb4d416d56da11e5397a961aca8ae",
          quantization: "Q4_K_M",
        },
        {
          path: "Qwen3.5-2B-UD-Q4_K_XL.gguf",
          sizeBytes: 1_339_752_704,
          sha256: "0af96165ea615bea39a04118d63f0b6d35908aea850ee4a51aa6151d851b8b35",
          revision: "f6d5376be1edb4d416d56da11e5397a961aca8ae",
          quantization: "UD-Q4_K_XL",
        },
      ],
    };
  }
  return invoke<RemoteModelRepository>("get_remote_model_repository", { source, repository });
}

export async function planModelDownload(
  source: DownloadSource,
  repository: string,
  remotePath: string,
): Promise<ModelDownloadPlan> {
  if (!isTauriRuntime()) {
    const detail = await getRemoteModelRepository(source, repository);
    const file = detail.files.find((candidate) => candidate.path === remotePath);
    if (!file) throw new Error("所选 GGUF 文件已不存在");
    return {
      planId: "browser-download-preview",
      expiresAtMs: Date.now() + 5 * 60 * 1000,
      source,
      repository,
      displayName: detail.displayName,
      license: detail.license,
      file,
      availableStorageBytes: browserHardwareProfile.modelStorageAvailableBytes,
      requiredStorageBytes: file.sizeBytes + 512 * 1024 ** 2,
      actionSummary: "下载到 HAL100 托管目录，完成 SHA-256 与 GGUF 校验后原子安装",
      requiresConfirmation: true,
    };
  }
  return invoke<ModelDownloadPlan>("plan_model_download", { source, repository, remotePath });
}

export async function startModelDownload(planId: string): Promise<ModelDownloadSnapshot> {
  if (!isTauriRuntime()) {
    throw new Error("浏览器预览模式不会写入模型文件");
  }
  return invoke<ModelDownloadSnapshot>("start_model_download", { planId });
}

export async function getModelDownloads(): Promise<ModelDownloadSnapshot[]> {
  if (!isTauriRuntime()) return [];
  return invoke<ModelDownloadSnapshot[]>("get_model_downloads");
}

export async function resumeModelDownload(downloadId: string): Promise<ModelDownloadSnapshot> {
  if (!isTauriRuntime()) {
    throw new Error("浏览器预览模式不会恢复模型下载");
  }
  return invoke<ModelDownloadSnapshot>("resume_model_download", { downloadId });
}

export async function cancelModelDownload(downloadId: string): Promise<void> {
  if (!isTauriRuntime()) {
    throw new Error("浏览器预览模式没有运行中的下载");
  }
  return invoke<void>("cancel_model_download", { downloadId });
}

export async function planModelRemoval(modelId: string): Promise<ModelRemovalPlan> {
  if (!isTauriRuntime()) {
    const library = await getModelLibrary();
    const model = library.models.find((candidate) => candidate.id === modelId);
    if (!model) throw new Error("模型不存在或已从索引移除");
    const removalKind: ModelRemovalKind =
      model.ownership === "external"
        ? "removeExternalIndex"
        : model.state === "missing"
          ? "removeMissingManagedIndex"
          : "moveManagedFileToTrash";
    return {
      planId: "browser-model-removal-preview",
      expiresAtMs: Date.now() + 5 * 60 * 1000,
      modelId: model.id,
      displayName: model.displayName,
      ownership: model.ownership,
      sizeBytes: model.sizeBytes,
      removalKind,
      actionSummary:
        removalKind === "removeExternalIndex"
          ? "只移除 HAL100 索引，不移动或删除外部源文件"
          : removalKind === "removeMissingManagedIndex"
            ? "只清理文件已经不存在的托管模型索引"
            : "将 HAL100 托管文件移到 macOS 废纸篓并移除索引",
      sourceFilePreserved: model.ownership === "external",
      requiresConfirmation: true,
    };
  }
  return invoke<ModelRemovalPlan>("plan_model_removal", { modelId });
}

export async function applyModelRemoval(planId: string): Promise<ModelRemovalResult> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会移除模型");
  return invoke<ModelRemovalResult>("apply_model_removal", { planId });
}

const browserLlamaCppStatus: LlamaCppStatus = {
  version: "b10218",
  installState: "notInstalled",
  runtimeState: "stopped",
  activeModelId: null,
  activeModelName: null,
  port: null,
  lastErrorCode: null,
};

const browserGatewayRoutingSnapshot: GatewayRoutingSnapshot = {
  activeBackendId: null,
  backendIds: [],
  modelRoutes: [],
  backendHealth: [],
};

const browserBackendCatalog: BackendCatalog = {
  activeBackendId: null,
  backends: [],
  modelRoutes: [],
};

export async function getLlamaCppStatus(): Promise<LlamaCppStatus> {
  if (!isTauriRuntime()) return browserLlamaCppStatus;
  return invoke<LlamaCppStatus>("get_llama_cpp_status");
}

export async function getGatewayRoutingSnapshot(): Promise<GatewayRoutingSnapshot> {
  if (!isTauriRuntime()) return browserGatewayRoutingSnapshot;
  return invoke<GatewayRoutingSnapshot>("get_gateway_routing_snapshot");
}

export async function getBackendCatalog(): Promise<BackendCatalog> {
  if (!isTauriRuntime()) return browserBackendCatalog;
  return invoke<BackendCatalog>("get_backend_catalog");
}

export async function saveExternalBackend(draft: BackendDraft): Promise<BackendCatalog> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会保存后端配置");
  return invoke<BackendCatalog>("save_external_backend", { draft });
}

export async function activateExternalBackend(backendId: string): Promise<BackendCatalog> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会切换活动后端");
  return invoke<BackendCatalog>("activate_external_backend", { backendId });
}

export async function forceActivateExternalBackend(backendId: string): Promise<BackendCatalog> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会强制切换活动后端");
  return invoke<BackendCatalog>("force_activate_external_backend", { backendId });
}

export async function probeExternalBackend(backendId: string): Promise<BackendProbeResult> {
  if (!isTauriRuntime()) {
    return {
      backendId,
      status: "healthy",
      httpStatus: 200,
      latencyMs: 8,
      modelCount: 1,
    };
  }
  return invoke<BackendProbeResult>("probe_external_backend", { backendId });
}

export async function saveModelRoute(draft: BackendRouteDraft): Promise<BackendCatalog> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会保存模型别名");
  return invoke<BackendCatalog>("save_model_route", { draft });
}

export async function deleteModelRoute(alias: string): Promise<BackendCatalog> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会删除模型别名");
  return invoke<BackendCatalog>("delete_model_route", { alias });
}

export async function deleteExternalBackend(backendId: string): Promise<BackendCatalog> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会删除后端");
  return invoke<BackendCatalog>("delete_external_backend", { backendId });
}

export async function discoverLocalBackends(): Promise<LocalBackendDiscovery> {
  if (!isTauriRuntime()) {
    return {
      checkedTargets: 3,
      candidates: [
        {
          kind: "externalOllama",
          displayName: "本机 Ollama（预览示例）",
          apiRoot: "http://127.0.0.1:11434/v1/",
          evidence: "浏览器预览不会实际扫描端口",
          version: "0.12.x",
        },
      ],
    };
  }
  return invoke<LocalBackendDiscovery>("discover_local_backends");
}

export async function planLlamaCppInstall(): Promise<EngineInstallPlan> {
  if (!isTauriRuntime()) {
    return {
      planId: "browser-engine-install",
      expiresAtMs: Date.now() + 5 * 60 * 1000,
      engine: "llama.cpp",
      version: "b10218",
      archiveSizeBytes: 10_938_782,
      publisher: "ggml-org/llama.cpp GitHub Releases",
      actionSummary: "下载固定版本的 Apple Silicon 官方构建，校验 SHA-256 后安装到 HAL100 托管目录",
      requiresConfirmation: true,
    };
  }
  return invoke<EngineInstallPlan>("plan_llama_cpp_install");
}

export async function applyLlamaCppInstall(planId: string): Promise<LlamaCppStatus> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会安装推理引擎");
  return invoke<LlamaCppStatus>("apply_llama_cpp_install", { planId });
}

export async function planLlamaCppRemove(): Promise<EngineRemovePlan> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式没有已安装引擎");
  return invoke<EngineRemovePlan>("plan_llama_cpp_remove");
}

export async function applyLlamaCppRemove(planId: string): Promise<LlamaCppStatus> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会卸载推理引擎");
  return invoke<LlamaCppStatus>("apply_llama_cpp_remove", { planId });
}

export async function startLlamaCppModel(modelId: string): Promise<LlamaCppStatus> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会启动推理服务");
  return invoke<LlamaCppStatus>("start_llama_cpp_model", { modelId });
}

export async function forceStartLlamaCppModel(modelId: string): Promise<LlamaCppStatus> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会强制切换本地模型");
  return invoke<LlamaCppStatus>("force_start_llama_cpp_model", { modelId });
}

export async function stopLlamaCpp(): Promise<LlamaCppStatus> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式没有运行中的推理服务");
  return invoke<LlamaCppStatus>("stop_llama_cpp");
}

export async function forceStopLlamaCpp(): Promise<LlamaCppStatus> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会强制停止本地模型");
  return invoke<LlamaCppStatus>("force_stop_llama_cpp");
}

export async function testActiveModel(prompt: string): Promise<ModelTestResult> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会向模型发送内容");
  return invoke<ModelTestResult>("test_active_model", { prompt });
}

export async function getUsageDashboard(): Promise<UsageDashboard> {
  if (!isTauriRuntime()) {
    const preview = new URLSearchParams(window.location.search).get("preview");
    return preview === "usage" ? browserUsagePreviewDashboard : browserUsageDashboard;
  }
  return invoke<UsageDashboard>("get_usage_dashboard");
}

export async function selectAndPlanGgufImport(): Promise<GgufImportPlan | null> {
  if (!isTauriRuntime()) {
    return {
      planId: "browser-gguf-preview",
      expiresAtMs: Date.now() + 5 * 60 * 1000,
      sourcePath: "/Users/example/Models/Qwen3-4B-Q4_K_M.gguf",
      displayName: "Qwen3-4B",
      fileName: "Qwen3-4B-Q4_K_M.gguf",
      sizeBytes: 2.8 * 1024 ** 3,
      ggufVersion: 3,
      tensorCount: 399,
      metadataCount: 31,
      quantization: "Q4_K_M",
      ownership: "external",
      actionSummary: "只在 HAL100 中建立外部模型索引；不复制、不移动、不删除源文件",
      requiresConfirmation: true,
    };
  }
  return invoke<GgufImportPlan | null>("select_and_plan_gguf_import");
}

export async function applyGgufImport(planId: string): Promise<GgufImportResult> {
  if (!isTauriRuntime()) {
    throw new Error("浏览器预览模式不会写入模型索引");
  }
  return invoke<GgufImportResult>("apply_gguf_import", { planId });
}

export async function getOpenCodeDetection(): Promise<OpenCodeDetection> {
  if (!isTauriRuntime()) {
    return browserOpenCodeDetection;
  }
  return invoke<OpenCodeDetection>("get_opencode_detection");
}

export async function getAgentEcosystemCatalog(): Promise<AgentEcosystemCatalog> {
  if (!isTauriRuntime()) {
    return browserAgentEcosystemCatalog;
  }
  return invoke<AgentEcosystemCatalog>("get_agent_ecosystem_catalog");
}

export async function planOpenCodeConfiguration(): Promise<OpenCodeConfigPlan> {
  if (!isTauriRuntime()) {
    return {
      planId: "browser-preview",
      expiresAtMs: Date.now() + 5 * 60 * 1000,
      configPath: browserOpenCodeDetection.configPath,
      credentialPath: "~/Library/Application Support/HAL100/credentials/opencode-gateway.key",
      changes: [
        { path: "provider.hal100.npm", value: "@ai-sdk/openai-compatible" },
        { path: "provider.hal100.options.baseURL", value: "http://127.0.0.1:10100/v1" },
        { path: "provider.hal100.options.apiKey", value: "独立0600凭据文件（内容不显示）" },
        { path: "provider.hal100.models.hal100-active", value: "HAL100 当前模型" },
      ],
      createsBackup: false,
      preservesDefaultModel: true,
      requiresConfirmation: true,
    };
  }
  return invoke<OpenCodeConfigPlan>("plan_opencode_configuration");
}

export async function applyOpenCodeConfiguration(planId: string): Promise<OpenCodeApplyResult> {
  if (!isTauriRuntime()) {
    throw new Error("浏览器预览模式不会执行配置写入");
  }
  return invoke<OpenCodeApplyResult>("apply_opencode_configuration", { planId });
}

export async function discardOpenCodeConfigurationPlan(planId: string): Promise<boolean> {
  if (!isTauriRuntime()) return true;
  return invoke<boolean>("discard_opencode_configuration_plan", { planId });
}

export async function planOpenCodeDisconnection(): Promise<ExternalAgentDisconnectPlan> {
  if (!isTauriRuntime()) {
    return {
      planId: "browser-disconnect-preview",
      integrationId: "opencode",
      expiresAtMs: Date.now() + 5 * 60 * 1000,
      configPath: browserOpenCodeDetection.configPath,
      credentialPath: "~/Library/Application Support/HAL100/credentials/opencode-gateway.key",
      changes: [
        { path: "provider.hal100", action: "removeManagedFragment" },
        { path: "opencode-gateway-key", action: "removeManagedCredential" },
      ],
      createsBackup: true,
      revokesCredential: true,
      requiresConfirmation: true,
    };
  }
  return invoke<ExternalAgentDisconnectPlan>("plan_opencode_disconnection");
}

export async function discardOpenCodeDisconnectionPlan(planId: string): Promise<boolean> {
  if (!isTauriRuntime()) return true;
  return invoke<boolean>("discard_opencode_disconnection_plan", { planId });
}

export async function applyOpenCodeDisconnection(
  planId: string,
): Promise<ExternalAgentDisconnectResult> {
  if (!isTauriRuntime()) {
    throw new Error("浏览器预览模式不会执行接入移除");
  }
  return invoke<ExternalAgentDisconnectResult>("apply_opencode_disconnection", { planId });
}

export async function getPiCodingAgentDetection(): Promise<ExternalAgentDetection> {
  if (!isTauriRuntime()) return browserPiCodingAgentDetection;
  return invoke<ExternalAgentDetection>("get_pi_coding_agent_detection");
}

export async function planPiCodingAgentConfiguration(): Promise<ExternalAgentConfigurationPlan> {
  if (!isTauriRuntime()) {
    return {
      planId: "browser-pi-preview",
      integrationId: "pi-coding-agent",
      expiresAtMs: Date.now() + 5 * 60 * 1000,
      configPath: browserPiCodingAgentDetection.configPath,
      credentialPath:
        "~/Library/Application Support/HAL100/credentials/pi-coding-agent-gateway.key",
      changes: [
        { path: "providers.hal100.baseUrl", value: "http://127.0.0.1:10100/v1" },
        { path: "providers.hal100.api", value: "openai-completions" },
        {
          path: "providers.hal100.apiKey",
          value: "固定/bin/cat命令读取独立0600凭据（内容不显示）",
        },
        { path: "providers.hal100.models[hal100-active]", value: "HAL100 当前模型" },
      ],
      gatewayProtocol: "openAiChatCompletions",
      createsBackup: false,
      preservesDefaultModel: true,
      requiresConfirmation: true,
      modelProfileRevision: "managed-route-v1",
      warnings: [],
    };
  }
  return invoke<ExternalAgentConfigurationPlan>("plan_pi_coding_agent_configuration");
}

export async function applyPiCodingAgentConfiguration(
  planId: string,
): Promise<ExternalAgentConfigurationResult> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会执行配置写入");
  return invoke<ExternalAgentConfigurationResult>("apply_pi_coding_agent_configuration", {
    planId,
  });
}

export async function discardPiCodingAgentConfigurationPlan(planId: string): Promise<boolean> {
  if (!isTauriRuntime()) return true;
  return invoke<boolean>("discard_pi_coding_agent_configuration_plan", { planId });
}

export async function planPiCodingAgentDisconnection(): Promise<ExternalAgentDisconnectPlan> {
  if (!isTauriRuntime()) {
    return {
      planId: "browser-pi-disconnect-preview",
      integrationId: "pi-coding-agent",
      expiresAtMs: Date.now() + 5 * 60 * 1000,
      configPath: browserPiCodingAgentDetection.configPath,
      credentialPath:
        "~/Library/Application Support/HAL100/credentials/pi-coding-agent-gateway.key",
      changes: [
        { path: "providers.hal100", action: "removeManagedFragment" },
        { path: "pi-coding-agent-gateway-key", action: "removeManagedCredential" },
      ],
      createsBackup: true,
      revokesCredential: true,
      requiresConfirmation: true,
    };
  }
  return invoke<ExternalAgentDisconnectPlan>("plan_pi_coding_agent_disconnection");
}

export async function discardPiCodingAgentDisconnectionPlan(planId: string): Promise<boolean> {
  if (!isTauriRuntime()) return true;
  return invoke<boolean>("discard_pi_coding_agent_disconnection_plan", { planId });
}

export async function applyPiCodingAgentDisconnection(
  planId: string,
): Promise<ExternalAgentDisconnectResult> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会执行接入移除");
  return invoke<ExternalAgentDisconnectResult>("apply_pi_coding_agent_disconnection", { planId });
}

export async function getOpenClawDetection(): Promise<ExternalAgentDetection> {
  if (!isTauriRuntime()) return browserOpenClawDetection;
  return invoke<ExternalAgentDetection>("get_openclaw_detection");
}

export async function planOpenClawConfiguration(
  protocol: ExternalAgentGatewayProtocol,
): Promise<ExternalAgentConfigurationPlan> {
  if (!isTauriRuntime()) {
    const api = {
      openAiChatCompletions: "openai-completions",
      openAiResponses: "openai-responses",
      anthropicMessages: "anthropic-messages",
    }[protocol];
    return {
      planId: "browser-openclaw-preview",
      integrationId: "openclaw",
      expiresAtMs: Date.now() + 5 * 60 * 1000,
      configPath: browserOpenClawDetection.configPath,
      credentialPath: "~/Library/Application Support/HAL100/credentials/openclaw-gateway.key",
      changes: [
        {
          path: "secrets.providers.hal100_gateway",
          value: "独立0600文件型SecretRef（内容不显示）",
        },
        {
          path: "models.providers.hal100.baseUrl",
          value:
            protocol === "anthropicMessages"
              ? "http://127.0.0.1:10100"
              : "http://127.0.0.1:10100/v1",
        },
        { path: "models.providers.hal100.api", value: api },
        { path: "models.providers.hal100.models[hal100-active]", value: "HAL100 当前模型" },
      ],
      gatewayProtocol: protocol,
      createsBackup: false,
      preservesDefaultModel: true,
      requiresConfirmation: true,
      modelProfileRevision: "managed-route-v1",
      warnings: ["HAL100不会修改OpenClaw默认模型，也不会启动、停止或重启OpenClaw服务。"],
    };
  }
  return invoke<ExternalAgentConfigurationPlan>("plan_openclaw_configuration", { protocol });
}

export async function applyOpenClawConfiguration(
  planId: string,
): Promise<ExternalAgentConfigurationResult> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会执行配置写入");
  return invoke<ExternalAgentConfigurationResult>("apply_openclaw_configuration", { planId });
}

export async function discardOpenClawConfigurationPlan(planId: string): Promise<boolean> {
  if (!isTauriRuntime()) return true;
  return invoke<boolean>("discard_openclaw_configuration_plan", { planId });
}

export async function planOpenClawDisconnection(): Promise<ExternalAgentDisconnectPlan> {
  if (!isTauriRuntime()) {
    return {
      planId: "browser-openclaw-disconnect-preview",
      integrationId: "openclaw",
      expiresAtMs: Date.now() + 5 * 60 * 1000,
      configPath: browserOpenClawDetection.configPath,
      credentialPath: "~/Library/Application Support/HAL100/credentials/openclaw-gateway.key",
      changes: [
        { path: "models.providers.hal100", action: "removeManagedFragment" },
        { path: "secrets.providers.hal100_gateway", action: "removeManagedFragment" },
        { path: "openclaw-gateway-key", action: "removeManagedCredential" },
      ],
      createsBackup: true,
      revokesCredential: true,
      requiresConfirmation: true,
    };
  }
  return invoke<ExternalAgentDisconnectPlan>("plan_openclaw_disconnection");
}

export async function discardOpenClawDisconnectionPlan(planId: string): Promise<boolean> {
  if (!isTauriRuntime()) return true;
  return invoke<boolean>("discard_openclaw_disconnection_plan", { planId });
}

export async function applyOpenClawDisconnection(
  planId: string,
): Promise<ExternalAgentDisconnectResult> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会执行接入移除");
  return invoke<ExternalAgentDisconnectResult>("apply_openclaw_disconnection", { planId });
}

export async function getHermesAgentDetection(): Promise<ExternalAgentDetection> {
  if (!isTauriRuntime()) return browserHermesAgentDetection;
  return invoke<ExternalAgentDetection>("get_hermes_agent_detection");
}

export async function planHermesAgentConfiguration(): Promise<ExternalAgentConfigurationPlan> {
  if (!isTauriRuntime()) {
    return {
      planId: "browser-hermes-preview",
      integrationId: "hermes-agent",
      expiresAtMs: Date.now() + 5 * 60 * 1000,
      configPath: browserHermesAgentDetection.configPath,
      credentialPath: "~/.hermes/.env",
      changes: [
        { path: "providers.hal100.api", value: "http://127.0.0.1:10100/v1" },
        { path: "providers.hal100.transport", value: "chat_completions" },
        {
          path: "providers.hal100.key_env",
          value: "HAL100_HERMES_GATEWAY_KEY（值不显示）",
        },
        { path: "providers.hal100.models[hal100-active]", value: "HAL100 当前模型" },
      ],
      gatewayProtocol: "openAiChatCompletions",
      createsBackup: false,
      preservesDefaultModel: true,
      requiresConfirmation: true,
      modelProfileRevision: "managed-route-v1",
      warnings: [
        "Hermes 0.18.2 要求模型上下文至少 64000 Token；真实桌面环境会在预览前校验。",
        "HAL100 只管理 default Profile 中的 hal100 Provider 和专属环境变量。",
      ],
    };
  }
  return invoke<ExternalAgentConfigurationPlan>("plan_hermes_agent_configuration");
}

export async function applyHermesAgentConfiguration(
  planId: string,
): Promise<ExternalAgentConfigurationResult> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会执行配置写入");
  return invoke<ExternalAgentConfigurationResult>("apply_hermes_agent_configuration", {
    planId,
  });
}

export async function discardHermesAgentConfigurationPlan(planId: string): Promise<boolean> {
  if (!isTauriRuntime()) return true;
  return invoke<boolean>("discard_hermes_agent_configuration_plan", { planId });
}

export async function planHermesAgentDisconnection(): Promise<ExternalAgentDisconnectPlan> {
  if (!isTauriRuntime()) {
    return {
      planId: "browser-hermes-disconnect-preview",
      integrationId: "hermes-agent",
      expiresAtMs: Date.now() + 5 * 60 * 1000,
      configPath: browserHermesAgentDetection.configPath,
      credentialPath: "~/.hermes/.env",
      changes: [
        { path: "providers.hal100", action: "removeManagedFragment" },
        { path: "HAL100_HERMES_GATEWAY_KEY", action: "removeManagedCredential" },
      ],
      createsBackup: true,
      revokesCredential: true,
      requiresConfirmation: true,
    };
  }
  return invoke<ExternalAgentDisconnectPlan>("plan_hermes_agent_disconnection");
}

export async function discardHermesAgentDisconnectionPlan(planId: string): Promise<boolean> {
  if (!isTauriRuntime()) return true;
  return invoke<boolean>("discard_hermes_agent_disconnection_plan", { planId });
}

export async function applyHermesAgentDisconnection(
  planId: string,
): Promise<ExternalAgentDisconnectResult> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会执行接入移除");
  return invoke<ExternalAgentDisconnectResult>("apply_hermes_agent_disconnection", { planId });
}
