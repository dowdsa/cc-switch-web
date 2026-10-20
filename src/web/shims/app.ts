let cachedVersion: string | null = null;

async function fetchMeta(): Promise<{ version?: string } | null> {
  try {
    const response = await fetch("/api/meta", { credentials: "same-origin" });
    if (!response.ok) return null;
    return (await response.json()) as { version?: string };
  } catch {
    return null;
  }
}

export async function getVersion(): Promise<string> {
  if (cachedVersion) return cachedVersion;
  const meta = await fetchMeta();
  cachedVersion = meta?.version ?? "web";
  return cachedVersion;
}

export async function getName(): Promise<string> {
  return "CC Switch Web";
}

export async function getTauriVersion(): Promise<string> {
  return "web";
}

export async function getIdentifier(): Promise<string> {
  return "com.ccswitch.web";
}
