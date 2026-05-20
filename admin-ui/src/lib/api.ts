import type {
  ApiErrorPayload,
  BackupEntry,
  BackupResponse,
  DownloadedArchive,
  RestoreBackupResponse,
  DownloadedNote,
  NoteDetail,
  NoteSnapshot,
  NotesPageResponse,
  NotesQuery,
  OverviewResponse,
  PasswordChangeRequest,
  PasswordRequest,
  RestartResponse,
  SessionResponse,
  SettingsResponse,
  SettingsUpdateRequest,
  SettingsUpdateResponse,
  TokenResetResponse,
  LogsResponse,
} from "./types";

type FetchLike = typeof fetch;
const AUTH_LOGIN_PATH = "/login";
const AUTH_LOGOUT_PATH = "/logout";
const ADMIN_API_PREFIX = "/admin/api";

export class AdminApiError extends Error {
  readonly status: number;
  readonly payload: ApiErrorPayload | null;

  constructor(status: number, payload: ApiErrorPayload | null, message: string) {
    super(message);
    this.name = "AdminApiError";
    this.status = status;
    this.payload = payload;
  }
}

export interface AdminApiClient {
  getSession(): Promise<SessionResponse>;
  login(request: PasswordRequest): Promise<SessionResponse>;
  logout(): Promise<SessionResponse>;
  bootstrap(request: PasswordRequest): Promise<SessionResponse>;
  getOverview(): Promise<OverviewResponse>;
  listNotes(query?: NotesQuery): Promise<NotesPageResponse>;
  getNoteDetail(id: string): Promise<NoteDetail>;
  getNoteHistory(id: string): Promise<NoteSnapshot[]>;
  downloadNote(id: string): Promise<DownloadedNote>;
  downloadNotesArchive(ids: string[]): Promise<DownloadedArchive>;
  getSettings(): Promise<SettingsResponse>;
  updateSettings(request: SettingsUpdateRequest): Promise<SettingsUpdateResponse>;
  resetSyncToken(): Promise<TokenResetResponse>;
  restartServer(): Promise<RestartResponse>;
  changePassword(request: PasswordChangeRequest): Promise<SessionResponse>;
  createBackup(): Promise<BackupResponse>;
  listBackups(): Promise<BackupEntry[]>;
  restoreBackup(fileName: string): Promise<RestoreBackupResponse>;
  readLogs(limit?: number): Promise<LogsResponse>;
}

