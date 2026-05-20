import { useEffect, useMemo, useState } from "react";
import { createAdminApiClient, type AdminApiClient, type AdminApiError } from "./lib/api";
import { Sidebar, type AdminView } from "./components/Sidebar";
import type {
  BackupEntry,
  LogsResponse,
  NoteDetail,
  NoteSnapshot,
  NoteStateFilter,
  NotesPageResponse,
  OverviewResponse,
  PasswordChangeRequest,
  SessionResponse,
  SettingsResponse,
  SettingsUpdateRequest,
} from "./lib/types";
import { LoginPage } from "./pages/LoginPage";
import { MaintenancePage } from "./pages/MaintenancePage";
import { NotesPage } from "./pages/NotesPage";
import { OverviewPage } from "./pages/OverviewPage";
import { SettingsPage } from "./pages/SettingsPage";

const NOTES_AUTO_REFRESH_MS = 5000;

type AppProps = {
  client?: AdminApiClient;
  notesAutoRefreshMs?: number;
};

type NotesQueryState = {
  search: string;
  category: string;
  state: NoteStateFilter;
};

type LoadOptions = {
  background?: boolean;
  suppressError?: boolean;
};

const defaultClient = createAdminApiClient();
const defaultNotesQuery: NotesQueryState = {
  search: "",
  category: "",
  state: "all",
};

