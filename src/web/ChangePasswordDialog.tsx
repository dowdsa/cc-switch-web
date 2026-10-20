import { useState, type FormEvent } from "react";
import { toast } from "sonner";
import { postJson } from "./api";

interface ChangePasswordDialogProps {
  open: boolean;
  onClose: () => void;
}

export function ChangePasswordDialog({
  open,
  onClose,
}: ChangePasswordDialogProps) {
  const [currentPassword, setCurrentPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  if (!open) return null;

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    setError(null);
    if (newPassword.length < 8) {
      setError("新密码至少需要 8 个字符");
      return;
    }
    if (newPassword !== confirm) {
      setError("两次输入的新密码不一致");
      return;
    }
    setBusy(true);
    try {
      await postJson("/api/auth/change-password", {
        currentPassword,
        newPassword,
      });
      toast.success("密码已修改，其他已登录会话已失效");
      setCurrentPassword("");
      setNewPassword("");
      setConfirm("");
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="fixed inset-0 z-[10000] flex items-center justify-center bg-black/40 px-4">
      <form
        onSubmit={handleSubmit}
        className="w-full max-w-sm space-y-3 rounded-xl border bg-card p-5 text-foreground shadow-lg"
      >
        <h2 className="text-base font-semibold">修改密码</h2>
        <input
          type="password"
          placeholder="当前密码"
          autoComplete="current-password"
          className="w-full rounded-md border bg-background px-3 py-2 text-sm outline-none focus:ring-2 focus:ring-ring"
          value={currentPassword}
          onChange={(event) => setCurrentPassword(event.target.value)}
          required
        />
        <input
          type="password"
          placeholder="新密码（至少 8 位）"
          autoComplete="new-password"
          className="w-full rounded-md border bg-background px-3 py-2 text-sm outline-none focus:ring-2 focus:ring-ring"
          value={newPassword}
          onChange={(event) => setNewPassword(event.target.value)}
          required
        />
        <input
          type="password"
          placeholder="确认新密码"
          autoComplete="new-password"
          className="w-full rounded-md border bg-background px-3 py-2 text-sm outline-none focus:ring-2 focus:ring-ring"
          value={confirm}
          onChange={(event) => setConfirm(event.target.value)}
          required
        />
        {error ? <p className="text-sm text-destructive">{error}</p> : null}
        <div className="flex justify-end gap-2 pt-1">
          <button
            type="button"
            onClick={onClose}
            className="rounded-md border px-3 py-1.5 text-sm"
          >
            取消
          </button>
          <button
            type="submit"
            disabled={busy}
            className="rounded-md bg-primary px-3 py-1.5 text-sm text-primary-foreground disabled:opacity-60"
          >
            {busy ? "保存中…" : "保存"}
          </button>
        </div>
      </form>
    </div>
  );
}
