import { StatCard } from "../components/StatCard";
import type { OverviewResponse } from "../lib/types";

type OverviewPageProps = {
  overview: OverviewResponse | null;
  busy: boolean;
  onRefresh: () => void;
};

export function OverviewPage({ overview, busy, onRefresh }: OverviewPageProps) {
  if (!overview) {
    return (
      <section className="page-shell">
        <header className="page-header">
          <div>
            <p className="section-label">概览</p>
            <h2>服务总览</h2>
          </div>
          <button type="button" onClick={onRefresh} disabled={busy}>
            刷新概览
          </button>
        </header>

        <div className="empty-panel">
          <p>还没有拿到服务端概览数据。</p>
        </div>
      </section>
    );
  }

  return (
    <section className="page-shell">
      <header className="page-header">
        <div>
          <p className="section-label">概览</p>
          <h2>服务总览</h2>
          <p className="muted">聚焦当前 revision、笔记规模和监听配置。</p>
        </div>
        <button type="button" onClick={onRefresh} disabled={busy}>
          刷新概览
        </button>
      </header>

      <div className="stats-grid">
        <StatCard label="最新 revision" value={overview.latestRevision} detail="服务端当前的最新变更序号。" />
        <StatCard label="笔记总数" value={overview.noteCount} detail="当前保存在服务端的活动笔记数量。" />
        <StatCard label="已删除" value={overview.deletedNoteCount} detail="包含 tombstone 的删除记录数量。" />
        <StatCard label="分类数量" value={overview.categoryCount} detail="服务端笔记中去重后的分类数量。" />
      </div>

      <div className="info-grid">
        <article className="panel stack">
          <div className="panel-heading">
            <h3>最近活动</h3>
          </div>
          <p>{overview.recentActivitySummary}</p>
          <p className="muted">最近快照时间：{overview.latestSnapshotAt ?? "暂无"}</p>
        </article>

        <article className="panel stack">
          <div className="panel-heading">
            <h3>监听与路径</h3>
          </div>
          <dl className="metadata-grid compact">
            <div>
              <dt>同步地址</dt>
              <dd>{overview.syncListen.join(" / ")}</dd>
            </div>
            <div>
              <dt>管理地址</dt>
              <dd>{overview.adminListen.join(" / ")}</dd>
            </div>
            <div>
              <dt>数据库</dt>
              <dd>{overview.dbPath}</dd>
            </div>
            <div>
              <dt>导出目录</dt>
              <dd>{overview.exportDir}</dd>
            </div>
          </dl>
        </article>
      </div>
    </section>
  );
}
