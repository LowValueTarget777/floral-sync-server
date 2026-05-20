import { useEffect, useState } from "react";
import type {
  PasswordChangeRequest,
  SettingsResponse,
  SettingsUpdateRequest,
} from "../lib/types";

type SettingsPageProps = {
  settings: SettingsResponse | null;
  busy: boolean;
  onSave: (request: SettingsUpdateRequest) => void;
  onRotateToken: () => void;
  onRestartServer: () => void;
  onChangePassword: (request: PasswordChangeRequest) => void;
};

export function SettingsPage({
  settings,
  busy,
  onSave,
  onRotateToken,
  onRestartServer,
  onChangePassword,
}: SettingsPageProps) {
  const [draft, setDraft] = useState<SettingsUpdateRequest>({
    syncListen: [],
    adminListen: [],
    dbPath: "",
    exportDir: "",
    logPath: "",
    logLevel: "info",
  });
  const [passwordDraft, setPasswordDraft] = useState<PasswordChangeRequest>({
    currentPassword: "",
    newPassword: "",
    confirmPassword: "",
  });
  const [showSyncToken, setShowSyncToken] = useState(false);
  const [copyFeedback, setCopyFeedback] = useState<"idle" | "copied" | "failed">("idle");

  useEffect(() => {
    if (!settings) {
      return;
    }
    setDraft({
      syncListen: settings.syncListen,
      adminListen: settings.adminListen,
      dbPath: settings.dbPath,
      exportDir: settings.exportDir,
      logPath: settings.logPath,
      logLevel: settings.logLevel,
    });
  }, [settings]);

  const hasRestartFlag = (field: string) =>
    settings?.pendingRestartFields.includes(field) ?? false;

  useEffect(() => {
    setCopyFeedback("idle");
  }, [settings?.syncToken]);

  const syncToken = settings?.syncToken ?? "";
  const tokenPreview = syncToken
    ? showSyncToken
      ? syncToken
      : maskToken(syncToken)
    : "未配置";

  async function handleCopySyncToken() {
    if (!syncToken) {
      return;
    }

    try {
      await copyText(syncToken);
      setCopyFeedback("copied");
    } catch {
      setCopyFeedback("failed");
    }
  }

  return (
    <section className="page-shell">
      <header className="page-header">
        <div>
          <p className="section-label">设置</p>
          <h2>服务配置</h2>
          <p className="muted">即时生效的项会直接更新，其余项会明确标记需要重启。</p>
        </div>
      </header>

      <div className="settings-grid">
        <form
          className="panel form-grid"
          onSubmit={(event) => {
            event.preventDefault();
            onSave(draft);
          }}
        >
          <div className="panel-heading">
            <h3>监听与路径</h3>
          </div>

          <label className="field">
            <span>
              同步监听
              {hasRestartFlag("syncListen") ? <em className="restart-badge">需要重启</em> : null}
            </span>
            <input
              value={draft.syncListen?.join(", ") ?? ""}
              onChange={(event) =>
                setDraft({
                  ...draft,
                  syncListen: splitAddresses(event.target.value),
                })
              }
            />
          </label>

          <label className="field">
            <span>
              管理监听
              {hasRestartFlag("adminListen") ? <em className="restart-badge">需要重启</em> : null}
            </span>
            <input
              value={draft.adminListen?.join(", ") ?? ""}
              onChange={(event) =>
                setDraft({
                  ...draft,
                  adminListen: splitAddresses(event.target.value),
                })
              }
            />
          </label>

          <label className="field">
            <span>
              数据库路径
              {hasRestartFlag("dbPath") ? <em className="restart-badge">需要重启</em> : null}
            </span>
            <input
              value={draft.dbPath ?? ""}
              onChange={(event) =>
                setDraft({
                  ...draft,
                  dbPath: event.target.value,
                })
              }
            />
          </label>

          <label className="field">
            <span>
              导出目录
              {hasRestartFlag("exportDir") ? <em className="restart-badge">需要重启</em> : null}
            </span>
            <input
              value={draft.exportDir ?? ""}
              onChange={(event) =>
                setDraft({
                  ...draft,
                  exportDir: event.target.value,
                })
              }
            />
          </label>

          <label className="field">
            <span>
              日志文件
              {hasRestartFlag("logPath") ? <em className="restart-badge">需要重启</em> : null}
            </span>
            <input
              value={draft.logPath ?? ""}
              onChange={(event) =>
                setDraft({
                  ...draft,
                  logPath: event.target.value,
                })
              }
            />
          </label>

          <label className="field">
            <span>
              日志级别
              {hasRestartFlag("logLevel") ? <em className="restart-badge">需要重启</em> : null}
            </span>
            <select
              value={draft.logLevel ?? "info"}
              onChange={(event) =>
                setDraft({
                  ...draft,
                  logLevel: event.target.value,
                })
              }
            >
              <option value="trace">trace</option>
              <option value="debug">debug</option>
              <option value="info">info</option>
              <option value="warn">warn</option>
              <option value="error">error</option>
            </select>
          </label>

          <button type="submit" disabled={busy}>
            保存配置
          </button>
        </form>

        <div className="stack-column">
          <section className="panel form-grid">
            <div className="panel-heading">
              <h3>同步凭据</h3>
            </div>
            <ul className="simple-list secret-list">
              <li>
                <strong>同步 Token</strong>
                <span>{settings?.syncTokenConfigured ? "已配置 · ••••••••" : "未配置"}</span>
              </li>
              <li>
                <strong>管理员密码</strong>
                <span>{settings?.adminPasswordConfigured ? "已配置 · ••••••••" : "未配置"}</span>
              </li>
              <li>
                <strong>会话密钥</strong>
                <span>
                  {settings?.adminSessionSecretConfigured
                    ? "已配置 · ••••••••"
                    : "未配置"}
                </span>
              </li>
            </ul>
            <p className="muted">当前同步 Token 默认隐藏显示，你可以在这里直接查看、复制或轮换。</p>
            <code className="token-preview">{tokenPreview}</code>
            <div className="button-row">
              <button
                type="button"
                className="secondary"
                onClick={() => setShowSyncToken((value) => !value)}
                disabled={busy || !settings?.syncTokenConfigured}
              >
                {showSyncToken ? "隐藏 Token" : "显示 Token"}
              </button>
              <button
                type="button"
                className="secondary"
                onClick={() => void handleCopySyncToken()}
                disabled={busy || !settings?.syncTokenConfigured}
              >
                复制 Token
              </button>
            </div>
            {copyFeedback === "copied" ? <p className="muted">已复制到剪贴板。</p> : null}
            {copyFeedback === "failed" ? <p className="muted">复制失败，请手动复制。</p> : null}
            <button type="button" onClick={onRotateToken} disabled={busy}>
              轮换同步 Token
            </button>
          </section>

          <section className="panel form-grid">
            <div className="panel-heading">
              <h3>服务进程</h3>
            </div>
            <p className="muted">修改监听地址、数据库路径等需要重启的配置后，可以在这里直接请求服务重启。</p>
            <p className="muted">当前进程退出后，需要由 Docker、systemd 等托管器自动重新拉起。</p>
            <button type="button" className="secondary" onClick={onRestartServer} disabled={busy}>
              一键重启服务
            </button>
          </section>

          <form
            className="panel form-grid"
            onSubmit={(event) => {
              event.preventDefault();
              onChangePassword(passwordDraft);
            }}
          >
            <div className="panel-heading">
              <h3>管理员密码</h3>
            </div>
            <label className="field">
              <span>当前密码</span>
              <input
                type="password"
                value={passwordDraft.currentPassword}
                onChange={(event) =>
                  setPasswordDraft({
                    ...passwordDraft,
                    currentPassword: event.target.value,
                  })
                }
              />
            </label>
            <label className="field">
              <span>新密码</span>
              <input
                type="password"
                value={passwordDraft.newPassword}
                onChange={(event) =>
                  setPasswordDraft({
                    ...passwordDraft,
                    newPassword: event.target.value,
                  })
                }
              />
            </label>
            <label className="field">
              <span>确认新密码</span>
              <input
                type="password"
                value={passwordDraft.confirmPassword}
                onChange={(event) =>
                  setPasswordDraft({
                    ...passwordDraft,
                    confirmPassword: event.target.value,
                  })
                }
              />
            </label>
            <button type="submit" disabled={busy}>
              更新密码
            </button>
          </form>
        </div>
      </div>
    </section>
  );
}

function splitAddresses(value: string): string[] {
  return value
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

function maskToken(value: string): string {
  return value.replace(/[A-Za-z0-9]/g, "•");
}

async function copyText(value: string): Promise<void> {
  if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(value);
    return;
  }

  if (typeof document === "undefined") {
    throw new Error("clipboard is not available");
  }

  const textarea = document.createElement("textarea");
  textarea.value = value;
  textarea.setAttribute("readonly", "true");
  textarea.style.position = "absolute";
  textarea.style.left = "-9999px";
  document.body.appendChild(textarea);
  textarea.select();
  const copied = document.execCommand("copy");
  textarea.remove();

  if (!copied) {
    throw new Error("copy failed");
  }
}
