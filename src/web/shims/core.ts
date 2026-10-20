import { maybeOverride } from "./overrides";

const JSON_HEADERS = {
  "Content-Type": "application/json",
  "X-CC-Switch-Web": "1",
};

let channelSequence = 0;

export class Channel<T = unknown> {
  readonly id: string;
  onmessage: (message: T) => void = () => {};

  constructor(id?: string) {
    this.id = id ?? `ch-${Date.now()}-${++channelSequence}`;
  }

  toJSON(): string {
    return this.id;
  }
}

export function isTauri(): boolean {
  return false;
}

function findChannel(value: unknown, depth = 0): Channel<unknown> | null {
  if (value instanceof Channel) return value;
  if (depth > 6 || value === null || typeof value !== "object") return null;
  if (Array.isArray(value)) {
    for (const item of value) {
      const found = findChannel(item, depth + 1);
      if (found) return found;
    }
    return null;
  }
  for (const item of Object.values(value as Record<string, unknown>)) {
    const found = findChannel(item, depth + 1);
    if (found) return found;
  }
  return null;
}

function notifyUnauthorized() {
  window.dispatchEvent(new Event("cc:unauthorized"));
}

async function throwResponseError(response: Response, command: string): Promise<never> {
  if (response.status === 401) {
    notifyUnauthorized();
  }
  let message = `命令 ${command} 失败 (HTTP ${response.status})`;
  try {
    const data = (await response.json()) as {
      error?: { message?: string };
    };
    if (data?.error?.message) message = data.error.message;
  } catch {
    /* 非 JSON 响应 */
  }
  throw new Error(message);
}

async function streamInvoke<T>(
  command: string,
  args: Record<string, unknown>,
  channel: Channel<unknown>,
): Promise<T> {
  const response = await fetch("/api/rpc/stream", {
    method: "POST",
    credentials: "same-origin",
    headers: JSON_HEADERS,
    body: JSON.stringify({ cmd: command, args, channelId: channel.id }),
  });
  if (!response.ok) {
    return throwResponseError(response, command);
  }
  const reader = response.body?.getReader();
  if (!reader) throw new Error("当前浏览器不支持流式响应");

  const decoder = new TextDecoder();
  let buffer = "";
  let result: unknown;

  for (;;) {
    const { value, done } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    let newline = buffer.indexOf("\n");
    while (newline >= 0) {
      const line = buffer.slice(0, newline).trim();
      buffer = buffer.slice(newline + 1);
      newline = buffer.indexOf("\n");
      if (!line) continue;
      const message = JSON.parse(line) as {
        type: string;
        message?: unknown;
        result?: unknown;
        error?: string;
      };
      if (message.type === "message") {
        channel.onmessage(message.message);
      } else if (message.type === "done") {
        result = message.result;
      } else if (message.type === "error") {
        throw new Error(message.error ?? "流式命令执行失败");
      }
    }
  }
  return result as T;
}

export async function invoke<T>(
  command: string,
  args: Record<string, unknown> = {},
  _options?: unknown,
): Promise<T> {
  const override = await maybeOverride(command, args);
  if (override.handled) {
    return override.value as T;
  }

  const channel = findChannel(args);
  if (channel) {
    return streamInvoke<T>(command, args, channel);
  }

  const response = await fetch("/api/rpc", {
    method: "POST",
    credentials: "same-origin",
    headers: JSON_HEADERS,
    body: JSON.stringify({ cmd: command, args }),
  });
  if (!response.ok) {
    return throwResponseError(response, command);
  }

  const contentType = response.headers.get("content-type") ?? "";
  if (contentType.includes("application/octet-stream")) {
    return (await response.arrayBuffer()) as T;
  }
  const text = await response.text();
  if (!text) return undefined as T;
  return JSON.parse(text) as T;
}
