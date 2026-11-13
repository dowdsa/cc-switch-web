export async function trace(...args: unknown[]): Promise<void> {
  console.debug("[cc-switch-web]", ...args);
}

export async function debug(...args: unknown[]): Promise<void> {
  console.debug("[cc-switch-web]", ...args);
}

export async function info(...args: unknown[]): Promise<void> {
  console.info("[cc-switch-web]", ...args);
}

export async function warn(...args: unknown[]): Promise<void> {
  console.warn("[cc-switch-web]", ...args);
}

export async function error(...args: unknown[]): Promise<void> {
  console.error("[cc-switch-web]", ...args);
}
