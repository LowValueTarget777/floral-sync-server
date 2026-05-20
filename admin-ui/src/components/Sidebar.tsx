import type { SessionResponse } from "../lib/types";

export type AdminView = "overview" | "notes" | "settings" | "maintenance";

type SidebarProps = {
  activeView: AdminView;
  session: SessionResponse | null;
  latestRevision?: number | null;
  onSelectView: (view: AdminView) => void;
};

const NAV_ITEMS: Array<{ view: AdminView; label: string; description: string }> = [
  { view: "overview", label: "概览", description: "服务状态与最近活动" },
  { view: "notes", label: "笔记", description: "只读浏览与版本历史" },
  { view: "settings", label: "设置", description: "监听、路径与凭据管理" },
  { view: "maintenance", label: "维护", description: "备份与日志巡检" },
];

export function Sidebar({
  activeView,
  session,
  latestRevision,
  onSelectView,
}: SidebarProps) {
  return (
    <aside className="sidebar-shell">
      <div className="sidebar-brand">
        <p className="eyebrow">Floral Sync Admin</p>
        <h1>同步服务管理台</h1>
        <p className="muted">
          一个偏运维视角的只读后台，用来查看同步状态、笔记快照、配置与维护信息。
        </p>
      </div>

      <nav className="sidebar-nav" aria-label="管理导航">
        {NAV_ITEMS.map((item) => (
          <button
            key={item.view}
            type="button"
            className={item.view === activeView ? "nav-button active" : "nav-button"}
            onClick={() => onSelectView(item.view)}
          >
            <span>{item.label}</span>
            <small>{item.description}</small>
          </button>
        ))}
      </nav>

      <div className="sidebar-meta">
        <div className="meta-item">
          <span>会话状态</span>
          <strong>{session?.authenticated ? "已登录" : "待登录"}</strong>
        </div>
        <div className="meta-item">
          <span>最新 revision</span>
          <strong>{latestRevision ?? "--"}</strong>
        </div>
      </div>
    </aside>
  );
}
