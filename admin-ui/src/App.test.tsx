import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { AdminApiError, createAdminApiClient, type AdminApiClient } from "./lib/api";
import { App } from "./App";
import type {
  BackupResponse,
  NoteDetail,
  NotesPageResponse,
  OverviewResponse,
  RestoreBackupResponse,
  SessionResponse,
  SettingsResponse,
  SettingsUpdateResponse,
  TokenResetResponse,
} from "./lib/types";

afterEach(() => {
  cleanup();
});

describe("App smoke shell", () => {
  test("mounts the bootstrap route when the server needs initial setup", async () => {
    render(<App client={createClient({ session: sessionFixture({ bootstrapRequired: true }) })} />);

    expect(
      await screen.findByRole("button", { name: "创建密码并进入控制台" }),
    ).toBeDefined();
  });

  test("mounts the login route when the server is configured but unauthenticated", async () => {
    render(<App client={createClient({ session: sessionFixture({ bootstrapRequired: false }) })} />);

    expect(await screen.findByRole("button", { name: "登录" })).toBeDefined();
  });

  test("loads the authenticated overview after session restoration", async () => {
    render(<App client={createClient({ session: authenticatedSession() })} />);

    expect(await screen.findByText("最近 15 分钟内收到 3 次同步写入。")).toBeDefined();
    expect(screen.getByText("127.0.0.1:8788")).toBeDefined();
  });

  test("clears an overview error after a successful refresh", async () => {
    const client = createClient({
      session: authenticatedSession(),
      overviewSequence: [new Error("暂时无法读取概览"), overviewFixture({ recentActivitySummary: "刷新后恢复正常。" })],
    });

    render(<App client={client} />);

    expect(await screen.findByText("暂时无法读取概览")).toBeDefined();

    fireEvent.click(screen.getByRole("button", { name: "刷新概览" }));

    await waitFor(() => {
      expect(screen.getByText("刷新后恢复正常。")).toBeDefined();
    });
    expect(screen.queryByText("暂时无法读取概览")).toBeNull();
  });

  test("returns to the login route after logout", async () => {
    render(<App client={createClient({ session: authenticatedSession() })} />);

    expect(await screen.findByText("最近 15 分钟内收到 3 次同步写入。")).toBeDefined();

    fireEvent.click(screen.getByRole("button", { name: "退出登录" }));

    expect(await screen.findByRole("button", { name: "登录" })).toBeDefined();
  });

  test("shows a friendly error when the session check fails", async () => {
    render(<App client={createClient({ sessionError: new Error("无法连接到管理接口") })} />);

    expect(await screen.findByText("无法连接到管理接口")).toBeDefined();
  });

  test("refreshes notes automatically while the notes view stays open", async () => {
    let noteCallCount = 0;
    const listNotes = async () => {
      noteCallCount += 1;
      return notesPageFixture({ total: noteCallCount });
    };

    render(
      <App
        notesAutoRefreshMs={10}
        client={createClient({
          session: authenticatedSession(),
          clientOverrides: {
            listNotes,
          },
        })}
      />,
    );

    expect(await screen.findByText("最近 15 分钟内收到 3 次同步写入。")).toBeDefined();

    fireEvent.click(screen.getByRole("button", { name: /笔记/ }));

    await waitFor(() => {
      expect(noteCallCount).toBeGreaterThanOrEqual(1);
    });

    await waitFor(() => {
      expect(noteCallCount).toBeGreaterThanOrEqual(2);
    });
  });

  test("loads maintenance data once even when there are no backups", async () => {
    let backupCallCount = 0;
    let logCallCount = 0;
    const listBackups = async () => {
      backupCallCount += 1;
      return [];
    };
    const readLogs = async () => {
      logCallCount += 1;
      return {
        path: "logs/floral-sync-server.log",
        lines: [],
      };
    };

    render(
      <App
        client={createClient({
          session: authenticatedSession(),
          clientOverrides: {
            listBackups,
            readLogs,
          },
        })}
      />,
    );

    expect(await screen.findByText("最近 15 分钟内收到 3 次同步写入。")).toBeDefined();

    fireEvent.click(screen.getByRole("button", { name: /维护/ }));

    await waitFor(() => {
      expect(
        (screen.getByRole("button", { name: "创建备份" }) as HTMLButtonElement).disabled,
      ).toBe(false);
    });

    await new Promise((resolve) => setTimeout(resolve, 20));

    expect(backupCallCount).toBe(1);
    expect(logCallCount).toBe(1);
  });

  test("restores a backup from the maintenance view", async () => {
    const restoreCalls: string[] = [];
    const originalConfirm = window.confirm;
    window.confirm = () => true;

    render(
      <App
        client={createClient({
          session: authenticatedSession(),
          clientOverrides: {
            listBackups: async () => [
              {
                fileName: "backup-2026-05-19.sqlite3",
                sizeBytes: 2048,
              },
            ],
            restoreBackup: async (fileName) => {
              restoreCalls.push(fileName);
              return { fileName };
            },
          },
        })}
      />,
    );

    expect(await screen.findByText("最近 15 分钟内收到 3 次同步写入。")).toBeDefined();

    fireEvent.click(screen.getByRole("button", { name: /维护/ }));

    expect(
      await screen.findByRole("button", { name: "恢复备份 backup-2026-05-19.sqlite3" }),
    ).toBeDefined();

    fireEvent.click(screen.getByRole("button", { name: "恢复备份 backup-2026-05-19.sqlite3" }));

    await waitFor(() => {
      expect(restoreCalls).toEqual(["backup-2026-05-19.sqlite3"]);
    });

    window.confirm = originalConfirm;
  });
});

