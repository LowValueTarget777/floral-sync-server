import { useState } from "react";

type LoginPageProps = {
  mode: "checking" | "bootstrap" | "login";
  busy: boolean;
  error: string | null;
  onSubmit: (password: string) => void;
};

export function LoginPage({ mode, busy, error, onSubmit }: LoginPageProps) {
  const [password, setPassword] = useState("");

  const submitLabel =
    mode === "bootstrap" ? "创建密码并进入控制台" : "登录";
  const title =
    mode === "bootstrap" ? "初始化引导" : mode === "login" ? "登录" : "正在检查";
  const hint =
    mode === "bootstrap"
      ? "首次启动需要先设置管理员密码，之后才能进入完整后台。"
      : "使用管理员密码进入只读后台，查看同步状态、笔记快照和维护信息。";

  if (mode === "checking") {
    return (
      <section className="panel stack auth-panel">
        <p className="section-label">Admin UI</p>
        <h2>正在检查会话状态</h2>
        <p className="muted">如果后台尚未初始化，会自动进入首次引导。</p>
        {error ? <p className="error">{error}</p> : null}
      </section>
    );
  }

  return (
    <section className="panel stack auth-panel">
      <p className="section-label">Admin UI</p>
      <h2>{title}</h2>
      <p className="muted">{hint}</p>

      <label className="field">
        <span>管理员密码</span>
        <input
          aria-label="管理员密码"
          type="password"
          value={password}
          onChange={(event) => setPassword(event.target.value)}
          placeholder={mode === "bootstrap" ? "设置一个新的管理员密码" : "输入管理员密码"}
        />
      </label>

      <button
        type="button"
        onClick={() => onSubmit(password)}
        disabled={busy || !password.trim()}
      >
        {submitLabel}
      </button>

      {error ? <p className="error">{error}</p> : null}
    </section>
  );
}
