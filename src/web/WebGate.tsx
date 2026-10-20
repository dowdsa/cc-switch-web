import {
  useCallback,
  useEffect,
  useState,
  type ReactNode,
} from "react";
import { isTauri } from "@tauri-apps/api/core";
import { getAuthStatus, postJson, type WebAuthStatus } from "./api";
import { ChangePasswordDialog } from "./ChangePasswordDialog";
import { LoginPage } from "./LoginPage";
import { SetupPage } from "./SetupPage";

export function WebGate({ children }: { children: ReactNode }) {
  if (isTauri()) {
    return <>{children}</>;
  }
  return <BrowserGate>{children}</BrowserGate>;
}

function BrowserGate({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<WebAuthStatus | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const next = await getAuthStatus();
      setStatus(next);
      setLoadError(null);
    } catch (error) {
      setLoadError(error instanceof Error ? error.message : String(error));
      setStatus({ needsSetup: false, authenticated: false });
    }
  }, []);

  useEffect(() => {
    void refresh();
    const onUnauthorized = () => {
      setStatus((previous) => ({
        needsSetup: previous?.needsSetup ?? false,
        authenticated: false,
        username: previous?.username ?? null,
      }));
    };
    window.addEventListener("cc:unauthorized", onUnauthorized);
    return () => {
      window.removeEventListener("cc:unauthorized", onUnauthorized);
    };
  }, [refresh]);

  if (status === null) {
    return (
      <div className="flex min-h-screen items-center justify-center bg-background text-sm text-muted-foreground">
        正在连接 CC Switch Web…
      </div>
    );
  }

  if (status.needsSetup) {
    return <SetupPage onSuccess={() => window.location.reload()} />;
  }

  if (!status.authenticated) {
    return (
      <>
        {loadError ? (
          <div className="fixed inset-x-0 top-0 z-50 bg-destructive/90 px-4 py-2 text-center text-xs text-destructive-foreground">
            {loadError}
          </div>
        ) : null}
        <LoginPage onSuccess={() => window.location.reload()} />
      </>
    );
  }

  return (
    <>
      {children}
      <AccountMenu
        username={status.username ?? ""}
        onSignedOut={() => window.location.reload()}
      />
    </>
  );
}

function AccountMenu({
  username,
  onSignedOut,
}: {
  username: string;
  onSignedOut: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [passwordOpen, setPasswordOpen] = useState(false);
  const [busy, setBusy] = useState(false);

  async function handleLogout() {
    setBusy(true);
    try {
      await postJson("/api/auth/logout", {});
    } catch {
      /* 即使请求失败也回到登录页 */
    } finally {
      onSignedOut();
    }
  }

  return (
    <div className="fixed bottom-3 right-3 z-[9999] flex flex-col items-end gap-2">
      {open ? (
        <div className="w-56 rounded-lg border bg-card p-2 text-sm text-card-foreground shadow-lg">
          <div className="truncate px-2 py-1 text-xs text-muted-foreground">
            {username}
          </div>
          <button
            type="button"
            className="w-full rounded-md px-2 py-1.5 text-left hover:bg-accent"
            onClick={() => {
              setPasswordOpen(true);
              setOpen(false);
            }}
          >
            修改密码
          </button>
          <button
            type="button"
            disabled={busy}
            className="w-full rounded-md px-2 py-1.5 text-left text-destructive hover:bg-accent disabled:opacity-60"
            onClick={() => void handleLogout()}
          >
            退出登录
          </button>
        </div>
      ) : null}
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        className="rounded-full border bg-card px-3 py-1.5 text-xs text-card-foreground shadow-sm hover:bg-accent"
        title="Web 版账号"
      >
        {open ? "关闭" : `账户 · ${username}`}
      </button>
      <ChangePasswordDialog
        open={passwordOpen}
        onClose={() => setPasswordOpen(false)}
      />
    </div>
  );
}
