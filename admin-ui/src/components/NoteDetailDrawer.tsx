import type { NoteDetail, NoteSnapshot } from "../lib/types";

type NoteDetailDrawerProps = {
  note: NoteDetail | null;
  history: NoteSnapshot[];
  onClose: () => void;
  onDownload: (noteId: string) => void;
};

export function NoteDetailDrawer({
  note,
  history,
  onClose,
  onDownload,
}: NoteDetailDrawerProps) {
  if (!note) {
    return null;
  }

  return (
    <aside className="detail-drawer" aria-label="笔记详情">
      <div className="drawer-header">
        <div>
          <p className="section-label">只读详情</p>
          <h3>{note.title}</h3>
        </div>
        <button type="button" className="ghost-button" onClick={onClose}>
          关闭
        </button>
      </div>

      <div className="drawer-actions">
        <button type="button" onClick={() => onDownload(note.id)}>
          下载 Markdown
        </button>
      </div>

      <dl className="metadata-grid">
        <div>
          <dt>分类</dt>
          <dd>{note.category || "未分类"}</dd>
        </div>
        <div>
          <dt>设备</dt>
          <dd>{note.deviceId}</dd>
        </div>
        <div>
          <dt>修订号</dt>
          <dd>{note.revision}</dd>
        </div>
        <div>
          <dt>更新时间</dt>
          <dd>{note.updatedAt}</dd>
        </div>
      </dl>

      <section className="drawer-panel">
        <div className="panel-heading">
          <h4>正文</h4>
        </div>
        <pre className="note-content">{note.content}</pre>
      </section>

      <section className="drawer-panel">
        <div className="panel-heading">
          <h4>版本历史</h4>
        </div>
        <ul className="history-list">
          {history.map((entry) => (
            <li key={entry.snapshotId}>
              <strong>r{entry.revision}</strong>
              <span>{entry.capturedAt}</span>
            </li>
          ))}
        </ul>
      </section>
    </aside>
  );
}
