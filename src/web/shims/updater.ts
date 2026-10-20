export interface Update {
  version: string;
  currentVersion: string;
  body?: string;
  date?: string;
}

/** Web 版由服务器部署升级，不自更新。 */
export async function check(): Promise<null> {
  return null;
}