describe("createAdminApiClient", () => {
  test("returns JSON responses without consuming the body twice", async () => {
    const client = createAdminApiClient(
      "",
      (async () =>
        new Response(
          JSON.stringify(sessionFixture({ bootstrapRequired: true })),
          {
            status: 200,
            headers: {
              "content-type": "application/json",
            },
          },
        )) as typeof fetch,
    );

    await expect(client.getSession()).resolves.toEqual(
      sessionFixture({ bootstrapRequired: true }),
    );
  });

  test("parses UTF-8 filenames for markdown downloads", async () => {
    const client = createAdminApiClient(
      "",
      (async () =>
        new Response("# note", {
          status: 200,
          headers: {
            "content-type": "text/markdown; charset=utf-8",
            "content-disposition":
              "attachment; filename*=UTF-8''%E6%B5%8B%E8%AF%95%E7%AC%94%E8%AE%B0.md",
          },
        })) as typeof fetch,
    );

    await expect(client.downloadNote("note-1")).resolves.toEqual({
      fileName: "测试笔记.md",
      markdown: "# note",
    });
  });

  test("preserves JSON error payloads on failed requests", async () => {
    const client = createAdminApiClient(
      "",
      (async () =>
        new Response(JSON.stringify({ error: "认证失败" }), {
          status: 401,
          statusText: "Unauthorized",
          headers: {
            "content-type": "application/json",
          },
        })) as typeof fetch,
    );

    await expect(client.getSession()).rejects.toBeInstanceOf(AdminApiError);
    await expect(client.getSession()).rejects.toMatchObject({
      status: 401,
      message: "认证失败",
    });
  });
});

type ClientOptions = {
  session?: SessionResponse;
  sessionError?: Error;
  overviewSequence?: Array<OverviewResponse | Error>;
  clientOverrides?: Partial<AdminApiClient>;
};

