import { useState, type FormEvent } from "react";
import { postJson } from "./api";

interface SetupPageProps {
  onSuccess: () => void;
}

export function SetupPage({ onSuccess }: SetupPageProps) {
  const [token, setToken] = useState("");
  const [username, setUsername] = useState("admin");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    setError(null);
    if (password.length < 8) {
      setError("密码至少需要 8 个字符");
      return;
    }
    if (password !== confirm) {
      setError("两次输入的密码不一致");
      return;
    }
    setBusy(true);
    try {
      await postJson("/api/auth/setup", { token, username, password });
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
        className="w-full max-w-md space-y-4 rounded-xl border bg-card p-6 shadow-sm"
      >
        <div className="space-y-1 text-center">
          <h1 className="text-xl font-semibold">初始化 CC Switch Web</h1>
          <p className="text-sm text-muted-foreground">
            首次使用：输入服务端启动日志中的初始化令牌，创建管理员账号。
          </p>
        </div>
        <label className="block space-y-1 text-sm">
          <span className="text-muted-foreground">初始化令牌</span>
          <input
            className="w-full rounded-md border bg-background px-3 py-2 font-mono text-xs outline-none focus:ring-2 focus:ring-ring"
            value={token}
            onChange={(event) => setToken(event.target.value)}
            required
          />
        </label>
        <label className="block space-y-1 text-sm">
          <span className="text-muted-foreground">管理员用户名</span>
          <input
            className="w-full rounded-md border bg-background px-3 py-2 outline-none focus:ring-2 focus:ring-ring"
            value={username}
            autoComplete="username"
            onChange={(event) => setUsername(event.target.value)}
            required
          />
        </label>
        <label className="block space-y-1 text-sm">
          <span className="text-muted-foreground">密码（至少 8 位）</span>
          <input
            type="password"
            className="w-full rounded-md border bg-background px-3 py-2 outline-none focus:ring-2 focus:ring-ring"
            value={password}
            autoComplete="new-password"
            onChange={(event) => setPassword(event.target.value)}
            required
          />
        </label>
        <label className="block space-y-1 text-sm">
          <span className="text-muted-foreground">确认密码</span>
          <input
            type="password"
            className="w-full rounded-md border bg-background px-3 py-2 outline-none focus:ring-2 focus:ring-ring"
            value={confirm}
            autoComplete="new-password"
            onChange={(event) => setConfirm(event.target.value)}
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
          {busy ? "创建中…" : "创建管理员并进入"}
        </button>
      </form>
    </div>
  );
}
