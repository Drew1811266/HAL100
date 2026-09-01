import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  BookmarkPlus,
  CheckCircle2,
  ChevronRight,
  Clock3,
  Cpu,
  Pencil,
  Play,
  RefreshCw,
  ShieldCheck,
  Trash2,
  X,
} from "lucide-react";
import { type FormEvent, useState } from "react";
import { NavLink } from "react-router-dom";
import { PageHeader } from "../../components/ui/PageHeader";
import {
  applyRuntimeProfileActivation,
  deleteRuntimeProfile,
  discardRuntimeProfileActivationPlan,
  type ExternalRuntimeProfileCandidate,
  getInferenceCapabilityCatalog,
  getRuntimeProfileCatalog,
  type InferenceAccelerator,
  type InferenceEngineCapability,
  type InferenceEngineRecommendationReason,
  type InferenceEngineSupportEvidenceKind,
  type InferenceEngineSupportStatus,
  isRuntimeProfileFailure,
  isTauriRuntime,
  planRuntimeProfileActivation,
  type RuntimeProfileActivationPlan,
  type RuntimeProfileCatalog,
  type RuntimeProfileDraft,
  type RuntimeProfileFailureCode,
  type RuntimeProfileIssue,
  type RuntimeProfileReadiness,
  type RuntimeProfileSummary,
  type RuntimeProfileSupportCell,
  reverifyExternalRuntimeProfile,
  saveCurrentRuntimeProfile,
  saveExternalRuntimeProfile,
  updateRuntimeProfile,
} from "../../lib/desktop-api";

const acceleratorCopy: Record<InferenceAccelerator, string> = {
  cpu: "CPU",
  metal: "Metal",
  cuda: "CUDA",
  rocm: "ROCm",
  vulkan: "Vulkan",
  sycl: "SYCL",
  intelGpu: "Intel GPU",
  intelNpu: "Intel NPU",
};

const readinessCopy: Record<
  RuntimeProfileReadiness,
  { label: string; tone: string; detail: string }
> = {
  active: { label: "正在运行", tone: "ready", detail: "当前运行状态与此方案一致" },
  ready: { label: "可以运行", tone: "ready", detail: "模型、引擎和验证快照均可用" },
  needsVerification: {
    label: "需要复验",
    tone: "warning",
    detail: "设备策略或运行组件已变化，运行成功后会更新验证快照",
  },
  needsRepair: {
    label: "需要修复",
    tone: "danger",
    detail: "模型或推理引擎当前不可用",
  },
};

const issueCopy: Record<RuntimeProfileIssue, string> = {
  engineNotInstalled: "方案所需推理引擎当前不可用",
  backendUnavailable: "方案绑定的外部后端当前不可用",
  backendIdentityChanged: "外部后端地址或身份已经变化",
  engineIncompatible: "当前设备与方案所需引擎不兼容",
  engineVersionChanged: "引擎版本已变化",
  modelUnavailable: "模型缺失或未通过校验",
  modelIntegrityChanged: "模型完整性快照已变化",
  capacityPolicyChanged: "设备容量策略已更新",
  supportCellMissing: "方案缺少精确的平台、架构、加速器与部署身份",
  supportCellChanged: "方案绑定的支持格已不再匹配当前设备或正式清单",
};

