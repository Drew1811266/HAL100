import { CircleGauge, RefreshCw } from "lucide-react";
import type { EnvironmentDiagnosticReport } from "../../lib/desktop-api";

const statusCopy: Record<EnvironmentDiagnosticReport["status"], { label: string; tone: string }> = {
  healthy: { label: "环境健康", tone: "ok" },
  needsAttention: { label: "需要关注", tone: "warning" },
  error: { label: "存在错误", tone: "danger" },
};

const componentCopy: Record<EnvironmentDiagnosticReport["findings"][number]["component"], string> =
  {
    gateway: "Gateway",
    inferenceEngine: "推理引擎",
    modelLibrary: "模型库",
    openCode: "OpenCode",
  };

interface EnvironmentDiagnosticsPanelProps {
  report: EnvironmentDiagnosticReport | undefined;
  error: unknown;
  isFetching: boolean;
  disabled: boolean;
  onRefresh: () => void;
}

export function EnvironmentDiagnosticsPanel({
  report,
  error,
  isFetching,
  disabled,
  onRefresh,
}: EnvironmentDiagnosticsPanelProps) {
  if (!report && !error) return null;

  return (
    <section className="agent-diagnostics" aria-label="环境诊断">
      <div className="agent-diagnostics-heading">
        <div>
          <span className="agent-status-icon">
            <CircleGauge size={18} />
          </span>
          <div>
            <p className="eyebrow">Rust 快照</p>
            <h2>环境诊断</h2>
          </div>
        </div>
        <button
          className="secondary-button"
          disabled={disabled || isFetching}
          onClick={onRefresh}
          type="button"
        >
          <RefreshCw className={isFetching ? "spinning" : ""} size={13} />
          {isFetching ? "诊断中…" : "立即诊断"}
        </button>
      </div>
      {report ? (
        <>
          <div className="agent-diagnostic-summary">
            <div>
              <span>总体状态</span>
              <strong className={`status-pill ${statusCopy[report.status].tone}`}>
                {statusCopy[report.status].label}
              </strong>
            </div>
            <div>
              <span>模型</span>
              <strong>
                {report.readyModelCount} 就绪 · {report.unhealthyModelCount} 异常
              </strong>
            </div>
            <div>
              <span>后端</span>
              <strong>{report.configuredBackendCount} 个已配置</strong>
            </div>
            <div>
              <span>问题</span>
              <strong>
                {report.errorCount} 错误 · {report.warningCount} 警告
              </strong>
            </div>
          </div>
          {report.findings.length > 0 ? (
            <div className="agent-diagnostic-findings">
              {report.findings.map((finding) => (
                <article className={`severity-${finding.severity}`} key={finding.findingId}>
                  <span>{componentCopy[finding.component]}</span>
                  <div>
                    <strong>{finding.title}</strong>
                    <small>{finding.summary}</small>
                  </div>
                  {finding.repairKind && <em>可生成修复计划</em>}
                </article>
              ))}
            </div>
          ) : (
            <p className="agent-diagnostic-empty">未发现需要说明的问题。</p>
          )}
          <small className="agent-diagnostic-meta">
            {new Date(report.generatedAtMs).toLocaleTimeString("zh-CN")} 按需生成 ·
            不读取原始日志，不做完整模型哈希，不后台轮询
          </small>
        </>
      ) : (
        <p className="inline-error">{errorMessage(error)}</p>
      )}
    </section>
  );
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