export function createAdminApiClient(baseUrl = "", fetchImpl?: FetchLike): AdminApiClient {
  const send = async (path: string, init: RequestInit = {}): Promise<Response> => {
    const fetcher = fetchImpl ?? globalThis.fetch;
    if (!fetcher) {
      throw new Error("fetch is not available in this environment");
    }

    const response = await fetcher(`${baseUrl}${path}`, {
      credentials: "include",
      ...init,
      headers: {
        Accept: "application/json",
        ...(init.body ? { "Content-Type": "application/json" } : null),
        ...(init.headers ?? {}),
      },
    });

    if (!response.ok) {
      const { payload, message } = await readError(response);
      throw new AdminApiError(
        response.status,
        payload,
        message,
      );
    }

    return response;
  };

  const request = async <T,>(path: string, init: RequestInit = {}): Promise<T> => {
    const response = await send(path, init);
    return (await response.json()) as T;
  };

  const withQuery = (path: string, query: Record<string, string | number | undefined | null>) => {
    const params = new URLSearchParams();
    for (const [key, value] of Object.entries(query)) {
      if (value === undefined || value === null || value === "") {
        continue;
      }
      params.set(key, String(value));
    }
    const queryString = params.toString();
    return queryString ? `${path}?${queryString}` : path;
  };

  return {
    getSession: () => request<SessionResponse>("/admin/api/session"),
    login: (requestBody) =>
      request<SessionResponse>(AUTH_LOGIN_PATH, {
        method: "POST",
        body: JSON.stringify(requestBody),
      }),
    // The sync server intentionally exposes auth entrypoints at the top level.
    logout: () => request<SessionResponse>(AUTH_LOGOUT_PATH, { method: "POST" }),
    bootstrap: (requestBody) =>
      request<SessionResponse>(`${ADMIN_API_PREFIX}/bootstrap`, {
        method: "POST",
        body: JSON.stringify(requestBody),
      }),
    getOverview: () => request<OverviewResponse>(`${ADMIN_API_PREFIX}/overview`),
    listNotes: (query) =>
      request<NotesPageResponse>(
        withQuery(`${ADMIN_API_PREFIX}/notes`, {
          page: query?.page,
          pageSize: query?.pageSize,
          search: query?.search,
          category: query?.category,
          state: query?.state,
        }),
      ),
    getNoteDetail: (id) =>
      request<NoteDetail>(`${ADMIN_API_PREFIX}/notes/${encodeURIComponent(id)}`),
    getNoteHistory: (id) =>
      request<NoteSnapshot[]>(
        `${ADMIN_API_PREFIX}/notes/${encodeURIComponent(id)}/history`,
      ),
    downloadNote: async (id) => {
      const response = await send(`${ADMIN_API_PREFIX}/notes/${encodeURIComponent(id)}/download`, {
        headers: {
          Accept: "text/markdown",
        },
      });

      return {
        fileName:
          extractFileName(response.headers.get("content-disposition")) ?? `${id}.md`,
        markdown: await response.text(),
      };
    },
    downloadNotesArchive: async (ids) => {
      const normalizedIds = ids
        .map((id) => id.trim())
        .filter((id) => id.length > 0);
      const path = withQuery(`${ADMIN_API_PREFIX}/notes/download.zip`, {
        ids: normalizedIds.length > 0 ? normalizedIds.join(",") : undefined,
      });
      const response = await send(path, {
        headers: {
          Accept: "application/zip",
        },
      });

      return {
        fileName:
          extractFileName(response.headers.get("content-disposition")) ??
          "floral-sync-notes.zip",
        blob: await response.blob(),
      };
    },
    getSettings: () => request<SettingsResponse>(`${ADMIN_API_PREFIX}/settings`),
    updateSettings: (requestBody) =>
      request<SettingsUpdateResponse>(`${ADMIN_API_PREFIX}/settings`, {
        method: "POST",
        body: JSON.stringify(requestBody),
      }),
    resetSyncToken: () =>
      request<TokenResetResponse>(`${ADMIN_API_PREFIX}/settings/token/reset`, {
        method: "POST",
      }),
    restartServer: () =>
      request<RestartResponse>(`${ADMIN_API_PREFIX}/settings/restart`, {
        method: "POST",
      }),
    changePassword: (requestBody) =>
      request<SessionResponse>(`${ADMIN_API_PREFIX}/settings/password`, {
        method: "POST",
        body: JSON.stringify(requestBody),
      }),
    createBackup: () =>
      request<BackupResponse>(`${ADMIN_API_PREFIX}/maintenance/backup`, {
        method: "POST",
      }),
    listBackups: () => request<BackupEntry[]>(`${ADMIN_API_PREFIX}/maintenance/backups`),
    restoreBackup: (fileName) =>
      request<RestoreBackupResponse>(`${ADMIN_API_PREFIX}/maintenance/restore`, {
        method: "POST",
        body: JSON.stringify({ fileName }),
      }),
    readLogs: (limit) =>
      request<LogsResponse>(withQuery(`${ADMIN_API_PREFIX}/logs`, { limit })),
  };
}

function extractFileName(contentDisposition: string | null): string | null {
  if (!contentDisposition) {
    return null;
  }

  const utf8Match = /filename\*=UTF-8''([^;]+)/i.exec(contentDisposition);
  if (utf8Match?.[1]) {
    try {
      return decodeURIComponent(utf8Match[1]);
    } catch {
      return utf8Match[1];
    }
  }

  const quotedMatch = /filename="([^"]+)"/i.exec(contentDisposition);
  if (quotedMatch?.[1]) {
    return quotedMatch[1];
  }

  const plainMatch = /filename=([^;]+)/i.exec(contentDisposition);
  return plainMatch?.[1]?.trim() ?? null;
}

async function readError(
  response: Response,
): Promise<{ payload: ApiErrorPayload | null; message: string }> {
  const contentType = response.headers.get("content-type") ?? "";
  if (contentType.includes("application/json")) {
    const payload = (await response.json().catch(() => null)) as ApiErrorPayload | null;
    if (payload?.error) {
      return {
        payload,
        message: payload.error,
      };
    }
  }

  const text = await response.text();
  return {
    payload: null,
    message: text || response.statusText,
  };
}