const failureCopy: Record<RuntimeProfileFailureCode, string> = {
  invalidRequest: "方案信息无效，请检查填写内容",
  persistenceUnavailable: "暂时无法读取或保存方案，请重试",
  managedEngineUnavailable: "托管推理引擎当前不可用，请先检查运行状态",
  backendUnavailable: "方案绑定的后端当前不可用",
  engineClientUnavailable: "无法创建推理引擎探测连接，请重试",
  engineEndpointInvalid: "推理引擎地址无效，请检查后端配置",
  engineUnreachable: "推理引擎当前不可达，请检查服务是否正在运行",
  engineResponseInvalid: "推理引擎响应不符合受控协议，请检查服务版本与配置",
  engineAdapterRegistryInvalid: "推理引擎适配器注册异常，请更新或修复 HAL100",
  engineAdapterUnavailable: "此方案所需的推理引擎适配器当前不可用",
  qualificationUnavailable: "此引擎尚未提供受控协议资格验证",
  qualificationFailed: "推理引擎未通过受控协议资格验证",
  acceptanceEvidenceUnavailable: "此支持格缺少匹配的验收证据",
  actionPlanUnavailable: "运行计划已失效，请重新生成",
  noVerifiedRuntime: "当前没有正在运行且可验证的本地模型",
  duplicateProfile: "当前模型与引擎组合已经保存",
  profileNotFound: "运行方案不存在或已被删除",
  profileNeedsRepair: "方案引用的模型或引擎当前不可用",
  profileChanged: "方案保存的引擎或模型身份已经变化，请重新验证",
  liveVerificationRequired: "外部运行方案必须先完成实时复验",
  supportCellSelectionRequired: "当前匹配多个支持格，请明确选择设备与部署方式",
  invalidSupportCell: "方案支持格与当前设备或正式清单不匹配",
  runtimeDeviceUnproven: "实时资格检查无法证明引擎正在使用所选加速器",
  externalProfileRequired: "只有外部运行方案可以执行此操作",
  activationFailed: "运行方案切换失败；HAL100 已按安全策略处理原运行状态",
  activationRecoveryRequired: "存在未完成的方案切换，请先恢复到已知状态",
  interactionIncomplete: "确认操作未完成，请重试",
};

function profileReadinessDetail(profile: RuntimeProfileSummary): string {
  if (profile.ownership === "external" && profile.readiness === "needsVerification") {
    return "外部引擎或模型身份已变化，重新验证快照后才能运行";
  }
  return readinessCopy[profile.readiness].detail;
}

function errorMessage(error: unknown): string {
  if (isRuntimeProfileFailure(error)) return failureCopy[error.code];
  if (error instanceof Error) return error.message;
  return String(error);
}

