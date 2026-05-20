import { useEffect, useMemo, useState } from "react";

import { NoteDetailDrawer } from "../components/NoteDetailDrawer";
import type {
  NoteDetail,
  NoteSnapshot,
  NoteStateFilter,
  NotesPageResponse,
} from "../lib/types";

type NotesQueryState = {
  search: string;
  category: string;
  state: NoteStateFilter;
};

type NotesPageProps = {
  busy: boolean;
  query: NotesQueryState;
  categories: string[];
  notesPage: NotesPageResponse | null;
  selectedNote: NoteDetail | null;
  selectedHistory: NoteSnapshot[];
  onQueryChange: (query: NotesQueryState) => void;
  onSelectNote: (noteId: string) => void;
  onCloseDetail: () => void;
  onDownloadNote: (noteId: string) => void;
  onDownloadSelected: (noteIds: string[]) => void;
};

export function NotesPage({
  busy,
  query,
  categories,
  notesPage,
  selectedNote,
  selectedHistory,
  onQueryChange,
  onSelectNote,
  onCloseDetail,
  onDownloadNote,
  onDownloadSelected,
}: NotesPageProps) {
  const notes = notesPage?.notes ?? [];
  const noteIds = useMemo(() => notes.map((note) => note.id), [notes]);
  const [selectedNoteIds, setSelectedNoteIds] = useState<string[]>([]);

  useEffect(() => {
    const currentIds = new Set(noteIds);
    setSelectedNoteIds((existing) => existing.filter((id) => currentIds.has(id)));
  }, [noteIds]);

  const allSelected = noteIds.length > 0 && noteIds.every((id) => selectedNoteIds.includes(id));
  const selectionStatus =
    selectedNoteIds.length > 0
      ? `已选 ${selectedNoteIds.length} 条`
      : busy
        ? "正在加载…"
        : "点击行查看详情";

  function toggleNoteSelection(noteId: string) {
    setSelectedNoteIds((existing) =>
      existing.includes(noteId)
        ? existing.filter((id) => id !== noteId)
        : [...existing, noteId],
    );
  }

  function toggleAllNotes() {
    setSelectedNoteIds(allSelected ? [] : noteIds);
  }

  return (
    <section className="page-shell notes-layout">
      <header className="page-header">
        <div>
          <p className="section-label">笔记</p>
          <h2>只读备份浏览</h2>
          <p className="muted">支持关键词、分类和删除状态筛选，详情面板只读不可改，并可批量导出 ZIP。</p>
        </div>
      </header>

      <div className="panel filters-row">
        <label className="field compact">
          <span>搜索</span>
          <input
            aria-label="搜索"
            value={query.search}
            onChange={(event) =>
              onQueryChange({ ...query, search: event.target.value })
            }
            placeholder="标题或正文关键词"
          />
        </label>
        <label className="field compact">
          <span>分类</span>
          <select
            aria-label="分类"
            value={query.category}
            onChange={(event) =>
              onQueryChange({ ...query, category: event.target.value })
            }
          >
            <option value="">全部分类</option>
            {categories.map((category) => (
              <option key={category} value={category}>
                {category}
              </option>
            ))}
          </select>
        </label>
        <label className="field compact">
          <span>状态</span>
          <select
            aria-label="状态"
            value={query.state}
            onChange={(event) =>
              onQueryChange({
                ...query,
                state: event.target.value as NoteStateFilter,
              })
            }
          >
            <option value="all">全部</option>
            <option value="active">活动</option>
            <option value="deleted">已删除</option>
          </select>
        </label>
      </div>

      <div className="notes-surface">
        <div className="panel table-shell">
          <div className="table-meta">
            <span>共 {notesPage?.total ?? 0} 条</span>
            <div className="actions-inline table-actions">
              <span>{selectionStatus}</span>
              <button
                type="button"
                className="secondary"
                onClick={() => onDownloadSelected(selectedNoteIds)}
                disabled={busy || selectedNoteIds.length === 0}
              >
                下载选中 ZIP
              </button>
            </div>
          </div>
          <table className="notes-table">
            <thead>
              <tr>
                <th className="selection-column">
                  <input
                    type="checkbox"
                    className="selection-toggle"
                    aria-label="全选当前页笔记"
                    checked={allSelected}
                    disabled={busy || noteIds.length === 0}
                    onChange={toggleAllNotes}
                    onClick={(event) => event.stopPropagation()}
                  />
                </th>
                <th>标题</th>
                <th>分类</th>
                <th>更新时间</th>
                <th>设备</th>
              </tr>
            </thead>
            <tbody>
              {notes.length === 0 ? (
                <tr>
                  <td colSpan={5} className="empty-cell">
                    没有符合条件的笔记。
                  </td>
                </tr>
              ) : (
                notes.map((note) => (
                  <tr
                    key={note.id}
                    className={`table-row${selectedNoteIds.includes(note.id) ? " selected" : ""}`}
                    onClick={() => onSelectNote(note.id)}
                  >
                    <td className="selection-cell">
                      <input
                        type="checkbox"
                        className="selection-toggle"
                        aria-label={`选择笔记 ${note.title || note.id}`}
                        checked={selectedNoteIds.includes(note.id)}
                        disabled={busy}
                        onChange={() => toggleNoteSelection(note.id)}
                        onClick={(event) => event.stopPropagation()}
                      />
                    </td>
                    <td>
                      <strong>{note.title}</strong>
                      {note.deletedAt ? (
                        <span className="badge-inline">已删除</span>
                      ) : null}
                    </td>
                    <td>{note.category || "未分类"}</td>
                    <td>{note.updatedAt}</td>
                    <td>{note.deviceId}</td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>

        <NoteDetailDrawer
          note={selectedNote}
          history={selectedHistory}
          onClose={onCloseDetail}
          onDownload={onDownloadNote}
        />
      </div>
    </section>
  );
}