export function App({
  client = defaultClient,
  notesAutoRefreshMs = NOTES_AUTO_REFRESH_MS,
}: AppProps = {}) {
  const [session, setSession] = useState<SessionResponse | null>(null);
  const [activeView, setActiveView] = useState<AdminView>("overview");
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const [overview, setOverview] = useState<OverviewResponse | null>(null);
  const [notesQuery, setNotesQuery] = useState<NotesQueryState>(defaultNotesQuery);
  const [notesPage, setNotesPage] = useState<NotesPageResponse | null>(null);
  const [selectedNote, setSelectedNote] = useState<NoteDetail | null>(null);
  const [selectedHistory, setSelectedHistory] = useState<NoteSnapshot[]>([]);
  const [settings, setSettings] = useState<SettingsResponse | null>(null);
  const [backups, setBackups] = useState<BackupEntry[] | null>(null);
  const [logs, setLogs] = useState<LogsResponse | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const authMode = useMemo(() => {
    if (!session) {
      return "checking" as const;
    }
    return session.bootstrapRequired ? ("bootstrap" as const) : ("login" as const);
  }, [session]);

  const categories = useMemo(() => {
    return Array.from(
      new Set(
        (notesPage?.notes ?? [])
          .map((note) => note.category)
          .filter((category) => category.trim().length > 0),
      ),
    ).sort((left, right) => left.localeCompare(right, "zh-CN"));
  }, [notesPage]);

  useEffect(() => {
    void restoreSession();
  }, [client]);

  useEffect(() => {
    if (!session?.authenticated || activeView !== "overview" || overview) {
      return;
    }

    void loadOverview();
  }, [activeView, overview, session]);

  useEffect(() => {
    if (!session?.authenticated || activeView !== "notes") {
      return;
    }

    void loadNotes(notesQuery);
  }, [activeView, notesQuery, session]);

  useEffect(() => {
    if (!session?.authenticated || activeView !== "settings" || settings) {
      return;
    }

    void loadSettings();
  }, [activeView, session, settings]);

  useEffect(() => {
    if (!session?.authenticated || activeView !== "maintenance") {
      return;
    }

    if (!logs || backups === null) {
      void loadMaintenance();
    }
  }, [activeView, backups, logs, session]);

  useEffect(() => {
    if (!session?.authenticated || activeView !== "notes") {
      return;
    }

    const timer = window.setInterval(() => {
      void loadNotes(notesQuery, { background: true, suppressError: true });
      if (selectedNote) {
        void loadNoteDetail(selectedNote.id, { background: true, suppressError: true });
      }
    }, notesAutoRefreshMs);

    return () => {
      window.clearInterval(timer);
    };
  }, [activeView, notesAutoRefreshMs, notesQuery, selectedNote, session]);

  async function restoreSession() {
    setBusy(true);
    try {
      const nextSession = await client.getSession();
      setSession(nextSession);
      setError(null);
    } catch (loadError) {
      setSession(null);
      setError(
        loadError instanceof Error ? loadError.message : "无法连接到管理接口",
      );
    } finally {
      setBusy(false);
    }
  }

  async function handleAuthSubmit(password: string) {
    if (!password.trim()) {
      return;
    }

    setBusy(true);
    setError(null);
    try {
      const nextSession =
        authMode === "bootstrap"
          ? await client.bootstrap({ password })
          : await client.login({ password });
      setSession(nextSession);
      setActiveView("overview");
    } catch (submitError) {
      const apiError = submitError as AdminApiError;
      setError(apiError.message || "操作失败");
    } finally {
      setBusy(false);
    }
  }

  async function handleLogout() {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const nextSession = await client.logout();
      setSession(nextSession);
      clearAuthenticatedState();
    } catch (logoutError) {
      setError(logoutError instanceof Error ? logoutError.message : "退出失败");
    } finally {
      setBusy(false);
    }
  }

  async function loadOverview(options: LoadOptions = {}) {
    if (!options.background) {
      setBusy(true);
      setError(null);
    }
    try {
      setOverview(await client.getOverview());
    } catch (overviewError) {
      if (!options.suppressError) {
        setError(overviewError instanceof Error ? overviewError.message : "读取概览失败");
      }
    } finally {
      if (!options.background) {
        setBusy(false);
      }
    }
  }

  async function loadNotes(query: NotesQueryState, options: LoadOptions = {}) {
    if (!options.background) {
      setBusy(true);
      setError(null);
    }
    try {
      const nextPage = await client.listNotes({
        page: 1,
        pageSize: 50,
        search: query.search || undefined,
        category: query.category || undefined,
        state: query.state,
      });
      setNotesPage(nextPage);
    } catch (notesError) {
      if (!options.suppressError) {
        setError(notesError instanceof Error ? notesError.message : "读取笔记失败");
      }
    } finally {
      if (!options.background) {
        setBusy(false);
      }
    }
  }

  async function loadNoteDetail(noteId: string, options: LoadOptions = {}) {
    if (!options.background) {
      setBusy(true);
      setError(null);
    }
    try {
      const [note, history] = await Promise.all([
        client.getNoteDetail(noteId),
        client.getNoteHistory(noteId),
      ]);
      setSelectedNote(note);
      setSelectedHistory(history);
    } catch (detailError) {
      if (!options.suppressError) {
        setError(detailError instanceof Error ? detailError.message : "读取详情失败");
      }
    } finally {
      if (!options.background) {
        setBusy(false);
      }
    }
  }

  async function openNote(noteId: string) {
    await loadNoteDetail(noteId);
  }

  async function downloadNote(noteId: string) {
    setError(null);
    try {
      const download = await client.downloadNote(noteId);
      triggerBlobDownload(
        download.fileName,
        new Blob([download.markdown], { type: "text/markdown;charset=utf-8" }),
      );
    } catch (downloadError) {
      setError(downloadError instanceof Error ? downloadError.message : "下载失败");
    }
  }

  async function downloadNotesArchive(noteIds: string[]) {
    setError(null);
    try {
      const download = await client.downloadNotesArchive(noteIds);
      triggerBlobDownload(download.fileName, download.blob);
    } catch (downloadError) {
      setError(downloadError instanceof Error ? downloadError.message : "批量下载失败");
    }
  }

  async function loadSettings() {
    setBusy(true);
    setError(null);
    try {
      setSettings(await client.getSettings());
    } catch (settingsError) {
      setError(settingsError instanceof Error ? settingsError.message : "读取设置失败");
    } finally {
      setBusy(false);
    }
  }

  async function saveSettings(request: SettingsUpdateRequest) {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const response = await client.updateSettings(request);
      setSettings({
        ...response.settings,
        pendingRestartFields: response.restartRequiredFields,
      });
    } catch (settingsError) {
      setError(settingsError instanceof Error ? settingsError.message : "保存设置失败");
    } finally {
      setBusy(false);
    }
  }

  async function rotateToken() {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const response = await client.resetSyncToken();
      setSettings((current) =>
        current
          ? {
              ...current,
              syncToken: response.syncToken,
              syncTokenConfigured: true,
            }
          : current,
      );
      setNotice("已轮换同步 Token。");
    } catch (tokenError) {
      setError(tokenError instanceof Error ? tokenError.message : "轮换 Token 失败");
    } finally {
      setBusy(false);
    }
  }

  async function restartServer() {
    if (typeof window !== "undefined") {
      const confirmed = window.confirm(
        "确认要重启服务吗？当前进程会退出，并依赖 Docker、systemd 等托管器自动拉起。",
      );
      if (!confirmed) {
        return;
      }
    }

    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await client.restartServer();
      setNotice("已发送重启请求，页面将在几秒后自动刷新。");
      if (typeof window !== "undefined") {
        window.setTimeout(() => {
          window.location.reload();
        }, 3000);
      }
    } catch (restartError) {
      setError(restartError instanceof Error ? restartError.message : "重启服务失败");
    } finally {
      setBusy(false);
    }
  }

  async function changePassword(request: PasswordChangeRequest) {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const nextSession = await client.changePassword(request);
      setSession(nextSession);
    } catch (passwordError) {
      setError(passwordError instanceof Error ? passwordError.message : "修改密码失败");
    } finally {
      setBusy(false);
    }
  }

  async function loadMaintenance() {
    setBusy(true);
    setError(null);
    try {
      const [nextBackups, nextLogs] = await Promise.all([
        client.listBackups(),
        client.readLogs(120),
      ]);
      setBackups(nextBackups);
      setLogs(nextLogs);
    } catch (maintenanceError) {
      setError(
        maintenanceError instanceof Error
          ? maintenanceError.message
          : "读取维护信息失败",
      );
    } finally {
      setBusy(false);
    }
  }

  async function createBackup() {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await client.createBackup();
      setBackups(await client.listBackups());
    } catch (backupError) {
      setError(backupError instanceof Error ? backupError.message : "创建备份失败");
    } finally {
      setBusy(false);
    }
  }

  async function restoreBackup(fileName: string) {
    if (typeof window !== "undefined") {
      const confirmed = window.confirm(
        `确认要恢复备份「${fileName}」吗？这会覆盖当前服务端的同步数据库。`,
      );
      if (!confirmed) {
        return;
      }
    }

    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await client.restoreBackup(fileName);
      setSelectedNote(null);
      setSelectedHistory([]);

      const [nextOverview, nextNotes, nextBackups, nextLogs] = await Promise.all([
        client.getOverview(),
        client.listNotes({
          page: 1,
          pageSize: 50,
          search: notesQuery.search || undefined,
          category: notesQuery.category || undefined,
          state: notesQuery.state,
        }),
        client.listBackups(),
        client.readLogs(120),
      ]);

      setOverview(nextOverview);
      setNotesPage(nextNotes);
      setBackups(nextBackups);
      setLogs(nextLogs);
    } catch (restoreError) {
      setError(restoreError instanceof Error ? restoreError.message : "恢复备份失败");
    } finally {
      setBusy(false);
    }
  }

  async function refreshLogs() {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      setLogs(await client.readLogs(120));
    } catch (logsError) {
      setError(logsError instanceof Error ? logsError.message : "刷新日志失败");
    } finally {
      setBusy(false);
    }
  }

  function clearAuthenticatedState() {
    setOverview(null);
    setNotesPage(null);
    setSelectedNote(null);
    setSelectedHistory([]);
    setSettings(null);
    setBackups(null);
    setLogs(null);
    setNotesQuery(defaultNotesQuery);
    setActiveView("overview");
    setNotice(null);
  }

  if (!session?.authenticated) {
    return (
      <main className="app-shell">
        <header className="topbar">
          <div>
            <p className="eyebrow">Floral Sync Admin</p>
            <h1>管理控制台</h1>
          </div>
          <div className="status-pill" aria-live="polite">
            {busy ? "正在检查初始化状态" : authMode === "bootstrap" ? "等待初始化" : "等待登录"}
          </div>
        </header>

        <section className="hero">
          <div className="hero-copy">
            <p className="section-label">Admin UI</p>
            <h2>先完成引导或登录，再查看同步状态、笔记和日志。</h2>
            <p className="muted">
              后台只提供只读查看、配置与维护，不会在浏览器里修改笔记内容。
            </p>
          </div>

          <LoginPage
            mode={authMode}
            busy={busy}
            error={error}
            onSubmit={handleAuthSubmit}
          />
        </section>
      </main>
    );
  }

  return (
    <div className="dashboard-shell">
      <Sidebar
        activeView={activeView}
        session={session}
        latestRevision={overview?.latestRevision ?? null}
        onSelectView={setActiveView}
      />

      <main className="workspace-shell">
        <div className="workspace-header">
          <div>
            <p className="section-label">管理工作区</p>
            <h2>{viewTitle(activeView)}</h2>
          </div>
          <button type="button" className="secondary" onClick={() => void handleLogout()} disabled={busy}>
            退出登录
          </button>
        </div>

        {error ? <div className="error-banner">{error}</div> : null}
        {notice ? <div className="notice-banner">{notice}</div> : null}

        {activeView === "overview" ? (
          <OverviewPage overview={overview} busy={busy} onRefresh={() => void loadOverview()} />
        ) : null}
        {activeView === "notes" ? (
          <NotesPage
            busy={busy}
            query={notesQuery}
            categories={categories}
            notesPage={notesPage}
            selectedNote={selectedNote}
            selectedHistory={selectedHistory}
            onQueryChange={setNotesQuery}
            onSelectNote={(noteId) => void openNote(noteId)}
            onCloseDetail={() => {
              setSelectedNote(null);
              setSelectedHistory([]);
            }}
            onDownloadNote={(noteId) => void downloadNote(noteId)}
            onDownloadSelected={(noteIds) => void downloadNotesArchive(noteIds)}
          />
        ) : null}
        {activeView === "settings" ? (
          <SettingsPage
            settings={settings}
            busy={busy}
            onSave={(request) => void saveSettings(request)}
            onRotateToken={() => void rotateToken()}
            onRestartServer={() => void restartServer()}
            onChangePassword={(request) => void changePassword(request)}
          />
        ) : null}
        {activeView === "maintenance" ? (
          <MaintenancePage
            busy={busy}
            backups={backups ?? []}
            logs={logs}
            onCreateBackup={() => void createBackup()}
            onRestoreBackup={(fileName) => void restoreBackup(fileName)}
            onRefreshLogs={() => void refreshLogs()}
          />
        ) : null}
      </main>
    </div>
  );
}

function viewTitle(view: AdminView): string {
  switch (view) {
    case "overview":
      return "服务总览";
    case "notes":
      return "只读笔记";
    case "settings":
      return "服务设置";
    case "maintenance":
      return "维护与巡检";
  }
}

function triggerBlobDownload(fileName: string, blob: Blob) {
  if (typeof window === "undefined" || typeof document === "undefined") {
    return;
  }

  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = fileName;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
}
