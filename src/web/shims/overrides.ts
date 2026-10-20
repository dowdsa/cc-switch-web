/** 桌面专属命令在浏览器中的等价实现（或安全的空操作）。 */
export type OverrideResult = { handled: boolean; value?: unknown };

const NOT_HANDLED: OverrideResult = { handled: false };

function textArg(args: Record<string, unknown>, ...keys: string[]): string {
  for (const key of keys) {
    const value = args[key];
    if (typeof value === "string" && value.length > 0) return value;
  }
  return "";
}

function pickFile(accept: string): Promise<File | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = accept;
    input.style.display = "none";
    input.addEventListener("change", () => {
      const file = input.files?.[0] ?? null;
      input.remove();
      resolve(file);
    });
    input.addEventListener("cancel", () => {
      input.remove();
      resolve(null);
    });
    document.body.appendChild(input);
    input.click();
  });
}

async function uploadFile(file: File): Promise<string> {
  const form = new FormData();
  form.append("file", file);
  const response = await fetch("/api/upload", {
    method: "POST",
    credentials: "same-origin",
    headers: { "X-CC-Switch-Web": "1" },
    body: form,
  });
  if (!response.ok) {
    const data = (await response.json().catch(() => null)) as {
      error?: { message?: string };
    } | null;
    throw new Error(data?.error?.message ?? `上传失败 (HTTP ${response.status})`);
  }
  const data = (await response.json()) as { path: string };
  return data.path;
}

function downloadBlob(filename: string, blob: Blob): void {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  URL.revokeObjectURL(url);
}

export async function maybeOverride(
  cmd: string,
  args: Record<string, unknown>,
): Promise<OverrideResult> {
  switch (cmd) {
    case "open_external": {
      const url = textArg(args, "url");
      if (url) window.open(url, "_blank", "noopener,noreferrer");
      return { handled: true };
    }
    case "copy_text_to_clipboard": {
      const text = textArg(args, "text", "content", "value");
      const { copyText } = await import("../polyfills");
      await copyText(text);
      return { handled: true };
    }
    case "peek_pending_deeplink":
      return { handled: true, value: null };
    case "ack_pending_deeplink":
      return { handled: true };
    case "update_tray_menu":
    case "open_app_config_folder":
    case "open_config_folder":
    case "open_cc_switch_skills_dir":
    case "open_workspace_directory":
    case "reveal_app_log_file":
    case "reveal_session_path":
    case "reveal_backup_location":
    case "take_tray_navigation":
    case "tray_panel_state":
    case "tray_panel_placement":
    case "tray_panel_layout":
    case "tray_panel_hide":
    case "tray_panel_action":
    case "tray_app_page_seen":
    case "set_window_theme":
      return { handled: true };
    case "open_provider_terminal":
    case "launch_session_terminal": {
      const command = textArg(args, "command", "providerId", "sessionId");
      if (command) {
        const { copyText } = await import("../polyfills");
        await copyText(command);
      }
      return { handled: true, value: true };
    }
    case "export_session_markdown": {
      const content = textArg(args, "content");
      const defaultName = textArg(args, "defaultName") || "session.md";
      const name = defaultName.endsWith(".md")
        ? defaultName
        : `${defaultName}.md`;
      const blob = new Blob([content], {
        type: "text/markdown;charset=utf-8",
      });
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = name;
      anchor.click();
      URL.revokeObjectURL(url);
      return { handled: true, value: name };
    }
    case "get_app_config_dir_override":
      return { handled: true, value: null };
    case "set_app_config_dir_override":
      return { handled: true, value: false };
    case "open_hermes_web_ui":
    case "launch_hermes_dashboard":
      return { handled: true, value: false };
    case "save_file_dialog":
      return { handled: true, value: "__browser_download__" };
    case "open_file_dialog": {
      const file = await pickFile(".sql,.db");
      if (!file) return { handled: true, value: null };
      return { handled: true, value: await uploadFile(file) };
    }
    case "open_zip_file_dialog": {
      const file = await pickFile(".zip,.skill");
      if (!file) return { handled: true, value: null };
      return { handled: true, value: await uploadFile(file) };
    }
    case "export_config_to_file": {
      const response = await fetch("/api/export/sql", {
        credentials: "same-origin",
      });
      if (!response.ok) throw new Error(`导出配置失败 (HTTP ${response.status})`);
      downloadBlob("cc-switch-backup.sql", await response.blob());
      return {
        handled: true,
        value: {
          success: true,
          message: "SQL exported successfully",
          filePath: "cc-switch-backup.sql",
        },
      };
    }
    case "pick_directory":
    case "import_prompt_from_file":
      return { handled: true, value: null };
    case "check_app_update_available":
    case "check_for_updates":
      return { handled: true, value: null };
    case "install_update_and_restart":
    case "open_portable_update_download":
    case "restart_app":
      return { handled: true, value: false };
    case "get_auto_launch_status":
      return { handled: true, value: { enabled: false, supported: false } };
    case "set_auto_launch":
      return { handled: true, value: false };
    default:
      return NOT_HANDLED;
  }
}
