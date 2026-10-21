import {
  useCallback,
  useEffect,
  useState,
  type ReactNode,
} from "react";
import { isTauri } from "@tauri-apps/api/core";
import { getAuthStatus, type WebAuthStatus } from "./api";
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

  return <>{children}</>;
}
