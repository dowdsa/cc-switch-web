export async function exit(code?: number): Promise<void> {
  console.warn(`[cc-switch-web] exit(${code ?? 0}) 在 Web 模式下不会结束服务进程`);
}

export async function relaunch(): Promise<void> {
  window.location.reload();
}
