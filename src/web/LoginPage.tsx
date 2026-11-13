import { useState, type FormEvent } from "react";
import { postJson } from "./api";

interface LoginPageProps {
  onSuccess: () => void;
}

export function LoginPage({ onSuccess }: LoginPageProps) {
  const [username, setUsername] = useState("admin");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    setError(null);
    setBusy(true);
    try {
      await postJson("/api/auth/login", { username, password });
      onSuccess();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-background px-4 text-foreground">
      <form
        onSubmit={handleSubmit}
        className="w-full max-w-sm space-y-4 rounded-xl border bg-card p-6 shadow-sm"
      >
        <div className="space-y-1 text-center">
          <h1 className="text-xl font-semibold">CC Switch Web</h1>
          <p className="text-sm text-muted-foreground">
            登录以管理 Claude Code / Codex / Gemini 等配置
          </p>
        </div>
        <label className="block space-y-1 text-sm">
          <span className="text-muted-foreground">用户名</span>
          <input
            className="w-full rounded-md border bg-background px-3 py-2 outline-none focus:ring-2 focus:ring-ring"
            value={username}
            autoComplete="username"
            onChange={(event) => setUsername(event.target.value)}
            required
          />
        </label>
        <label className="block space-y-1 text-sm">
          <span className="text-muted-foreground">密码</span>
          <input
            type="password"
            className="w-full rounded-md border bg-background px-3 py-2 outline-none focus:ring-2 focus:ring-ring"
            value={password}
            autoComplete="current-password"
            onChange={(event) => setPassword(event.target.value)}
            required
          />
        </label>
        {error ? (
          <p className="rounded-md bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {error}
          </p>
        ) : null}
        <button
          type="submit"
          disabled={busy}
          className="w-full rounded-md bg-primary px-3 py-2 text-sm font-medium text-primary-foreground disabled:opacity-60"
        >
          {busy ? "登录中…" : "登录"}
        </button>
        <p className="text-center text-xs text-muted-foreground">
          忘记密码？在服务器上执行 `cc-switch --web-reset-password` 重置。
        </p>
      </form>
    </div>
  );
}
