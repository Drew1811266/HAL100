import { useQuery } from "@tanstack/react-query";
import { ChevronRight } from "lucide-react";
import { Link } from "react-router-dom";
import { PageHeader } from "../../components/ui/PageHeader";
import { StatusSummary } from "../../components/ui/StatusSummary";
import { getAppOverview, getLlamaCppStatus, getModelLibrary } from "../../lib/desktop-api";
import { buildOverviewStatus } from "../../presentation/status";

export function OverviewPage({ setupRequired }: { setupRequired: boolean }) {
  const overview = useQuery({ queryKey: ["app-overview"], queryFn: getAppOverview });
  const models = useQuery({ queryKey: ["model-library"], queryFn: getModelLibrary });
  const runtime = useQuery({ queryKey: ["llama-cpp-status"], queryFn: getLlamaCppStatus });

  if (overview.isPending || models.isPending || runtime.isPending) {
    return <div className="state-message">正在连接 HAL100 Core…</div>;
  }

  if (overview.isError) {
    return <div className="state-message error">无法读取后台核心状态。</div>;
  }

  const data = overview.data;
  const status = buildOverviewStatus(data, setupRequired, {
    engineInstalled: runtime.data ? runtime.data.installState === "installed" : null,
    readyModelCount: models.data
      ? models.data.models.filter((model) => model.state === "ready").length
      : null,
  });
  const secondaryLinks = [
    { label: "管理模型", path: "/workspace/models" },
    { label: "查看运行", path: "/workspace/runtime" },
    { label: "连接软件", path: "/integrations" },
  ]
    .filter((link) => link.path !== status.actionPath)
    .slice(0, 2);
  return (
    <div className="page-content overview-page">
      <PageHeader
        description="优先显示当前状态与下一步建议，其余技术信息按需展开。"
        eyebrow="首页"
        title="今天需要处理什么"
      />

      <StatusSummary
        action={
          <Link className="primary-button" to={status.actionPath}>
            {status.actionLabel}
            <ChevronRight size={14} />
          </Link>
        }
        secondaryActions={secondaryLinks.map((link) => (
          <Link key={link.path} to={link.path}>
            {link.label}
          </Link>
        ))}
        status={status}
      />
    </div>
  );
}
