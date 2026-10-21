const JSON_HEADERS = {
  "Content-Type": "application/json",
  "X-CC-Switch-Web": "1",
};

export interface WebAuthStatus {
  needsSetup: boolean;
  authenticated: boolean;
  username?: string | null;
}

export async function getAuthStatus(): Promise<WebAuthStatus> {
  const response = await fetch("/api/auth/status", {
    credentials: "same-origin",
  });
  if (!response.ok) {
    throw new Error(`无法获取登录状态 (HTTP ${response.status})`);
  }
  return (await response.json()) as WebAuthStatus;
}

export async function postJson<T>(
  path: string,
  body: unknown,
): Promise<T> {
  const response = await fetch(path, {
    method: "POST",
    credentials: "same-origin",
    headers: JSON_HEADERS,
    body: JSON.stringify(body ?? {}),
  });
  let data: unknown = null;
  try {
    data = await response.json();
  } catch {
    /* 非 JSON 响应 */
  }
  if (!response.ok) {
    const message =
      (data as { error?: { message?: string } } | null)?.error?.message ??
      `请求失败 (HTTP ${response.status})`;
    throw new Error(message);
  }
  return data as T;
}
