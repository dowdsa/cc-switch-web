let cachedHome: string | null = null;

export async function homeDir(): Promise<string> {
  if (cachedHome !== null) return cachedHome;
  try {
    const response = await fetch("/api/meta", { credentials: "same-origin" });
    if (response.ok) {
      const meta = (await response.json()) as { homeDir?: string };
      if (meta.homeDir) {
        cachedHome = meta.homeDir;
        return cachedHome;
      }
    }
  } catch {
    /* ignore */
  }
  return "/";
}

export async function join(...segments: string[]): Promise<string> {
  const joined = segments
    .filter((segment) => segment.length > 0)
    .join("/")
    .replace(/\/{2,}/g, "/");
  return joined || "/";
}

export async function sep(): Promise<string> {
  return "/";
}

export async function appDataDir(): Promise<string> {
  return `${await homeDir()}/.cc-switch`;
}

export async function appConfigDir(): Promise<string> {
  return `${await homeDir()}/.cc-switch`;
}

export async function basename(path: string): Promise<string> {
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] ?? "";
}

export async function dirname(path: string): Promise<string> {
  const index = path.lastIndexOf("/");
  return index <= 0 ? "/" : path.slice(0, index);
}