function createClient(options: ClientOptions = {}): AdminApiClient {
  const overviewSequence = options.overviewSequence ?? [overviewFixture()];
  let overviewIndex = 0;

  const nextOverview = async (): Promise<OverviewResponse> => {
    const current = overviewSequence[Math.min(overviewIndex, overviewSequence.length - 1)];
    overviewIndex += 1;
    if (current instanceof Error) {
      throw current;
    }
    return current;
  };

  return {
    getSession: async () => {
      if (options.sessionError) {
        throw options.sessionError;
      }
      return options.session ?? sessionFixture();
    },
    login: async () => authenticatedSession(),
    logout: async () => sessionFixture({ bootstrapRequired: false }),
    bootstrap: async () => authenticatedSession(),
    getOverview: nextOverview,
    listNotes: async () => notesPageFixture(),
    getNoteDetail: async () => noteDetailFixture(),
    getNoteHistory: async () => [],
    downloadNote: async () => ({
      fileName: "note.md",
      markdown: "# note",
    }),
    downloadNotesArchive: async () => ({
      fileName: "notes.zip",
      blob: new Blob(["zip-data"], { type: "application/zip" }),
    }),
    getSettings: async () => settingsFixture(),
    updateSettings: async () => settingsUpdateFixture(),
    resetSyncToken: async () => tokenResetFixture(),
    restartServer: async () => ({ restartRequested: true }),
    changePassword: async () => authenticatedSession(),
    createBackup: async () => backupFixture(),
    listBackups: async () => [],
    restoreBackup: async (fileName) => restoreBackupFixture(fileName),
    readLogs: async () => ({
      path: "logs/floral-sync-server.log",
      lines: [],
    }),
    ...options.clientOverrides,
  };
}

function sessionFixture(overrides: Partial<SessionResponse> = {}): SessionResponse {
  return {
    authenticated: false,
    bootstrapRequired: false,
    passwordConfigured: true,
    expiresAt: null,
    ...overrides,
  };
}

function authenticatedSession(): SessionResponse {
  return {
    authenticated: true,
    bootstrapRequired: false,
    passwordConfigured: true,
    expiresAt: "2026-05-19T08:00:00Z",
  };
}

function overviewFixture(overrides: Partial<OverviewResponse> = {}): OverviewResponse {
  return {
    latestRevision: 42,
    noteCount: 12,
    deletedNoteCount: 1,
    categoryCount: 3,
    latestSnapshotAt: "2026-05-19T08:00:00Z",
    syncListen: ["0.0.0.0:8787"],
    adminListen: ["127.0.0.1:8788"],
    dbPath: "data/floral-sync.sqlite3",
    exportDir: "exports",
    logPath: "logs/floral-sync-server.log",
    logLevel: "info",
    recentActivitySummary: "最近 15 分钟内收到 3 次同步写入。",
    ...overrides,
  };
}

function notesPageFixture(overrides: Partial<NotesPageResponse> = {}): NotesPageResponse {
  return {
    page: 1,
    pageSize: 20,
    total: 1,
    notes: [],
    ...overrides,
  };
}

function noteDetailFixture(): NoteDetail {
  return {
    id: "note-1",
    title: "测试笔记",
    content: "只读内容",
    category: "默认",
    createdAt: "2026-05-19T07:00:00Z",
    updatedAt: "2026-05-19T08:00:00Z",
    deletedAt: null,
    contentHash: "hash",
    deviceId: "device-a",
    revision: 42,
  };
}

function settingsFixture(): SettingsResponse {
  return {
    syncListen: ["0.0.0.0:8787"],
    adminListen: ["127.0.0.1:8788"],
    dbPath: "data/floral-sync.sqlite3",
    exportDir: "exports",
    logPath: "logs/floral-sync-server.log",
    logLevel: "info",
    syncToken: "current-sync-token",
    syncTokenConfigured: true,
    adminPasswordConfigured: true,
    adminSessionSecretConfigured: true,
    pendingRestartFields: [],
  };
}

function settingsUpdateFixture(): SettingsUpdateResponse {
  return {
    settings: settingsFixture(),
    restartRequiredFields: [],
  };
}

function tokenResetFixture(): TokenResetResponse {
  return {
    syncToken: "next-token",
  };
}

function backupFixture(): BackupResponse {
  return {
    fileName: "backup.sqlite3",
    path: "exports/backup.sqlite3",
  };
}

function restoreBackupFixture(fileName: string): RestoreBackupResponse {
  return {
    fileName,
  };
}
