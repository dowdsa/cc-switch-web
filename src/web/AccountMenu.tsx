import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { isTauri } from "@tauri-apps/api/core";
import { ChevronUp, KeyRound, LogOut, UserRound } from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { ChangePasswordDialog } from "./ChangePasswordDialog";
import { getAuthStatus, postJson } from "./api";

/**
 * Web 模式账号入口：固定在左侧栏最底部，点击后向上弹出账户菜单。
 * 桌面模式返回 null（由桌面端自己的 UI 处理）。
 */
export function WebSidebarAccount({ collapsed }: { collapsed: boolean }) {
  const [username, setUsername] = useState<string | null>(null);
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
    <button
      type="button"
      title={display}
      aria-label={display}
      className="flex h-9 w-full items-center justify-center rounded-control text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1 data-[state=open]:bg-subtle"
    >
      <span className="flex h-6 w-6 items-center justify-center rounded-full bg-subtle text-caption font-medium text-fg-1">
        {initial}
      </span>
    </button>
  ) : (
    <button
      type="button"
      aria-label={display}
      className="flex h-10 w-full items-center gap-2 rounded-control px-2 text-start text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1 data-[state=open]:bg-subtle"
    >
      <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-subtle text-caption font-medium text-fg-1">
        {initial}
      </span>
      <span className="min-w-0 flex-1 truncate text-body">{username}</span>
      <span className="shrink-0 text-caption text-fg-3">Web 账户</span>
      <ChevronUp className="h-4 w-4 shrink-0 text-fg-3" strokeWidth={1.5} />
    </button>
  );

  return (
    <div className="shrink-0 border-t border-border px-2 pb-2 pt-1.5">
      <div className="mb-1 flex items-center gap-1 px-2 text-caption text-fg-3">
        <UserRound className="h-3.5 w-3.5" strokeWidth={1.5} />
        {!collapsed && <span>账号</span>}
      </div>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>{trigger}</DropdownMenuTrigger>
        <DropdownMenuContent
          side="top"
          align="start"
          sideOffset={8}
          className="w-56 border-border bg-card p-1.5"
        >
          <DropdownMenuLabel className="px-2 py-2 font-normal">
            <div className="flex items-center gap-2.5">
              <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-subtle text-sm font-medium">
                {initial}
              </span>
              <div className="min-w-0">
                <div className="truncate text-sm font-medium">{username}</div>
                <div className="text-xs text-fg-3">Web 管理员账户</div>
              </div>
            </div>
          </DropdownMenuLabel>
          <DropdownMenuSeparator />
          <DropdownMenuItem
            className="gap-2"
            onSelect={() => setPasswordOpen(true)}
          >
            <KeyRound className="h-4 w-4" strokeWidth={1.5} />
            修改密码
          </DropdownMenuItem>
          <DropdownMenuItem
            disabled={busy}
            className="gap-2 text-destructive focus:text-destructive"
            onSelect={() => void handleLogout()}
          >
            <LogOut className="h-4 w-4" strokeWidth={1.5} />
            {busy ? "退出中…" : "退出登录"}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
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
