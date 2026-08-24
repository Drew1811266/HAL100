import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  ChevronRight,
  ClipboardCopy,
  KeyRound,
  RefreshCw,
  ShieldCheck,
  UserRoundPlus,
  X,
} from "lucide-react";
import { useEffect, useState } from "react";
import { Drawer } from "../../components/ui/Drawer";
import { PageHeader } from "../../components/ui/PageHeader";
import {
  applyHermesAgentConfiguration,
  applyHermesAgentDisconnection,
  applyOpenClawConfiguration,
  applyOpenClawDisconnection,
  applyOpenCodeConfiguration,
  applyOpenCodeDisconnection,
  applyPiCodingAgentConfiguration,
  applyPiCodingAgentDisconnection,
  createGenericClient,
  discardHermesAgentConfigurationPlan,
  discardHermesAgentDisconnectionPlan,
  discardOpenClawConfigurationPlan,
  discardOpenClawDisconnectionPlan,
  discardOpenCodeConfigurationPlan,
  discardOpenCodeDisconnectionPlan,
  discardPiCodingAgentConfigurationPlan,
  discardPiCodingAgentDisconnectionPlan,
  type ExternalAgentConfigurationPlan,
  type ExternalAgentDetection,
  type ExternalAgentDisconnectPlan,
  type ExternalAgentGatewayProtocol,
  type ExternalAgentIntegrationState,
  type GenericClientCredential,
  getAgentEcosystemCatalog,
  getGenericClientCatalog,
  getHermesAgentDetection,
  getOpenClawDetection,
  getOpenCodeDetection,
  getPiCodingAgentDetection,
  isTauriRuntime,
  type OpenCodeConfigPlan,
  type OpenCodeIntegrationState,
  planHermesAgentConfiguration,
  planHermesAgentDisconnection,
  planOpenClawConfiguration,
  planOpenClawDisconnection,
  planOpenCodeConfiguration,
  planOpenCodeDisconnection,
  planPiCodingAgentConfiguration,
  planPiCodingAgentDisconnection,
  revokeGenericClient,
} from "../../lib/desktop-api";

function formatRequestTime(timestampMs: number): string {
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(new Date(timestampMs));
}

const integrationStateCopy: Record<
  OpenCodeIntegrationState,
  { label: string; tone: "ok" | "neutral" | "warning" }
> = {
  notConfigured: { label: "尚未配置", tone: "neutral" },
  configured: { label: "已由 HAL100 配置", tone: "ok" },
  conflict: { label: "存在配置冲突", tone: "warning" },
  modifiedOutsideHal100: { label: "配置已被外部修改", tone: "warning" },
};

const externalIntegrationStateCopy: Record<
  ExternalAgentIntegrationState,
  { label: string; tone: "ok" | "neutral" | "warning" }
> = {
  notInstalled: { label: "未检测到", tone: "neutral" },
  installedNotConfigured: { label: "尚未配置", tone: "neutral" },
  configured: { label: "已由 HAL100 配置", tone: "ok" },
  needsRefresh: { label: "配置需要刷新", tone: "warning" },
  conflict: { label: "存在配置冲突", tone: "warning" },
  modifiedOutsideHal100: { label: "配置已被外部修改", tone: "warning" },
  unsupportedVersion: { label: "版本暂不支持", tone: "warning" },
  blocked: { label: "接入被阻止", tone: "warning" },
};

const externalAgentProtocolCopy: Record<ExternalAgentGatewayProtocol, string> = {
  openAiChatCompletions: "Chat Completions",
  openAiResponses: "Responses",
  anthropicMessages: "Anthropic Messages",
};

type IntegrationId = "openCode" | "piCodingAgent" | "openClaw" | "hermesAgent" | "otherClients";

interface IntegrationDescriptor {
  brand: string;
  displayName: string;
  id: IntegrationId;
}

const integrationRegistry: Record<IntegrationId, IntegrationDescriptor> = {
  openCode: { id: "openCode", displayName: "OpenCode", brand: "OC" },
  piCodingAgent: { id: "piCodingAgent", displayName: "Pi Coding Agent", brand: "π" },
  openClaw: { id: "openClaw", displayName: "OpenClaw", brand: "CL" },
  hermesAgent: { id: "hermesAgent", displayName: "Hermes Agent", brand: "H" },
  otherClients: { id: "otherClients", displayName: "其他客户端", brand: "+" },
};

function IntegrationSummaryRow({
  actionLabel,
  description,
  descriptor,
  onOpen,
  status,
}: {
  actionLabel: string;
  description: string;
  descriptor: IntegrationDescriptor;
  onOpen: () => void;
  status: { label: string; tone: "ok" | "neutral" | "warning" };
}) {
  return (
    <section className="integration-summary-row" data-integration-id={descriptor.id}>
      <div className="integration-brand">{descriptor.brand}</div>
      <div className="integration-summary-copy">
        <h2>{descriptor.displayName}</h2>
        <p>{description}</p>
      </div>
      <span className={`status-pill ${status.tone}`}>{status.label}</span>
      <button className="secondary-button" onClick={onOpen} type="button">
        {actionLabel}
      </button>
    </section>
  );
}

