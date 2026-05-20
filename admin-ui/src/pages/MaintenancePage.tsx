import type { BackupEntry, LogsResponse } from "../lib/types";

type MaintenancePageProps = {
  busy: boolean;
  backups: BackupEntry[];
  logs: LogsResponse | null;
  onCreateBackup: () => void;
  onRestoreBackup: (fileName: string) => void;
  onRefreshLogs: () => void;
};

export function MaintenancePage({
  busy,
  backups,
  logs,
  onCreateBackup,
  onRestoreBackup,
  onRefreshLogs,
}: MaintenancePageProps) {
  return (
    <section className="page-shell">
      <header className="page-header">
        <div>
          <p className="section-label">维护</p>
          <h2>备份与日志</h2>
          <p className="muted">面向日常巡检：创建 SQLite 备份、查看最近日志片段。</p>
        </div>
        <div className="actions-inline">
          <button type="button" onClick={onCreateBackup} disabled={busy}>
            创建备份
          </button>
          <button type="button" className="secondary" onClick={onRefreshLogs} disabled={busy}>
            刷新日志
          </button>
        </div>
      </header>

      <div className="maintenance-grid">
        <section className="panel stack">
          <div className="panel-heading">
            <h3>备份文件</h3>
          </div>
          <ul className="simple-list">
            {backups.length === 0 ? (
              <li className="empty-item">暂无备份。</li>
            ) : (
              backups.map((entry) => (
                <li key={entry.fileName}>
                  <div>
                    <strong>{entry.fileName}</strong>
                    <span>{entry.sizeBytes} bytes</span>
                  </div>
                  <button
                    type="button"
                    className="secondary"
                    onClick={() => onRestoreBackup(entry.fileName)}
                    disabled={busy}
                    aria-label={`恢复备份 ${entry.fileName}`}
                  >
                    恢复
                  </button>
                </li>
              ))
            )}
          </ul>
        </section>

        <section className="panel stack">
          <div className="panel-heading">
            <h3>最近日志</h3>
          </div>
          <p className="muted">{logs?.path ?? "尚未读取日志文件。"}</p>
          <pre className="log-view">{logs?.lines.join("\n") || "暂无日志内容。"}</pre>
        </section>
      </div>
    </section>
  );
}
