export async function message(content: string): Promise<void> {
  window.alert(content);
}

export async function ask(content: string): Promise<boolean> {
  return window.confirm(content);
}

export async function confirm(content: string): Promise<boolean> {
  return window.confirm(content);
}

export async function open(): Promise<string | null> {
  return null;
}

export async function save(): Promise<string | null> {
  return null;
}
