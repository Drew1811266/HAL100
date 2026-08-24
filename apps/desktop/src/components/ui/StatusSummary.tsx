import { AlertTriangle, ShieldCheck } from "lucide-react";
import type { OverviewStatusView } from "../../presentation/status";
import { DetailDrawer } from "./DetailDrawer";

export function StatusSummary({ status }: { status: OverviewStatusView }) {
  return (
    <section className={`overview-summary ${status.status}`} aria-label="当前状态">
      <div className="overview-summary-heading">
        <span className="overview-summary-icon">
          {status.status === "ready" ? <ShieldCheck size={20} /> : <AlertTriangle size={20} />}
        </span>
        <div>
          <span className={`status-pill ${status.status}`}>{status.label}</span>
          <h2>{status.title}</h2>
          <p>{status.description}</p>
        </div>
      </div>
      <DetailDrawer className="overview-details" summary="查看系统详情">
        <dl>
          {status.details.map((detail) => (
            <div key={detail.label}>
              <dt>{detail.label}</dt>
              <dd>{detail.value}</dd>
            </div>
          ))}
        </dl>
      </DetailDrawer>
    </section>
  );
}
