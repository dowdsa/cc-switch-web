import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { isTauri } from "@tauri-apps/api/core";
import { ChevronUp, KeyRound, LogOut, UserRound } from "lucide-react";
import { HoverTip } from "@/components/ui/hover-tip";
import { cn } from "@/lib/utils";
import { ChangePasswordDialog } from "./ChangePasswordDialog";
import { getAuthStatus, postJson } from "./api";

/**
 * Web 模式账号入口：固定在左侧栏最底部，不使用悬浮按钮。
 * 桌面模式返回 null（由桌面端自己的 UI 处理）。
 */
export function WebSidebarAccount({ collapsed }: { collapsed: boolean }) {
  const [username, setUsername] = useState<string | null>(null);
  const [open, setOpen] = useState(false);
  const [passwordOpen, setPasswordOpen] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (isTauri()) return;
    let cancelled = false;
    void getAuthStatus()
      .then((status) => {
        if (!cancelled && status.authenticated) {
          setUsername(status.username?.trim() || "admin");
        }
      })
      .catch(() => {
        /* 未登录时 WebGate 会接管，不显示账号入口 */
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open]);

  if (isTauri() || !username) return null;

  const initial = username.slice(0, 1).toUpperCase();
  const display = `账户 · ${username}`;

  async function handleLogout() {
    setBusy(true);
    try {
      await postJson("/api/auth/logout", {});
    } catch {
      /* 即使请求失败也回到登录页 */
    } finally {
      window.location.reload();
    }
  }

  const trigger = collapsed ? (
    <HoverTip content={display} side="right">
      <button
        type="button"
        aria-label={display}
        aria-haspopup="dialog"
        onClick={() => setOpen(true)}
        className="flex h-9 w-full items-center justify-center rounded-control text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1"
      >
        <span className="flex h-6 w-6 items-center justify-center rounded-full bg-subtle text-caption font-medium text-fg-1">
          {initial}
        </span>
      </button>
    </HoverTip>
  ) : (
    <button
      type="button"
      aria-haspopup="dialog"
      aria-label={display}
      onClick={() => setOpen(true)}
      className="flex h-10 w-full items-center gap-2 rounded-control px-2 text-start text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1"
    >
      <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-subtle text-caption font-medium text-fg-1">
        {initial}
      </span>
      <span className="min-w-0 flex-1 truncate text-body">{username}</span>
      <span className="shrink-0 text-caption text-fg-3">Web 账户</span>
      <ChevronUp className="h-4 w-4 shrink-0 text-fg-3" strokeWidth={1.5} />
    </button>
  );

  const dialog = open
    ? createPortal(
        <div
          className="fixed inset-0 z-[10000] flex items-center justify-center bg-black/40 px-4"
          onClick={() => setOpen(false)}
        >
          <div
            role="dialog"
            aria-modal="true"
            aria-label="账户"
            className="w-full max-w-xs space-y-1 rounded-xl border bg-card p-3 text-card-foreground shadow-lg"
            onClick={(event) => event.stopPropagation()}
          >
            <div className="flex items-center gap-2.5 px-2 py-2">
              <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-subtle text-sm font-medium">
                {initial}
              </span>
              <div className="min-w-0">
                <div className="truncate text-sm font-medium">{username}</div>
                <div className="text-xs text-fg-3">Web 管理员账户</div>
              </div>
            </div>
            <button
              type="button"
              onClick={() => {
                setOpen(false);
                setPasswordOpen(true);
              }}
              className="flex w-full items-center gap-2 rounded-md px-2 py-2 text-sm hover:bg-accent"
            >
              <KeyRound className="h-4 w-4" strokeWidth={1.5} />
              修改密码
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => void handleLogout()}
              className="flex w-full items-center gap-2 rounded-md px-2 py-2 text-sm text-destructive hover:bg-accent disabled:opacity-60"
            >
              <LogOut className="h-4 w-4" strokeWidth={1.5} />
              {busy ? "退出中…" : "退出登录"}
            </button>
            <div className="flex justify-end pt-1">
              <button
                type="button"
                onClick={() => setOpen(false)}
                className="rounded-md border px-2.5 py-1 text-xs text-fg-2 hover:bg-subtle"
              >
                取消
              </button>
            </div>
          </div>
        </div>,
        document.body,
      )
    : null;

  return (
    <div
      className={cn(
        "shrink-0 border-t border-border px-2 pb-2 pt-1.5",
      )}
    >
      <div className="mb-1 flex items-center gap-1 px-2 text-caption text-fg-3">
        <UserRound className="h-3.5 w-3.5" strokeWidth={1.5} />
        {!collapsed && <span>账号</span>}
      </div>
      {trigger}
      {dialog}
      {passwordOpen
        ? createPortal(
            <ChangePasswordDialog
              open={passwordOpen}
              onClose={() => setPasswordOpen(false)}
            />,
            document.body,
          )
        : null}
    </div>
  );
}
