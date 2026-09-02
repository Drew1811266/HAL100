import { NavLink } from "react-router-dom";

const workspaceTabs = [
  { label: "模型库", path: "/workspace/models" },
  { label: "本地运行", path: "/workspace/runtime" },
  { label: "连接服务", path: "/workspace/services" },
  { label: "快捷切换", path: "/workspace/profiles" },
];

export function WorkspaceTabs() {
  return (
    <nav aria-label="模型与运行" className="section-tabs">
      {workspaceTabs.map((tab) => (
        <NavLink
          className={({ isActive }) => (isActive ? "active" : undefined)}
          key={tab.path}
          to={tab.path}
        >
          {tab.label}
        </NavLink>
      ))}
    </nav>
  );
}