function integrationRecommendedAction({
  connected,
  installed,
  needsAttention,
}: {
  connected: boolean;
  installed: boolean;
  needsAttention: boolean;
}): string {
  if (needsAttention) return "解决问题";
  if (connected) return "查看详情";
  return installed ? "配置接入" : "查看接入方式";
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function ManagedAgentConfigurationDialog({
  displayName,
  plan,
  applying,
  error,
  onCancel,
  onApply,
}: {
  displayName: string;
  plan: OpenCodeConfigPlan | ExternalAgentConfigurationPlan;
  applying: boolean;
  error: string | null;
  onCancel: () => void;
  onApply: () => void;
}) {
  const runtime = isTauriRuntime();
  return (
    <div className="dialog-backdrop" role="presentation">
      <section
        aria-labelledby="managed-agent-dialog-title"
        aria-modal="true"
        className="dialog"
        role="dialog"
      >
        <div className="dialog-heading">
          <div>
            <p className="eyebrow">需要确认</p>
            <h2 id="managed-agent-dialog-title">配置 {displayName}</h2>
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
        <p className="dialog-intro">
          将修改全局配置 <code>{plan.configPath}</code>。以下是唯一会由 HAL100 管理的字段：
        </p>
        <div className="change-preview">
          {plan.changes.map((change) => (
            <div key={change.path}>
              <code>+ {change.path}</code>
              <span>{change.value}</span>
            </div>
          ))}
        </div>
        <div className="safety-summary">
          <KeyRound size={17} />
          <p>
            Key 单独保存在 <code>{plan.credentialPath}</code>，权限为 0600；
            {plan.createsBackup ? "应用前会创建时间戳备份。" : "当前没有旧配置，无需创建备份。"}
            不会修改默认模型或已有 Provider。
          </p>
        </div>
        {"warnings" in plan && plan.warnings.length > 0 && (
          <div className="warning-list">
            <AlertTriangle size={17} />
            <div>
              {plan.warnings.map((warning) => (
                <p key={warning}>{warning}</p>
              ))}
            </div>
          </div>
        )}
        {!runtime && <p className="inline-notice">浏览器预览模式只能查看变更，不能应用。</p>}
        {error && <p className="inline-error">{error}</p>}
        <div className="dialog-actions">
          <button className="secondary-button" disabled={applying} onClick={onCancel} type="button">
            取消
          </button>
          <button
            className="primary-button"
            disabled={applying || !runtime}
            onClick={onApply}
            type="button"
          >
            {applying ? "正在验证并应用…" : "确认并应用配置"}
          </button>
        </div>
      </section>
    </div>
  );
}

function ManagedAgentDisconnectDialog({
  displayName,
  plan,
  applying,
  error,
  onCancel,
  onApply,
}: {
  displayName: string;
  plan: ExternalAgentDisconnectPlan;
  applying: boolean;
  error: string | null;
  onCancel: () => void;
  onApply: () => void;
}) {
  const runtime = isTauriRuntime();
  return (
    <div className="dialog-backdrop" role="presentation">
      <section
        aria-labelledby="managed-agent-disconnect-title"
        aria-modal="true"
        className="dialog"
        role="dialog"
      >
        <div className="dialog-heading">
          <div>
            <p className="eyebrow">需要确认</p>
            <h2 id="managed-agent-disconnect-title">断开 {displayName}</h2>
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
        <p className="dialog-intro">
          只会从 <code>{plan.configPath}</code> 移除 HAL100 自己管理的内容：
        </p>
        <div className="change-preview">
          {plan.changes.map((change) => (
            <div key={`${change.action}:${change.path}`}>
              <code>- {change.path}</code>
              <span>
                {change.action === "removeManagedCredential"
                  ? "吊销并删除专属 Key"
                  : "移除受管分片"}
              </span>
            </div>
          ))}
        </div>
        <div className="safety-summary">
          <ShieldCheck size={17} />
          <p>
            应用前会备份配置。用户的默认模型、其他 Provider 和项目配置不会被修改；
            {displayName} 专属 Key 吊销后无法继续调用 HAL100 Gateway。
          </p>
        </div>
        {!runtime && <p className="inline-notice">浏览器预览模式只能查看变更，不能断开接入。</p>}
        {error && <p className="inline-error">{error}</p>}
        <div className="dialog-actions">
          <button className="secondary-button" disabled={applying} onClick={onCancel} type="button">
            取消
          </button>
          <button
            className="danger-button"
            disabled={applying || !runtime}
            onClick={onApply}
            type="button"
          >
            {applying ? "等待原生确认…" : "确认断开接入"}
          </button>
        </div>
      </section>
    </div>
  );
}

function GenericClientAccess() {
  const queryClient = useQueryClient();
  const [displayName, setDisplayName] = useState("");
  const [issuedCredential, setIssuedCredential] = useState<GenericClientCredential | null>(null);
  const [copied, setCopied] = useState(false);
  const catalog = useQuery({
    queryKey: ["generic-client-catalog"],
    queryFn: getGenericClientCatalog,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
  const createMutation = useMutation({
    mutationFn: createGenericClient,
    onSuccess: (credential) => {
      setDisplayName("");
      setIssuedCredential(credential);
      queryClient.invalidateQueries({ queryKey: ["generic-client-catalog"] });
      queryClient.invalidateQueries({ queryKey: ["audit-log"] });
    },
  });
  const revokeMutation = useMutation({
    mutationFn: revokeGenericClient,
    onSuccess: (nextCatalog) => {
      queryClient.setQueryData(["generic-client-catalog"], nextCatalog);
      queryClient.invalidateQueries({ queryKey: ["audit-log"] });
    },
  });

  const copyIssuedKey = async () => {
    if (!issuedCredential) return;
    try {
      await navigator.clipboard.writeText(issuedCredential.apiKey);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  };

  return (
    <section
      className="generic-access-card drawer-generic-access"
      aria-labelledby="generic-access-title"
    >
      <div className="usage-section-heading">
        <div>
          <p className="eyebrow">通用接入</p>
          <h2 id="generic-access-title">OpenAI / Anthropic 客户端</h2>
        </div>
        <span>每个软件使用独立 Key</span>
      </div>
      <p className="section-description">
        Base URL 保持固定；切换模型或后端无需修改客户端。独立 Key 让 Token 能准确归属到具体软件。
      </p>

      <div className="generic-endpoint-grid">
        <article>
          <strong>OpenAI 兼容</strong>
          <code>http://127.0.0.1:10100/v1</code>
          <small>/v1/chat/completions · /v1/responses</small>
          <span>model: hal100-active</span>
        </article>
        <article>
          <strong>Anthropic Messages</strong>
          <code>http://127.0.0.1:10100</code>
          <small>/v1/messages</small>
          <span>支持 x-api-key、SSE和缓存 Usage · model: hal100-active</span>
        </article>
      </div>

      <form
        className="generic-client-form"
        onSubmit={(event) => {
          event.preventDefault();
          const name = displayName.trim();
          if (name && !createMutation.isPending) createMutation.mutate(name);
        }}
      >
        <label htmlFor="generic-client-name">客户端名称</label>
        <input
          id="generic-client-name"
          maxLength={80}
          onChange={(event) => setDisplayName(event.target.value)}
          placeholder="例如：Continue、团队脚本、我的编辑器"
          value={displayName}
        />
        <button
          className="primary-button"
          disabled={!displayName.trim() || createMutation.isPending || !isTauriRuntime()}
          type="submit"
        >
          <UserRoundPlus size={14} />
          {createMutation.isPending ? "正在签发…" : "生成独立 Key"}
        </button>
      </form>
      {!isTauriRuntime() && (
        <p className="inline-notice">
          浏览器预览不会签发凭据；Tauri 开发版只在创建时显示一次明文 Key。
        </p>
      )}
      {createMutation.isError && (
        <p className="inline-error">{errorMessage(createMutation.error)}</p>
      )}
      {revokeMutation.isError && (
        <p className="inline-error">{errorMessage(revokeMutation.error)}</p>
      )}

      {catalog.isPending ? (
        <div className="state-message compact-state">正在读取本地客户端凭据…</div>
      ) : catalog.isError ? (
        <div className="state-message error compact-state">{errorMessage(catalog.error)}</div>
      ) : catalog.data.clients.length === 0 ? (
        <div className="generic-client-empty">
          <KeyRound size={18} />
          <span>尚未签发通用客户端 Key。OpenCode 专属凭据不会显示在这里。</span>
        </div>
      ) : (
        <div className="generic-client-list">
          {catalog.data.clients.map((client) => (
            <article key={client.clientAppId}>
              <div>
                <strong>{client.displayName}</strong>
                <span>{client.displayPrefix}</span>
              </div>
              <small>{formatRequestTime(client.createdAtMs)}</small>
              <button
                className="danger-button compact-button"
                disabled={revokeMutation.isPending}
                onClick={() => revokeMutation.mutate(client.clientAppId)}
                type="button"
              >
                撤销 Key
              </button>
            </article>
          ))}
        </div>
      )}

      {issuedCredential && (
        <div className="dialog-backdrop" role="presentation">
          <section
            aria-labelledby="issued-key-title"
            aria-modal="true"
            className="dialog issued-key-dialog"
            role="dialog"
          >
            <div className="dialog-heading">
              <div>
                <p className="eyebrow">仅显示一次</p>
                <h2 id="issued-key-title">保存 {issuedCredential.client.displayName} 的 Key</h2>
              </div>
            </div>
            <p>关闭后 HAL100 无法再次显示明文；数据库只保存 SHA-256 摘要。</p>
            <code className="issued-key-value">{issuedCredential.apiKey}</code>
            <div className="dialog-actions">
              <button className="secondary-button" onClick={copyIssuedKey} type="button">
                <ClipboardCopy size={14} />
                {copied ? "已复制" : "复制 Key"}
              </button>
              <button
                className="primary-button"
                onClick={() => {
                  setIssuedCredential(null);
                  setCopied(false);
                }}
                type="button"
              >
                我已保存，关闭
              </button>
            </div>
          </section>
        </div>
      )}
    </section>
  );
}

function PiCodingAgentIntegrationCard() {
  const queryClient = useQueryClient();
  const detection = useQuery<ExternalAgentDetection>({
    queryKey: ["pi-coding-agent-detection"],
    queryFn: getPiCodingAgentDetection,
  });
  const [plan, setPlan] = useState<ExternalAgentConfigurationPlan | null>(null);
  const [disconnectPlan, setDisconnectPlan] = useState<ExternalAgentDisconnectPlan | null>(null);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [resultMessage, setResultMessage] = useState<string | null>(null);
  const planMutation = useMutation({
    mutationFn: planPiCodingAgentConfiguration,
    onSuccess: (nextPlan) => {
      setResultMessage(null);
      setPlan(nextPlan);
    },
  });
  const applyMutation = useMutation({
    mutationFn: (planId: string) => applyPiCodingAgentConfiguration(planId),
    onSuccess: async (result) => {
      setPlan(null);
      setResultMessage(
        result.backupPath
          ? `Pi 配置完成，备份已保存到 ${result.backupPath}`
          : "Pi 配置完成，独立凭据已生效。",
      );
      await queryClient.invalidateQueries({ queryKey: ["pi-coding-agent-detection"] });
    },
  });
  const disconnectPlanMutation = useMutation({
    mutationFn: planPiCodingAgentDisconnection,
    onSuccess: (nextPlan) => {
      setResultMessage(null);
      setDisconnectPlan(nextPlan);
    },
  });
  const disconnectMutation = useMutation({
    mutationFn: (planId: string) => applyPiCodingAgentDisconnection(planId),
    onSuccess: async (result) => {
      setDisconnectPlan(null);
      setResultMessage(
        result.backupPath
          ? `Pi 接入已断开，配置备份保存在 ${result.backupPath}`
          : "Pi 接入已断开，专属凭据已吊销。",
      );
      await queryClient.invalidateQueries({ queryKey: ["pi-coding-agent-detection"] });
    },
  });

  if (detection.isPending) {
    return <section className="integration-card state-message">正在检测 Pi Coding Agent…</section>;
  }
  if (detection.isError) {
    return (
      <section className="integration-card state-message error">
        {errorMessage(detection.error)}
      </section>
    );
  }

  const data = detection.data;
  const stateCopy = externalIntegrationStateCopy[data.integrationState];
  const connected =
    data.integrationState === "configured" || data.integrationState === "needsRefresh";
  const cannotConfigure =
    !data.installed ||
    data.integrationState === "conflict" ||
    data.integrationState === "modifiedOutsideHal100" ||
    data.integrationState === "unsupportedVersion" ||
    data.integrationState === "blocked";

  return (
    <>
      <IntegrationSummaryRow
        actionLabel={integrationRecommendedAction({
          connected,
          installed: data.installed,
          needsAttention:
            data.integrationState !== "notInstalled" &&
            data.integrationState !== "installedNotConfigured" &&
            data.integrationState !== "configured",
        })}
        description={
          data.installed ? "已检测到官方 Pi CLI，可使用独立身份接入" : "电脑上未发现官方 Pi CLI"
        }
        descriptor={integrationRegistry.piCodingAgent}
        onOpen={() => setDetailsOpen(true)}
        status={stateCopy}
      />
      {detailsOpen && (
        <Drawer
          description="官方 Pi Coding Agent 与 HAL100 内置 Runtime 使用不同 HOME、配置、会话和凭据。"
          eyebrow="软件接入"
          onClose={() => setDetailsOpen(false)}
          title="Pi Coding Agent"
        >
          <section className="integration-drawer-summary">
            <span className={`status-pill ${stateCopy.tone}`}>{stateCopy.label}</span>
            <p>
              {data.installed
                ? `已检测到 ${data.version ?? "未知版本"}`
                : "未从常用安装位置检测到官方 Pi CLI"}
            </p>
          </section>
          <dl className="integration-details">
            <div>
              <dt>Gateway Base URL</dt>
              <dd>http://127.0.0.1:10100/v1</dd>
            </div>
            <div>
              <dt>Pi 模型配置</dt>
              <dd>{data.configPath}</dd>
            </div>
            <div>
              <dt>模型契约</dt>
              <dd>{data.modelProfileRevision}</dd>
            </div>
            <div>
              <dt>隔离边界</dt>
              <dd>{connected ? "Pi 专属 Key · 独立于内置 Runtime" : "配置后启用"}</dd>
            </div>
          </dl>
          {data.warnings.length > 0 && (
            <div className="warning-list">
              <AlertTriangle size={17} />
              <div>
                {data.warnings.map((warning) => (
                  <p key={warning}>{warning}</p>
                ))}
              </div>
            </div>
          )}
          {(planMutation.isError || disconnectPlanMutation.isError) && (
            <p className="inline-error">
              {errorMessage(planMutation.error ?? disconnectPlanMutation.error)}
            </p>
          )}
          {resultMessage && <p className="inline-success">{resultMessage}</p>}
          <div className="integration-actions">
            <button
              className="secondary-button quiet-button"
              disabled={detection.isFetching}
              onClick={() => detection.refetch()}
              type="button"
            >
              {detection.isFetching ? "检测中…" : "重新检测"}
            </button>
            <button
              className="primary-button"
              disabled={
                cannotConfigure || planMutation.isPending || data.integrationState === "configured"
              }
              onClick={() => planMutation.mutate()}
              type="button"
            >
              {planMutation.isPending
                ? "正在生成预览…"
                : data.integrationState === "configured"
                  ? "配置已生效"
                  : data.integrationState === "needsRefresh"
                    ? "刷新 Pi 配置"
                    : "配置 Pi"}
            </button>
            {connected && (
              <button
                className="danger-button"
                disabled={disconnectPlanMutation.isPending}
                onClick={() => disconnectPlanMutation.mutate()}
                type="button"
              >
                {disconnectPlanMutation.isPending ? "正在生成预览…" : "断开接入"}
              </button>
            )}
          </div>
        </Drawer>
      )}
      {plan && (
        <ManagedAgentConfigurationDialog
          applying={applyMutation.isPending}
          displayName="Pi Coding Agent"
          error={applyMutation.isError ? errorMessage(applyMutation.error) : null}
          onApply={() => applyMutation.mutate(plan.planId)}
          onCancel={() => {
            if (!applyMutation.isPending) {
              void discardPiCodingAgentConfigurationPlan(plan.planId);
              setPlan(null);
            }
          }}
          plan={plan}
        />
      )}
      {disconnectPlan && (
        <ManagedAgentDisconnectDialog
          applying={disconnectMutation.isPending}
          displayName="Pi Coding Agent"
          error={disconnectMutation.isError ? errorMessage(disconnectMutation.error) : null}
          onApply={() => disconnectMutation.mutate(disconnectPlan.planId)}
          onCancel={() => {
            if (!disconnectMutation.isPending) {
              void discardPiCodingAgentDisconnectionPlan(disconnectPlan.planId);
              setDisconnectPlan(null);
            }
          }}
          plan={disconnectPlan}
        />
      )}
    </>
  );
}

function OpenClawIntegrationCard() {
  const queryClient = useQueryClient();
  const detection = useQuery<ExternalAgentDetection>({
    queryKey: ["openclaw-detection"],
    queryFn: getOpenClawDetection,
  });
  const [protocol, setProtocol] = useState<ExternalAgentGatewayProtocol>("openAiChatCompletions");
  const [plan, setPlan] = useState<ExternalAgentConfigurationPlan | null>(null);
  const [disconnectPlan, setDisconnectPlan] = useState<ExternalAgentDisconnectPlan | null>(null);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [resultMessage, setResultMessage] = useState<string | null>(null);
  useEffect(() => {
    if (detection.data?.configuredProtocol) {
      setProtocol(detection.data.configuredProtocol);
    }
  }, [detection.data?.configuredProtocol]);
  const planMutation = useMutation({
    mutationFn: (selectedProtocol: ExternalAgentGatewayProtocol) =>
      planOpenClawConfiguration(selectedProtocol),
    onSuccess: (nextPlan) => {
      setResultMessage(null);
      setPlan(nextPlan);
    },
  });
  const applyMutation = useMutation({
    mutationFn: (planId: string) => applyOpenClawConfiguration(planId),
    onSuccess: async (result) => {
      setPlan(null);
      setResultMessage(
        result.backupPath
          ? `OpenClaw 配置完成，备份已保存到 ${result.backupPath}`
          : "OpenClaw 配置完成，独立凭据已生效。",
      );
      await queryClient.invalidateQueries({ queryKey: ["openclaw-detection"] });
    },
  });
  const disconnectPlanMutation = useMutation({
    mutationFn: planOpenClawDisconnection,
    onSuccess: (nextPlan) => {
      setResultMessage(null);
      setDisconnectPlan(nextPlan);
    },
  });
  const disconnectMutation = useMutation({
    mutationFn: (planId: string) => applyOpenClawDisconnection(planId),
    onSuccess: async (result) => {
      setDisconnectPlan(null);
      setResultMessage(
        result.backupPath
          ? `OpenClaw 接入已断开，配置备份保存在 ${result.backupPath}`
          : "OpenClaw 接入已断开，专属凭据已吊销。",
      );
      await queryClient.invalidateQueries({ queryKey: ["openclaw-detection"] });
    },
  });

  if (detection.isPending) {
    return <section className="integration-card state-message">正在检测 OpenClaw…</section>;
  }
  if (detection.isError) {
    return (
      <section className="integration-card state-message error">
        {errorMessage(detection.error)}
      </section>
    );
  }

  const data = detection.data;
  const stateCopy = externalIntegrationStateCopy[data.integrationState];
  const connected =
    data.integrationState === "configured" || data.integrationState === "needsRefresh";
  const selectedProtocolAlreadyActive =
    data.integrationState === "configured" && data.configuredProtocol === protocol;
  const cannotConfigure =
    !data.installed ||
    data.integrationState === "conflict" ||
    data.integrationState === "modifiedOutsideHal100" ||
    data.integrationState === "unsupportedVersion" ||
    data.integrationState === "blocked";

  return (
    <>
      <IntegrationSummaryRow
        actionLabel={integrationRecommendedAction({
          connected,
          installed: data.installed,
          needsAttention:
            data.integrationState !== "notInstalled" &&
            data.integrationState !== "installedNotConfigured" &&
            data.integrationState !== "configured",
        })}
        description={
          data.installed
            ? "已检测到官方 OpenClaw CLI，可选择兼容协议接入"
            : "电脑上未发现官方 OpenClaw CLI"
        }
        descriptor={integrationRegistry.openClaw}
        onOpen={() => setDetailsOpen(true)}
        status={stateCopy}
      />
      {detailsOpen && (
        <Drawer
          description="选择 OpenClaw 使用的 Gateway 协议；HAL100 只管理自己的 Provider 分片和专属凭据。"
          eyebrow="软件接入"
          onClose={() => setDetailsOpen(false)}
          title="OpenClaw"
        >
          <section className="integration-drawer-summary">
            <span className={`status-pill ${stateCopy.tone}`}>{stateCopy.label}</span>
            <p>
              {data.installed
                ? `已检测到 ${data.version ?? "未知版本"}`
                : "未从常用安装位置检测到官方 OpenClaw CLI"}
            </p>
          </section>
          <details className="inline-disclosure">
            <summary>
              <span>连接详情</span>
              <ChevronRight size={14} />
            </summary>
            <dl className="integration-details">
              <div>
                <dt>OpenClaw 配置</dt>
                <dd>{data.configPath}</dd>
              </div>
              <div>
                <dt>当前协议</dt>
                <dd>
                  {data.configuredProtocol
                    ? externalAgentProtocolCopy[data.configuredProtocol]
                    : "尚未配置"}
                </dd>
              </div>
              <div>
                <dt>模型契约</dt>
                <dd>{data.modelProfileRevision}</dd>
              </div>
              <div>
                <dt>隔离边界</dt>
                <dd>{connected ? "OpenClaw 专属 Key · 文件型 SecretRef" : "配置后启用"}</dd>
              </div>
            </dl>
          </details>
          <label className="integration-protocol-selector">
            <span>Gateway 协议</span>
            <select
              disabled={planMutation.isPending || applyMutation.isPending}
              onChange={(event) => setProtocol(event.target.value as ExternalAgentGatewayProtocol)}
              value={protocol}
            >
              <option value="openAiChatCompletions">Chat Completions</option>
              <option value="openAiResponses">Responses</option>
              <option value="anthropicMessages">Anthropic Messages</option>
            </select>
            <small>切换协议只替换 HAL100 自己的 Provider 分片，不改变 OpenClaw 默认模型。</small>
          </label>
          {data.warnings.length > 0 && (
            <div className="warning-list">
              <AlertTriangle size={17} />
              <div>
                {data.warnings.map((warning) => (
                  <p key={warning}>{warning}</p>
                ))}
              </div>
            </div>
          )}
          {(planMutation.isError || disconnectPlanMutation.isError) && (
            <p className="inline-error">
              {errorMessage(planMutation.error ?? disconnectPlanMutation.error)}
            </p>
          )}
          {resultMessage && <p className="inline-success">{resultMessage}</p>}
          <div className="integration-actions">
            <button
              className="secondary-button"
              disabled={detection.isFetching}
              onClick={() => detection.refetch()}
              type="button"
            >
              {detection.isFetching ? "检测中…" : "重新检测"}
            </button>
            <button
              className="primary-button"
              disabled={cannotConfigure || planMutation.isPending || selectedProtocolAlreadyActive}
              onClick={() => planMutation.mutate(protocol)}
              type="button"
            >
              {planMutation.isPending
                ? "正在调用官方工具验证…"
                : selectedProtocolAlreadyActive
                  ? "所选协议已生效"
                  : connected
                    ? "切换 OpenClaw 协议"
                    : "配置 OpenClaw"}
            </button>
            {connected && (
              <button
                className="danger-button"
                disabled={disconnectPlanMutation.isPending}
                onClick={() => disconnectPlanMutation.mutate()}
                type="button"
              >
                {disconnectPlanMutation.isPending ? "正在生成预览…" : "断开接入"}
              </button>
            )}
          </div>
        </Drawer>
      )}
      {plan && (
        <ManagedAgentConfigurationDialog
          applying={applyMutation.isPending}
          displayName="OpenClaw"
          error={applyMutation.isError ? errorMessage(applyMutation.error) : null}
          onApply={() => applyMutation.mutate(plan.planId)}
          onCancel={() => {
            if (!applyMutation.isPending) {
              void discardOpenClawConfigurationPlan(plan.planId);
              setPlan(null);
            }
          }}
          plan={plan}
        />
      )}
      {disconnectPlan && (
        <ManagedAgentDisconnectDialog
          applying={disconnectMutation.isPending}
          displayName="OpenClaw"
          error={disconnectMutation.isError ? errorMessage(disconnectMutation.error) : null}
          onApply={() => disconnectMutation.mutate(disconnectPlan.planId)}
          onCancel={() => {
            if (!disconnectMutation.isPending) {
              void discardOpenClawDisconnectionPlan(disconnectPlan.planId);
              setDisconnectPlan(null);
            }
          }}
          plan={disconnectPlan}
        />
      )}
    </>
  );
}

function HermesAgentIntegrationCard() {
  const queryClient = useQueryClient();
  const detection = useQuery<ExternalAgentDetection>({
    queryKey: ["hermes-agent-detection"],
    queryFn: getHermesAgentDetection,
  });
  const [plan, setPlan] = useState<ExternalAgentConfigurationPlan | null>(null);
  const [disconnectPlan, setDisconnectPlan] = useState<ExternalAgentDisconnectPlan | null>(null);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [resultMessage, setResultMessage] = useState<string | null>(null);
  const planMutation = useMutation({
    mutationFn: planHermesAgentConfiguration,
    onSuccess: (nextPlan) => {
      setResultMessage(null);
      setPlan(nextPlan);
    },
  });
  const applyMutation = useMutation({
    mutationFn: (planId: string) => applyHermesAgentConfiguration(planId),
    onSuccess: async (result) => {
      setPlan(null);
      setResultMessage(
        result.backupPath
          ? `Hermes 配置完成，非敏感 YAML 备份已保存到 ${result.backupPath}`
          : "Hermes 配置完成，独立凭据已生效。",
      );
      await queryClient.invalidateQueries({ queryKey: ["hermes-agent-detection"] });
    },
  });
  const disconnectPlanMutation = useMutation({
    mutationFn: planHermesAgentDisconnection,
    onSuccess: (nextPlan) => {
      setResultMessage(null);
      setDisconnectPlan(nextPlan);
    },
  });
  const disconnectMutation = useMutation({
    mutationFn: (planId: string) => applyHermesAgentDisconnection(planId),
    onSuccess: async (result) => {
      setDisconnectPlan(null);
      setResultMessage(
        result.backupPath
          ? `Hermes 接入已断开，非敏感 YAML 备份保存在 ${result.backupPath}`
          : "Hermes 接入已断开，专属凭据已吊销。",
      );
      await queryClient.invalidateQueries({ queryKey: ["hermes-agent-detection"] });
    },
  });

  if (detection.isPending) {
    return <section className="integration-card state-message">正在检测 Hermes Agent…</section>;
  }
  if (detection.isError) {
    return (
      <section className="integration-card state-message error">
        {errorMessage(detection.error)}
      </section>
    );
  }

  const data = detection.data;
  const stateCopy = externalIntegrationStateCopy[data.integrationState];
  const connected =
    data.integrationState === "configured" || data.integrationState === "needsRefresh";
  const cannotConfigure =
    !data.installed ||
    data.integrationState === "conflict" ||
    data.integrationState === "modifiedOutsideHal100" ||
    data.integrationState === "unsupportedVersion" ||
    data.integrationState === "blocked";

  return (
    <>
      <IntegrationSummaryRow
        actionLabel={integrationRecommendedAction({
          connected,
          installed: data.installed,
          needsAttention:
            data.integrationState !== "notInstalled" &&
            data.integrationState !== "installedNotConfigured" &&
            data.integrationState !== "configured",
        })}
        description={
          data.installed
            ? "已检测到官方 Hermes CLI，可写入独立 Provider"
            : "电脑上未发现官方 Hermes CLI"
        }
        descriptor={integrationRegistry.hermesAgent}
        onOpen={() => setDetailsOpen(true)}
        status={stateCopy}
      />
      {detailsOpen && (
        <Drawer
          description="HAL100 只管理 Hermes default Profile 中的专属 Provider 和独立环境变量。"
          eyebrow="软件接入"
          onClose={() => setDetailsOpen(false)}
          title="Hermes Agent"
        >
          <section className="integration-drawer-summary">
            <span className={`status-pill ${stateCopy.tone}`}>{stateCopy.label}</span>
            <p>
              {data.installed
                ? `已检测到 ${data.version ?? "未知版本"}`
                : "未从常用安装位置检测到官方 Hermes CLI"}
            </p>
          </section>
          <details className="inline-disclosure">
            <summary>
              <span>连接详情</span>
              <ChevronRight size={14} />
            </summary>
            <dl className="integration-details">
              <div>
                <dt>Hermes default Profile</dt>
                <dd>{data.configPath}</dd>
              </div>
              <div>
                <dt>Gateway 协议</dt>
                <dd>Chat Completions</dd>
              </div>
              <div>
                <dt>运行前置条件</dt>
                <dd>Hermes ≥ 0.18.2 · 上下文 ≥ 64000 Token</dd>
              </div>
              <div>
                <dt>隔离边界</dt>
                <dd>
                  {connected
                    ? "Hermes 专属 Key · .env 独立变量"
                    : "只管理 providers.hal100 与专属变量"}
                </dd>
              </div>
            </dl>
          </details>
          {data.warnings.length > 0 && (
            <div className="warning-list">
              <AlertTriangle size={17} />
              <div>
                {data.warnings.map((warning) => (
                  <p key={warning}>{warning}</p>
                ))}
              </div>
            </div>
          )}
          {(planMutation.isError || disconnectPlanMutation.isError) && (
            <p className="inline-error">
              {errorMessage(planMutation.error ?? disconnectPlanMutation.error)}
            </p>
          )}
          {resultMessage && <p className="inline-success">{resultMessage}</p>}
          <div className="integration-actions">
            <button
              className="secondary-button"
              disabled={detection.isFetching}
              onClick={() => detection.refetch()}
              type="button"
            >
              {detection.isFetching ? "检测中…" : "重新检测"}
            </button>
            <button
              className="primary-button"
              disabled={
                cannotConfigure || planMutation.isPending || data.integrationState === "configured"
              }
              onClick={() => planMutation.mutate()}
              type="button"
            >
              {planMutation.isPending
                ? "正在调用官方 CLI 验证…"
                : data.integrationState === "configured"
                  ? "配置已生效"
                  : data.integrationState === "needsRefresh"
                    ? "刷新 Hermes 配置"
                    : "配置 Hermes"}
            </button>
            {connected && (
              <button
                className="danger-button"
                disabled={disconnectPlanMutation.isPending}
                onClick={() => disconnectPlanMutation.mutate()}
                type="button"
              >
                {disconnectPlanMutation.isPending ? "正在生成预览…" : "断开接入"}
              </button>
            )}
          </div>
        </Drawer>
      )}
      {plan && (
        <ManagedAgentConfigurationDialog
          applying={applyMutation.isPending}
          displayName="Hermes Agent"
          error={applyMutation.isError ? errorMessage(applyMutation.error) : null}
          onApply={() => applyMutation.mutate(plan.planId)}
          onCancel={() => {
            if (!applyMutation.isPending) {
              void discardHermesAgentConfigurationPlan(plan.planId);
              setPlan(null);
            }
          }}
          plan={plan}
        />
      )}
      {disconnectPlan && (
        <ManagedAgentDisconnectDialog
          applying={disconnectMutation.isPending}
          displayName="Hermes Agent"
          error={disconnectMutation.isError ? errorMessage(disconnectMutation.error) : null}
          onApply={() => disconnectMutation.mutate(disconnectPlan.planId)}
          onCancel={() => {
            if (!disconnectMutation.isPending) {
              void discardHermesAgentDisconnectionPlan(disconnectPlan.planId);
              setDisconnectPlan(null);
            }
          }}
          plan={disconnectPlan}
        />
      )}
    </>
  );
}

export function IntegrationsPage() {
  const queryClient = useQueryClient();
  const [boundaryOpen, setBoundaryOpen] = useState(false);
  const [openCodeDetailsOpen, setOpenCodeDetailsOpen] = useState(false);
  const [genericDetailsOpen, setGenericDetailsOpen] = useState(false);
  const ecosystem = useQuery({
    queryKey: ["agent-ecosystem-catalog"],
    queryFn: getAgentEcosystemCatalog,
    enabled: boundaryOpen,
  });
  const detection = useQuery({ queryKey: ["opencode-detection"], queryFn: getOpenCodeDetection });
  const [plan, setPlan] = useState<OpenCodeConfigPlan | null>(null);
  const [disconnectPlan, setDisconnectPlan] = useState<ExternalAgentDisconnectPlan | null>(null);
  const [resultMessage, setResultMessage] = useState<string | null>(null);
  const planMutation = useMutation({
    mutationFn: planOpenCodeConfiguration,
    onSuccess: (nextPlan) => {
      setResultMessage(null);
      setPlan(nextPlan);
    },
  });
  const applyMutation = useMutation({
    mutationFn: (planId: string) => applyOpenCodeConfiguration(planId),
    onSuccess: async (result) => {
      setPlan(null);
      setResultMessage(
        result.backupPath
          ? `配置完成，备份已保存到 ${result.backupPath}`
          : "配置完成，OpenCode 专属凭据已生效。",
      );
      await queryClient.invalidateQueries({ queryKey: ["opencode-detection"] });
    },
  });
  const disconnectPlanMutation = useMutation({
    mutationFn: planOpenCodeDisconnection,
    onSuccess: (nextPlan) => {
      setResultMessage(null);
      setDisconnectPlan(nextPlan);
    },
  });
  const disconnectMutation = useMutation({
    mutationFn: (planId: string) => applyOpenCodeDisconnection(planId),
    onSuccess: async (result) => {
      setDisconnectPlan(null);
      setResultMessage(
        result.backupPath
          ? `接入已断开，配置备份保存在 ${result.backupPath}`
          : "接入已断开，OpenCode 专属凭据已吊销。",
      );
      await queryClient.invalidateQueries({ queryKey: ["opencode-detection"] });
    },
  });

  if (detection.isPending) {
    return <div className="state-message">正在读取 Agent 接入边界并检测外部客户端…</div>;
  }
  if (detection.isError) {
    return <div className="state-message error">{errorMessage(detection.error)}</div>;
  }
  const data = detection.data;
  const stateCopy = integrationStateCopy[data.integrationState];
  const cannotPlan =
    data.integrationState === "conflict" || data.integrationState === "modifiedOutsideHal100";

  return (
    <div className="page-content integrations-page">
      <PageHeader
        action={
          <div className="page-header-actions">
            <button
              className="secondary-button"
              onClick={() => setBoundaryOpen(true)}
              type="button"
            >
              了解运行边界
            </button>
            <button
              className="secondary-button refresh-button"
              disabled={detection.isFetching}
              onClick={() => {
                detection.refetch();
                void queryClient.invalidateQueries({ queryKey: ["pi-coding-agent-detection"] });
                void queryClient.invalidateQueries({ queryKey: ["openclaw-detection"] });
                void queryClient.invalidateQueries({ queryKey: ["hermes-agent-detection"] });
                void queryClient.invalidateQueries({ queryKey: ["generic-client-catalog"] });
              }}
              type="button"
            >
              <RefreshCw className={detection.isFetching ? "spinning" : ""} size={14} />
              {detection.isFetching ? "检测中…" : "重新检测全部"}
            </button>
          </div>
        }
        description="统一管理外部 Agent 与其他兼容客户端，每个软件保持独立身份和配置。"
        eyebrow="客户端接入"
        title="软件接入"
      />

      <div className="integration-summary-list">
        <IntegrationSummaryRow
          actionLabel={
            cannotPlan
              ? "解决问题"
              : data.integrationState === "configured"
                ? "查看详情"
                : "配置接入"
          }
          description={
            data.installed
              ? "已检测到 OpenCode CLI，可创建专属 Gateway 身份"
              : "电脑上未发现 OpenCode CLI"
          }
          descriptor={integrationRegistry.openCode}
          onOpen={() => setOpenCodeDetailsOpen(true)}
          status={stateCopy}
        />
        {openCodeDetailsOpen && (
          <Drawer
            description="HAL100 只管理 OpenCode 的专属 Provider 和凭据，不修改默认模型或其他 Provider。"
            eyebrow="软件接入"
            onClose={() => setOpenCodeDetailsOpen(false)}
            title="OpenCode"
          >
            <section className="integration-drawer-summary">
              <span className={`status-pill ${stateCopy.tone}`}>{stateCopy.label}</span>
              <p>
                {data.installed
                  ? `已检测到 ${data.version ?? "未知版本"}`
                  : "未从常用安装位置检测到 CLI"}
              </p>
            </section>
            <details className="inline-disclosure">
              <summary>
                <span>连接详情</span>
                <ChevronRight size={14} />
              </summary>
              <dl className="integration-details">
                <div>
                  <dt>Gateway Base URL</dt>
                  <dd>http://127.0.0.1:10100/v1</dd>
                </div>
                <div>
                  <dt>全局配置</dt>
                  <dd>{data.configPath}</dd>
                </div>
                <div>
                  <dt>配置格式</dt>
                  <dd>{data.configFormat.toUpperCase()}</dd>
                </div>
                <div>
                  <dt>用量归属</dt>
                  <dd>
                    {data.integrationState === "configured" ? "OpenCode 专属 Key" : "配置后启用"}
                  </dd>
                </div>
              </dl>
            </details>

            {data.warnings.length > 0 && (
              <div className="warning-list">
                <AlertTriangle size={17} />
                <div>
                  {data.warnings.map((warning) => (
                    <p key={warning}>{warning}</p>
                  ))}
                </div>
              </div>
            )}
            {(planMutation.isError || disconnectPlanMutation.isError) && (
              <p className="inline-error">
                {errorMessage(planMutation.error ?? disconnectPlanMutation.error)}
              </p>
            )}
            {resultMessage && <p className="inline-success">{resultMessage}</p>}

            <div className="integration-actions">
              <button
                className="secondary-button"
                disabled={detection.isFetching}
                onClick={() => detection.refetch()}
                type="button"
              >
                {detection.isFetching ? "检测中…" : "重新检测"}
              </button>
              <button
                className="primary-button"
                disabled={
                  cannotPlan || planMutation.isPending || data.integrationState === "configured"
                }
                onClick={() => planMutation.mutate()}
                type="button"
              >
                {planMutation.isPending
                  ? "正在生成预览…"
                  : data.integrationState === "configured"
                    ? "配置已生效"
                    : "配置 OpenCode"}
              </button>
              {data.integrationState === "configured" && (
                <button
                  className="danger-button"
                  disabled={disconnectPlanMutation.isPending}
                  onClick={() => disconnectPlanMutation.mutate()}
                  type="button"
                >
                  {disconnectPlanMutation.isPending ? "正在生成预览…" : "断开接入"}
                </button>
              )}
            </div>
          </Drawer>
        )}

        <PiCodingAgentIntegrationCard />

        <OpenClawIntegrationCard />

        <HermesAgentIntegrationCard />

        <IntegrationSummaryRow
          actionLabel="管理客户端"
          description="为其他 OpenAI / Anthropic 兼容软件创建独立 Key"
          descriptor={integrationRegistry.otherClients}
          onOpen={() => setGenericDetailsOpen(true)}
          status={{ label: "按需配置", tone: "neutral" }}
        />
      </div>

      {genericDetailsOpen && (
        <Drawer
          description="每个客户端使用独立 Key 和用量归属；Secret 只在创建时显示一次。"
          eyebrow="通用接入"
          onClose={() => setGenericDetailsOpen(false)}
          title="其他客户端"
        >
          <GenericClientAccess />
        </Drawer>
      )}

      {boundaryOpen && (
        <Drawer
          description="HAL100 内置 Runtime 与用户安装的外部 Agent 互不覆盖。"
          eyebrow="运行边界"
          onClose={() => setBoundaryOpen(false)}
          title="内置与外部相互独立"
        >
          {ecosystem.isPending && <div className="state-message">正在读取运行边界…</div>}
          {ecosystem.isError && (
            <div className="state-message error">{errorMessage(ecosystem.error)}</div>
          )}
          {ecosystem.data && (
            <div className="agent-boundary-grid integration-boundary-drawer">
              <article>
                <span className="boundary-kind">HAL100 私有组件</span>
                <strong>{ecosystem.data.builtInRuntime.displayName}（内置）</strong>
                <p>
                  底层使用固定版本 {ecosystem.data.builtInRuntime.engineName}；
                  {ecosystem.data.builtInRuntime.isolationSummary}。
                </p>
                <code>{ecosystem.data.builtInRuntime.clientAppId}</code>
              </article>
              <article>
                <span className="boundary-kind">用户安装的软件</span>
                <strong>外部 Agent 集成</strong>
                <p>独立安装、配置、会话和升级；HAL100 只管理预览过的配置片段和专属 Key。</p>
                <code>opencode · pi-coding-agent · openclaw · hermes-agent</code>
              </article>
            </div>
          )}
        </Drawer>
      )}

      {plan && (
        <ManagedAgentConfigurationDialog
          applying={applyMutation.isPending}
          displayName="OpenCode"
          error={applyMutation.isError ? errorMessage(applyMutation.error) : null}
          onApply={() => applyMutation.mutate(plan.planId)}
          onCancel={() => {
            if (!applyMutation.isPending) {
              void discardOpenCodeConfigurationPlan(plan.planId);
              setPlan(null);
            }
          }}
          plan={plan}
        />
      )}
      {disconnectPlan && (
        <ManagedAgentDisconnectDialog
          applying={disconnectMutation.isPending}
          displayName="OpenCode"
          error={disconnectMutation.isError ? errorMessage(disconnectMutation.error) : null}
          onApply={() => disconnectMutation.mutate(disconnectPlan.planId)}
          onCancel={() => {
            if (!disconnectMutation.isPending) {
              void discardOpenCodeDisconnectionPlan(disconnectPlan.planId);
              setDisconnectPlan(null);
            }
          }}
          plan={disconnectPlan}
        />
      )}
    </div>
  );
}