function formatDateTime(value: number | null): string {
  if (value === null) return "尚未运行";
  return new Intl.DateTimeFormat("zh-CN", {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

function formatContext(tokens: number | null): string {
  if (tokens === null) return "由引擎决定";
  return tokens % 1024 === 0 ? `${tokens / 1024}K` : tokens.toLocaleString("zh-CN");
}

function formatEngineVersion(version: string): string {
  return version === "qualification-required" ? "版本未暴露（由部署身份绑定）" : version;
}

function formatReviewedThroughput(milliTokensPerSecond: number): string {
  return `${(milliTokensPerSecond / 1000).toLocaleString("zh-CN", {
    maximumFractionDigits: 1,
    minimumFractionDigits: 1,
  })} token/s`;
}

function engineLabel(engine: string): string {
  switch (engine) {
    case "llamaCpp":
      return "llama.cpp";
    case "vllm":
      return "vLLM";
    case "mlxLm":
      return "MLX-LM";
    case "mlcLlm":
      return "MLC LLM";
    case "openVinoModelServer":
      return "OpenVINO Model Server";
    case "sglang":
      return "SGLang";
    case "lmDeploy":
      return "LMDeploy";
    case "tensorRtLlm":
      return "TensorRT-LLM";
    default:
      return engine.charAt(0).toUpperCase() + engine.slice(1);
  }
}

const supportStatusCopy: Record<InferenceEngineSupportStatus, string> = {
  reserved: "规划中",
  connected: "已连接，待验收",
  verifiedExternal: "已验证外部",
  managed: "HAL100 托管",
};

const recommendationReasonCopy: Record<InferenceEngineRecommendationReason, string> = {
  hostCompatible: "匹配当前设备",
  formalSupport: "正式支持",
  managedLifecycle: "由 HAL100 管理生命周期",
  verifiedRuntimeObserved: "已观察到运行实例",
  connectedOnly: "仅证明服务可连接",
  hostMismatch: "当前设备不匹配",
  supportCellAmbiguous: "多个加速器支持等级不同，需明确选择",
  protocolRequiresExplicitQualification: "需要独立资格验收",
};

const supportEvidenceCopy: Record<InferenceEngineSupportEvidenceKind, string> = {
  officialContract: "官方合同",
  protocolQualification: "协议资格",
  platformRuntime: "平台真机",
  engineIdentity: "引擎身份",
  modelDeploymentIdentity: "模型部署身份",
  runtimeProfileLifecycle: "方案闭环",
  stability: "稳定性",
};

function sortEngineCapabilities(engines: InferenceEngineCapability[]): InferenceEngineCapability[] {
  return [...engines].sort(
    (left, right) =>
      (right.recommendation?.score ?? 0) - (left.recommendation?.score ?? 0) ||
      left.descriptor.displayName.localeCompare(right.descriptor.displayName, "zh-CN"),
  );
}

function evidenceLabel(candidate: ExternalRuntimeProfileCandidate): string {
  switch (candidate.evidence.kind) {
    case "contentDigest":
      return `内容摘要 ${candidate.evidence.value.slice(0, 12)}`;
    case "repositoryRevision":
      return `仓库版本 ${candidate.evidence.value.slice(0, 12)}`;
    case "deploymentFingerprint":
      return `部署指纹 ${candidate.evidence.value.slice(0, 12)}`;
    case "catalogIdentity":
      return `目录身份 ${candidate.evidence.value}`;
  }
}

function supportCellKey(cell: RuntimeProfileSupportCell): string {
  return `${cell.platform}:${cell.architecture}:${cell.accelerator}:${cell.deployment}`;
}

function supportCellLabel(cell: RuntimeProfileSupportCell): string {
  const platform =
    cell.platform === "macOs" ? "macOS" : cell.platform === "windows" ? "Windows" : "Linux";
  const accelerator = acceleratorCopy[cell.accelerator];
  return `${platform} · ${cell.architecture} · ${accelerator} · ${cell.deployment === "local" ? "本机" : "远程"}`;
}

function ProfileEditorDialog({
  profile,
  externalCandidate,
  saving,
  error,
  onCancel,
  onSave,
}: {
  profile: RuntimeProfileSummary | null;
  externalCandidate?: ExternalRuntimeProfileCandidate | null;
  saving: boolean;
  error: string | null;
  onCancel: () => void;
  onSave: (draft: RuntimeProfileDraft, supportCell: RuntimeProfileSupportCell | null) => void;
}) {
  const [name, setName] = useState(profile?.name ?? "");
  const [description, setDescription] = useState(profile?.description ?? "");
  const supportCells = externalCandidate?.supportCells ?? [];
  const [selectedSupportCellKey, setSelectedSupportCellKey] = useState(
    supportCells[0] ? supportCellKey(supportCells[0]) : "",
  );

  function submit(event: FormEvent) {
    event.preventDefault();
    const supportCell =
      supportCells.find((cell) => supportCellKey(cell) === selectedSupportCellKey) ?? null;
    onSave({ name, description }, supportCell);
  }

  return (
    <div className="dialog-backdrop" role="presentation">
      <section
        aria-labelledby="runtime-profile-editor-title"
        aria-modal="true"
        className="dialog runtime-profile-dialog"
        role="dialog"
      >
        <div className="dialog-heading">
          <div>
            <p className="eyebrow">
              {profile ? "编辑方案" : externalCandidate ? "保存外部组合" : "保存当前组合"}
            </p>
            <h2 id="runtime-profile-editor-title">{profile ? "修改方案信息" : "保存为运行方案"}</h2>
          </div>
          <button
            aria-label="关闭"
            className="icon-button"
            disabled={saving}
            onClick={onCancel}
            type="button"
          >
            <X size={17} />
          </button>
        </div>
        <p className="dialog-intro">
          {profile
            ? "这里只修改名称和说明，不改变已经验证的模型、引擎或设备策略。"
            : externalCandidate
              ? `将实时复验 ${externalCandidate.backendDisplayName} 的 ${externalCandidate.modelId}，只保存后端身份、引擎版本和类型化模型证据，不保存命令或凭据。`
              : "HAL100 只保存当前正在运行且已经校验的模型、引擎版本和设备策略，不保存路径、命令或凭据。"}
        </p>
        <form className="runtime-profile-form" onSubmit={submit}>
          <label>
            <span>方案名称</span>
            <input
              maxLength={80}
              onChange={(event) => setName(event.target.value)}
              placeholder="例如：代码助手"
              required
              value={name}
            />
          </label>
          {externalCandidate && supportCells.length > 0 && (
            <label>
              <span>运行支持格</span>
              <select
                aria-label="运行支持格"
                onChange={(event) => setSelectedSupportCellKey(event.target.value)}
                required
                value={selectedSupportCellKey}
              >
                {supportCells.map((cell) => (
                  <option key={supportCellKey(cell)} value={supportCellKey(cell)}>
                    {supportCellLabel(cell)}
                  </option>
                ))}
              </select>
              <small>明确选择后，方案会绑定此支持格；Rust 仍会在保存和激活前再次验证。</small>
            </label>
          )}
          <label>
            <span>说明</span>
            <textarea
              maxLength={500}
              onChange={(event) => setDescription(event.target.value)}
              placeholder="记录适用场景或验证结论（可选）"
              rows={4}
              value={description}
            />
          </label>
          {error && <p className="inline-error">{error}</p>}
          <div className="dialog-actions">
            <button className="secondary-button" disabled={saving} onClick={onCancel} type="button">
              取消
            </button>
            <button className="primary-button" disabled={saving || !name.trim()} type="submit">
              {saving ? "正在保存…" : profile ? "保存修改" : "保存方案"}
            </button>
          </div>
        </form>
      </section>
    </div>
  );
}

function ActivationDialog({
  plan,
  applying,
  error,
  onCancel,
  onApply,
}: {
  plan: RuntimeProfileActivationPlan;
  applying: boolean;
  error: string | null;
  onCancel: () => void;
  onApply: () => void;
}) {
  return (
    <div className="dialog-backdrop" role="presentation">
      <section
        aria-labelledby="runtime-profile-activation-title"
        aria-modal="true"
        className="dialog runtime-profile-dialog"
        role="dialog"
      >
        <div className="dialog-heading">
          <div>
            <p className="eyebrow">一次性切换计划</p>
            <h2 id="runtime-profile-activation-title">运行“{plan.profileName}”</h2>
          </div>
          <button
            aria-label="关闭"
            className="icon-button"
            disabled={applying}
            onClick={onCancel}
            type="button"
          >
            <X size={17} />
          </button>
        </div>
        <p className="dialog-intro">{plan.actionSummary}。</p>
        <dl className="runtime-profile-plan-grid">
          <div>
            <dt>目标模型</dt>
            <dd>{plan.modelDisplayName}</dd>
          </div>
          <div>
            <dt>推理引擎</dt>
            <dd>
              {plan.engine} {formatEngineVersion(plan.engineVersion)}
            </dd>
          </div>
          <div>
            <dt>上下文档位</dt>
            <dd>{formatContext(plan.contextWindowTokens)}</dd>
          </div>
          {plan.supportCell && (
            <div>
              <dt>支持格</dt>
              <dd>{supportCellLabel(plan.supportCell)}</dd>
            </div>
          )}
          <div>
            <dt>当前模型</dt>
            <dd>{plan.currentModelName ?? "未运行"}</dd>
          </div>
        </dl>
        {plan.issues.length > 0 && (
          <div className="runtime-profile-warning-list">
            <AlertTriangle size={17} />
            <div>
              <strong>运行成功后将更新验证快照</strong>
              <span>{plan.issues.map((issue) => issueCopy[issue]).join("、")}</span>
            </div>
          </div>
        )}
        <div className="safety-summary">
          <ShieldCheck size={17} />
          <p>计划只可使用一次；Rust 会重新检查方案与现实状态，切换失败时尝试恢复原模型。</p>
        </div>
        {!isTauriRuntime() && (
          <p className="inline-notice">浏览器预览模式只展示计划，不会启动模型。</p>
        )}
        {error && <p className="inline-error">{error}</p>}
        <div className="dialog-actions">
          <button className="secondary-button" disabled={applying} onClick={onCancel} type="button">
            取消
          </button>
          <button
            className="primary-button"
            disabled={applying || !isTauriRuntime()}
            onClick={onApply}
            type="button"
          >
            <Play size={14} />
            {applying ? "正在切换…" : plan.requiresConfirmation ? "确认并切换" : "运行方案"}
          </button>
        </div>
      </section>
    </div>
  );
}

function DeleteDialog({
  profile,
  deleting,
  error,
  onCancel,
  onDelete,
}: {
  profile: RuntimeProfileSummary;
  deleting: boolean;
  error: string | null;
  onCancel: () => void;
  onDelete: () => void;
}) {
  return (
    <div className="dialog-backdrop" role="presentation">
      <section
        aria-labelledby="runtime-profile-delete-title"
        aria-modal="true"
        className="dialog runtime-profile-dialog"
        role="dialog"
      >
        <div className="dialog-heading">
          <div>
            <p className="eyebrow">删除方案</p>
            <h2 id="runtime-profile-delete-title">删除“{profile.name}”</h2>
          </div>
        </div>
        <p className="dialog-intro">
          只会删除这条运行方案，不会停止当前模型，也不会删除模型文件或推理引擎。
        </p>
        {error && <p className="inline-error">{error}</p>}
        <div className="dialog-actions">
          <button className="secondary-button" disabled={deleting} onClick={onCancel} type="button">
            取消
          </button>
          <button className="danger-button" disabled={deleting} onClick={onDelete} type="button">
            {deleting ? "正在删除…" : "删除方案"}
          </button>
        </div>
      </section>
    </div>
  );
}

export function RuntimeProfilesPage() {
  const queryClient = useQueryClient();
  const [editorProfile, setEditorProfile] = useState<RuntimeProfileSummary | null | undefined>();
  const [externalCandidate, setExternalCandidate] =
    useState<ExternalRuntimeProfileCandidate | null>(null);
  const [activationPlan, setActivationPlan] = useState<RuntimeProfileActivationPlan | null>(null);
  const [deleteProfile, setDeleteProfile] = useState<RuntimeProfileSummary | null>(null);
  const [operationError, setOperationError] = useState<string | null>(null);

  const catalog = useQuery({
    queryKey: ["runtime-profiles"],
    queryFn: getRuntimeProfileCatalog,
    refetchInterval: 4_000,
  });
  const capabilities = useQuery({
    queryKey: ["inference-capabilities"],
    queryFn: getInferenceCapabilityCatalog,
    staleTime: 60_000,
  });

  function acceptCatalog(next: RuntimeProfileCatalog) {
    queryClient.setQueryData(["runtime-profiles"], next);
    void queryClient.invalidateQueries({ queryKey: ["llama-cpp-status"] });
    void queryClient.invalidateQueries({ queryKey: ["backend-catalog"] });
    void queryClient.invalidateQueries({ queryKey: ["audit-log"] });
  }

  const saveMutation = useMutation({
    mutationFn: saveCurrentRuntimeProfile,
    onSuccess: (next) => {
      acceptCatalog(next);
      setEditorProfile(undefined);
      setOperationError(null);
    },
    onError: (error) => setOperationError(errorMessage(error)),
  });
  const saveExternalMutation = useMutation({
    mutationFn: ({
      candidate,
      draft,
      supportCell,
    }: {
      candidate: ExternalRuntimeProfileCandidate;
      draft: RuntimeProfileDraft;
      supportCell: RuntimeProfileSupportCell | null;
    }) =>
      saveExternalRuntimeProfile({
        ...draft,
        backendId: candidate.backendId,
        modelId: candidate.modelId,
        expectedEvidence: candidate.evidence,
        supportCell,
      }),
    onSuccess: (next) => {
      acceptCatalog(next);
      setExternalCandidate(null);
      setOperationError(null);
    },
    onError: (error) => setOperationError(errorMessage(error)),
  });
  const updateMutation = useMutation({
    mutationFn: ({ profileId, draft }: { profileId: string; draft: RuntimeProfileDraft }) =>
      updateRuntimeProfile(profileId, draft),
    onSuccess: (next) => {
      acceptCatalog(next);
      setEditorProfile(undefined);
      setOperationError(null);
    },
    onError: (error) => setOperationError(errorMessage(error)),
  });
  const reverifyMutation = useMutation({
    mutationFn: reverifyExternalRuntimeProfile,
    onSuccess: (next) => {
      acceptCatalog(next);
      setOperationError(null);
    },
    onError: (error) => setOperationError(errorMessage(error)),
  });
  const planMutation = useMutation({
    mutationFn: planRuntimeProfileActivation,
    onSuccess: (plan) => {
      setActivationPlan(plan);
      setOperationError(null);
    },
    onError: (error) => setOperationError(errorMessage(error)),
  });
  const applyMutation = useMutation({
    mutationFn: applyRuntimeProfileActivation,
    onSuccess: (result) => {
      acceptCatalog(result.catalog);
      setActivationPlan(null);
      setOperationError(null);
    },
    onError: (error) => setOperationError(errorMessage(error)),
  });
  const deleteMutation = useMutation({
    mutationFn: deleteRuntimeProfile,
    onSuccess: (next) => {
      acceptCatalog(next);
      setDeleteProfile(null);
      setOperationError(null);
    },
    onError: (error) => setOperationError(errorMessage(error)),
  });

  if (catalog.isPending) {
    return <div className="state-message">正在读取本机运行方案…</div>;
  }
  if (catalog.isError) {
    return <div className="state-message error">{errorMessage(catalog.error)}</div>;
  }

  const data = catalog.data;
  const managedEngine = capabilities.data?.engines[0];
  const externalCandidates = capabilities.data?.runtimeProfileCandidates ?? [];
  const compatibilityLabel = capabilities.isError
    ? "能力检测不可用"
    : managedEngine
      ? managedEngine.compatibility.compatible
        ? `${managedEngine.compatibility.matchedAccelerators
            .map((accelerator) => acceleratorCopy[accelerator])
            .join(" + ")} · 已兼容`
        : "当前设备不兼容"
      : "正在检测能力";
  const editorOpen = editorProfile !== undefined || externalCandidate !== null;
  return (
    <div className="page-content models-page runtime-profiles-page">
      <PageHeader
        action={
          <button
            className="primary-button refresh-button"
            disabled={!data.canSaveCurrent}
            onClick={() => {
              setOperationError(null);
              setExternalCandidate(null);
              setEditorProfile(null);
            }}
            title={data.canSaveCurrent ? "保存当前正在运行的组合" : "先在“运行”中启动一个本地模型"}
            type="button"
          >
            <BookmarkPlus size={14} />
            保存当前方案
          </button>
        }
        className="model-page-header"
        description="保存已经验证的模型与推理引擎组合，以后可以安全预检并快速切换。"
        eyebrow="个人运行环境"
        title="运行方案"
      />
      <section className="runtime-profile-principle">
        <ShieldCheck size={19} />
        <div>
          <strong>只保存可验证的运行身份</strong>
          <p>方案保存在本机，不包含模型路径、启动命令或凭据；设备策略变化后必须重新运行并复验。</p>
        </div>
        <span className={`status-pill ${managedEngine?.compatibility.compatible ? "ready" : ""}`}>
          {compatibilityLabel}
        </span>
      </section>

      {capabilities.data && (
        <section aria-label="推理引擎能力与建议" className="runtime-profile-engine-catalog">
          <header>
            <div>
              <strong>推理引擎能力与建议</strong>
              <p>
                建议由 Rust
                根据当前设备、正式支持状态和已观察实例确定；连接状态不会直接变成可运行方案。
              </p>
            </div>
            <span className="status-pill">
              {capabilities.data.host.platform} · {capabilities.data.host.architecture}
            </span>
          </header>
          <div className="runtime-profile-engine-list">
            {sortEngineCapabilities(capabilities.data.engines).map((capability, index) => {
              const recommendation = capability.recommendation;
              const supportStatus = capability.compatibility.supportStatus;
              const supportEvidence = capability.supportEvidence;
              return (
                <article className="runtime-profile-engine-row" key={capability.descriptor.kind}>
                  <span className="runtime-profile-engine-rank">{index + 1}</span>
                  <div>
                    <strong>{capability.descriptor.displayName}</strong>
                    <p>
                      {supportStatus ? supportStatusCopy[supportStatus] : "未匹配支持单元"}
                      {recommendation
                        ? ` · ${recommendation.reasons.map((reason) => recommendationReasonCopy[reason]).join(" · ")}`
                        : ""}
                    </p>
                    {supportEvidence && (
                      <small>
                        证据 {supportEvidence.verified.length}/
                        {supportEvidence.verified.length + supportEvidence.missing.length}
                        {supportEvidence.missing.length > 0 &&
                          ` · 待补：${supportEvidence.missing
                            .map((evidence) => supportEvidenceCopy[evidence])
                            .join("、")}`}
                      </small>
                    )}
                  </div>
                  <span className={`status-pill ${recommendation?.eligible ? "ready" : ""}`}>
                    {recommendation ? `${recommendation.score} 分` : "待检测"}
                  </span>
                </article>
              );
            })}
          </div>
        </section>
      )}

      {externalCandidates.length > 0 && (
        <section aria-label="外部运行身份候选" className="runtime-profile-candidates">
          <header>
            <div>
              <strong>已识别外部运行身份</strong>
              <p>
                以下候选来自已保存的外部后端与实时模型证据。保存时 Rust
                会重新复验，运行时还会再次检查漂移。
              </p>
            </div>
            <span className="status-pill ready">{externalCandidates.length} 个可验证候选</span>
          </header>
          <div className="runtime-profile-candidate-list">
            {externalCandidates.slice(0, 6).map((candidate) => (
              <article
                className="runtime-profile-candidate"
                key={`${candidate.backendId}:${candidate.modelId}:${candidate.modelDigest}`}
              >
                <div>
                  <strong>{candidate.modelId}</strong>
                  <span>{candidate.backendDisplayName}</span>
                </div>
                <div className="runtime-profile-candidate-meta">
                  <span>
                    {engineLabel(candidate.engine)} {formatEngineVersion(candidate.engineVersion)}
                    {candidate.parameterSize ? ` · ${candidate.parameterSize}` : ""}
                    {candidate.quantization ? ` · ${candidate.quantization}` : ""}
                  </span>
                  <code title={`${candidate.evidence.algorithm}: ${candidate.evidence.value}`}>
                    {evidenceLabel(candidate)}
                  </code>
                  <button
                    className="secondary-button"
                    onClick={() => {
                      setOperationError(null);
                      setEditorProfile(undefined);
                      setExternalCandidate(candidate);
                    }}
                    type="button"
                  >
                    <BookmarkPlus size={13} />
                    保存为方案
                  </button>
                </div>
              </article>
            ))}
          </div>
          {externalCandidates.length > 6 && (
            <p className="runtime-profile-candidate-overflow">
              另有 {externalCandidates.length - 6} 个候选，将在外部方案编辑器中按需展开。
            </p>
          )}
        </section>
      )}

      {operationError && !editorOpen && !activationPlan && !deleteProfile && (
        <p className="inline-error runtime-profile-page-error">{operationError}</p>
      )}

      {data.profiles.length === 0 ? (
        <section className="runtime-profile-empty">
          <BookmarkPlus size={24} />
          <strong>还没有保存运行方案</strong>
          <p>可以先启动托管本地模型，或从上方实时识别的外部运行身份保存一套方案。</p>
          <NavLink className="primary-button" to="/workspace/runtime">
            前往运行
            <ChevronRight size={14} />
          </NavLink>
        </section>
      ) : (
        <section aria-label="已保存的运行方案" className="runtime-profile-grid">
          {data.profiles.map((profile) => {
            const status = readinessCopy[profile.readiness];
            const running = profile.readiness === "active";
            const blocked = profile.readiness === "needsRepair";
            const requiresExternalReverification =
              profile.ownership === "external" && profile.readiness === "needsVerification";
            return (
              <article className={`runtime-profile-card ${profile.readiness}`} key={profile.id}>
                <header>
                  <div className="runtime-profile-card-title">
                    <span className={`runtime-profile-state-icon ${status.tone}`}>
                      {running ? (
                        <CheckCircle2 size={18} />
                      ) : blocked ? (
                        <AlertTriangle size={18} />
                      ) : (
                        <Cpu size={18} />
                      )}
                    </span>
                    <div>
                      <h2>{profile.name}</h2>
                      <p>{profile.description || "未添加说明"}</p>
                    </div>
                  </div>
                  <span className={`status-pill ${status.tone}`}>{status.label}</span>
                </header>

                <div className="runtime-profile-spec">
                  <div>
                    <span>模型</span>
                    <strong>{profile.modelDisplayName}</strong>
                  </div>
                  <div>
                    <span>推理引擎</span>
                    <strong>
                      {profile.engine} {formatEngineVersion(profile.engineVersion)}
                    </strong>
                  </div>
                  <div>
                    <span>上下文</span>
                    <strong>{formatContext(profile.contextWindowTokens)}</strong>
                  </div>
                  {profile.adapterBinding.supportCell && (
                    <div>
                      <span>支持格</span>
                      <strong>{supportCellLabel(profile.adapterBinding.supportCell)}</strong>
                    </div>
                  )}
                </div>

                {profile.reviewedPerformance && (
                  <div className="runtime-profile-performance">
                    <ShieldCheck size={14} />
                    <div>
                      <strong>同模型与当前设备的受审阅实测</strong>
                      <span>
                        p95 {profile.reviewedPerformance.p95LatencyMs.toLocaleString("zh-CN")} ms ·{" "}
                        {formatReviewedThroughput(
                          profile.reviewedPerformance.sampleCompletionTokensPerSecondMilli,
                        )}
                      </span>
                      <small>
                        固定工作负载 {profile.reviewedPerformance.workloadRevision}
                        ，仅作方案间参考，不代表实时保证
                      </small>
                    </div>
                  </div>
                )}

                <div className={`runtime-profile-readiness ${status.tone}`}>
                  {profile.readiness === "needsVerification" ? (
                    <AlertTriangle size={14} />
                  ) : (
                    <ShieldCheck size={14} />
                  )}
                  <span>
                    {profileReadinessDetail(profile)}
                    {profile.issues.length > 0 &&
                      `：${profile.issues.map((issue) => issueCopy[issue]).join("、")}`}
                  </span>
                </div>

                <footer>
                  <span>
                    <Clock3 size={13} />
                    最近运行 {formatDateTime(profile.lastActivatedAtMs)}
                  </span>
                  <div className="runtime-profile-actions">
                    <button
                      aria-label={`编辑 ${profile.name}`}
                      className="icon-button"
                      onClick={() => {
                        setOperationError(null);
                        setExternalCandidate(null);
                        setEditorProfile(profile);
                      }}
                      title="编辑名称与说明"
                      type="button"
                    >
                      <Pencil size={14} />
                    </button>
                    <button
                      aria-label={`删除 ${profile.name}`}
                      className="icon-button"
                      onClick={() => {
                        setOperationError(null);
                        setDeleteProfile(profile);
                      }}
                      title="删除方案定义"
                      type="button"
                    >
                      <Trash2 size={14} />
                    </button>
                    {requiresExternalReverification ? (
                      <button
                        className="primary-button"
                        disabled={reverifyMutation.isPending}
                        onClick={() => reverifyMutation.mutate(profile.id)}
                        type="button"
                      >
                        <RefreshCw size={14} />
                        重新验证
                      </button>
                    ) : (
                      <button
                        className={running ? "secondary-button" : "primary-button"}
                        disabled={running || blocked || planMutation.isPending}
                        onClick={() => planMutation.mutate(profile.id)}
                        type="button"
                      >
                        <Play size={14} />
                        {running ? "正在运行" : blocked ? "需要修复" : "运行方案"}
                      </button>
                    )}
                  </div>
                </footer>
              </article>
            );
          })}
        </section>
      )}

      {editorOpen && (
        <ProfileEditorDialog
          error={operationError}
          externalCandidate={externalCandidate}
          onCancel={() => {
            setEditorProfile(undefined);
            setExternalCandidate(null);
            setOperationError(null);
          }}
          onSave={(draft, supportCell) => {
            if (editorProfile) {
              updateMutation.mutate({ profileId: editorProfile.id, draft });
            } else if (externalCandidate) {
              saveExternalMutation.mutate({ candidate: externalCandidate, draft, supportCell });
            } else {
              saveMutation.mutate(draft);
            }
          }}
          profile={editorProfile ?? null}
          saving={
            saveMutation.isPending || saveExternalMutation.isPending || updateMutation.isPending
          }
        />
      )}

      {activationPlan && (
        <ActivationDialog
          applying={applyMutation.isPending}
          error={operationError}
          onApply={() => applyMutation.mutate(activationPlan.planId)}
          onCancel={() => {
            void discardRuntimeProfileActivationPlan(activationPlan.planId);
            setActivationPlan(null);
            setOperationError(null);
          }}
          plan={activationPlan}
        />
      )}

      {deleteProfile && (
        <DeleteDialog
          deleting={deleteMutation.isPending}
          error={operationError}
          onCancel={() => {
            setDeleteProfile(null);
            setOperationError(null);
          }}
          onDelete={() => deleteMutation.mutate(deleteProfile.id)}
          profile={deleteProfile}
        />
      )}
    </div>
  );
}
