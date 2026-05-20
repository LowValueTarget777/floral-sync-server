export type IsoDateTime = string;

export interface ApiErrorPayload {
  error: string;
}

export interface SessionResponse {
  authenticated: boolean;
  bootstrapRequired: boolean;
  passwordConfigured: boolean;
  expiresAt: IsoDateTime | null;
}

export interface PasswordRequest {
  password: string;
}

export interface PasswordChangeRequest {
  currentPassword: string;
  newPassword: string;
  confirmPassword: string;
}

export interface OverviewResponse {
  latestRevision: number;
  noteCount: number;
  deletedNoteCount: number;
  categoryCount: number;
  latestSnapshotAt: IsoDateTime | null;
  syncListen: string[];
  adminListen: string[];
  dbPath: string;
  exportDir: string;
  logPath: string;
  logLevel: string;
  recentActivitySummary: string;
}

export type NoteStateFilter = "all" | "active" | "deleted";

export interface NoteListItem {
  id: string;
  title: string;
  category: string;
  updatedAt: IsoDateTime;
  deletedAt: IsoDateTime | null;
  deviceId: string;
  revision: number;
}

export interface NoteDetail {
  id: string;
  title: string;
  content: string;
  category: string;
  createdAt: IsoDateTime;
  updatedAt: IsoDateTime;
  deletedAt: IsoDateTime | null;
  contentHash: string;
  deviceId: string;
  revision: number;
}

export interface NoteSnapshot {
  snapshotId: number;
  noteId: string;
  revision: number;
  title: string;
  content: string;
  category: string;
  createdAt: IsoDateTime;
  updatedAt: IsoDateTime;
  deletedAt: IsoDateTime | null;
  contentHash: string;
  deviceId: string;
  capturedAt: IsoDateTime;
}

export interface NotesPageResponse {
  page: number;
  pageSize: number;
  total: number;
  notes: NoteListItem[];
}

export interface NotesQuery {
  page?: number;
  pageSize?: number;
  search?: string;
  category?: string;
  state?: NoteStateFilter;
}

export interface SettingsResponse {
  syncListen: string[];
  adminListen: string[];
  dbPath: string;
  exportDir: string;
  logPath: string;
  logLevel: string;
  syncToken: string;
  syncTokenConfigured: boolean;
  adminPasswordConfigured: boolean;
  adminSessionSecretConfigured: boolean;
  pendingRestartFields: string[];
}

export interface SettingsUpdateRequest {
  syncListen?: string[];
  adminListen?: string[];
  dbPath?: string;
  exportDir?: string;
  logPath?: string;
  logLevel?: string;
}

export interface SettingsUpdateResponse {
  settings: SettingsResponse;
  restartRequiredFields: string[];
}

export interface TokenResetResponse {
  syncToken: string;
}

export interface RestartResponse {
  restartRequested: boolean;
}

export interface BackupResponse {
  fileName: string;
  path: string;
}

export interface BackupEntry {
  fileName: string;
  sizeBytes: number;
}

export interface RestoreBackupResponse {
  fileName: string;
}

export interface DownloadedNote {
  fileName: string;
  markdown: string;
}

export interface DownloadedArchive {
  fileName: string;
  blob: Blob;
}

export interface LogsResponse {
  path: string;
  lines: string[];
}
