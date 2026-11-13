/**
 * Web 模式补丁：内网 HTTP 访问属于非安全上下文（isSecureContext=false），
 * 浏览器不提供 crypto.randomUUID 与 navigator.clipboard，桌面版代码里的
 * 直接调用会在渲染时抛异常。这里用可在非安全上下文使用的 API 补齐。
 */

function randomBytes(length: number): Uint8Array {
  const bytes = new Uint8Array(length);
  const cryptoObj = globalThis.crypto;
  if (cryptoObj && typeof cryptoObj.getRandomValues === "function") {
    cryptoObj.getRandomValues(bytes);
    return bytes;
  }
  for (let i = 0; i < length; i += 1) {
    bytes[i] = Math.floor(Math.random() * 256);
  }
  return bytes;
}

function uuidV4(): string {
  const bytes = randomBytes(16);
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

function installCryptoRandomUuid(): void {
  const cryptoObj = globalThis.crypto as Crypto | undefined;
  if (!cryptoObj || typeof cryptoObj.randomUUID === "function") return;
  try {
    Object.defineProperty(cryptoObj, "randomUUID", {
      configurable: true,
      writable: true,
      value: uuidV4,
    });
  } catch {
    /* 个别浏览器不允许扩展 crypto，保持原状 */
  }
}

function copyWithTextarea(text: string): boolean {
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.top = "-9999px";
  textarea.style.left = "-9999px";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  const selection = document.getSelection();
  const previous =
    selection && selection.rangeCount > 0 ? selection.getRangeAt(0) : null;
  textarea.select();
  let copied = false;
  try {
    copied = document.execCommand("copy");
  } catch {
    copied = false;
  }
  textarea.remove();
  if (selection && previous) {
    selection.removeAllRanges();
    selection.addRange(previous);
  }
  return copied;
}

function installClipboardFallback(): void {
  const nav = navigator as Navigator & { clipboard?: Clipboard };
  if (nav.clipboard && typeof nav.clipboard.writeText === "function") return;
  const fallback = {
    writeText: async (text: string) => {
      if (!copyWithTextarea(text)) {
        throw new Error("当前浏览器不允许写入剪贴板");
      }
    },
  };
  try {
    Object.defineProperty(nav, "clipboard", {
      configurable: true,
      value: fallback as unknown as Clipboard,
    });
  } catch {
    /* ignore */
  }
}

export async function copyText(text: string): Promise<boolean> {
  const clipboard = navigator.clipboard;
  if (clipboard && typeof clipboard.writeText === "function") {
    try {
      await clipboard.writeText(text);
      return true;
    } catch {
      /* 权限或安全上下文限制，走兜底 */
    }
  }
  return copyWithTextarea(text);
}

installCryptoRandomUuid();
installClipboardFallback();
