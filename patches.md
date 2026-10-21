# Web 模式改动清单（用于上游合并）

桌面版构建（默认 feature）行为不变；Web 模式通过 `cargo build --features web` 启用。

## 修改的上游文件

| 文件 | 改动 |
|---|---|
| `src-tauri/Cargo.toml` | 新增 `web` feature 与可选依赖 `argon2`、`env_logger`；`tower-http/fs`、`axum/multipart` |
| `src-tauri/src/main.rs` | `--web` / `--web-reset-password` 分支，进入 `web::run()`（不初始化 Tauri） |
| `src-tauri/src/lib.rs` | 新增 `#[cfg(feature = "web")] pub mod web;` |
| `src-tauri/src/commands/provider.rs` | `claude_provider_models_are_claude_safe` 改为 `pub(crate)`，供 Web 端 Claude Desktop 导入复用 |
| `README.md` | 根 README 改为英文默认版（含“中文 README”按钮与上游说明） |
| `docs/README.zh-CN.md` | 删除上游版本后重新创建为本项目中文 README（非官方文档，避免 Gitee 中文环境抢占根 README） |
| `README_DE.md` / `README_JA.md` | 删除上游多语言 README（含赞助商内容） |
| `.github/workflows/` / `.github/actions/` | 删除上游 CI/发布/定时工作流及其配套 actions（不适用于 Web 仓库） |
| `.github/dependabot.yml` | 删除上游依赖自动升级配置 |
| `.github/labeler.yml` / `CODEOWNERS` / `FUNDING.yml` | 删除上游维护与赞助配置 |
| `src/main.tsx` | 引入 `WebGate`；Web 未登录时跳过后端启动请求；启动渲染抽成 `renderApp()` |
| `src/components/shell/Sidebar.tsx` | 侧栏底部挂载 Web 账号入口（`WebSidebarAccount`） |
| `src/components/shell/WindowControls.tsx` | 非 Tauri 环境返回 `null` |
| `package.json` | 新增 `dev:web` / `build:web` 脚本 |
| `.gitignore` | 忽略 `dist-web/` |

## 新增文件（与上游隔离）

- `src-tauri/src/web/*`：axum 服务、认证（独立 `auth.db`）、SSE 事件总线、RPC 桥（覆盖前端全部 327 个命令）、上传/SQL 导出、会话 NDJSON 流与图片二进制。
- `src/web/*`：WebGate / 登录 / 初始化 / 改密界面，以及 `@tauri-apps/*` 浏览器 shim（Vite alias 注入）。
- `vite.config.web.ts`：Web 构建（`dist-web/`），仅 Web 构建注入 favicon。
- `public-web/`：浏览器页签图标（`favicon.svg` / `favicon.ico` / `apple-touch-icon.png`，取自 CC 应用图标）。
- `scripts/web-parity.mjs`：对比前端 `invoke` 命令与 Web RPC/覆盖实现。
- `scripts/sync-upstream.sh`：上游版本同步（fetch、合并、校验）。
- `scripts/publish-repos.sh`：把当前工作树发布为 Gitee / GitHub 单提交快照，并过滤上游 CI/Dependabot 配置。
- `deploy/install.sh`：一键构建、安装 systemd 服务并启动。
- `deploy/*`：systemd 与 Nginx 示例。
- `DEPLOY.md`：部署、升级、同步上游说明。

## 合并上游后的自检

```bash
pnpm exec tsc --noEmit
pnpm exec vite build --config vite.config.web.ts
cd src-tauri && cargo check && cargo check --features web
node ../scripts/web-parity.mjs   # 必须 0 未覆盖
```

上游新增命令会出现在 parity 的未覆盖列表里；为它补上 `src-tauri/src/web/rpc.rs` 中的分支或 `src/web/shims/overrides.ts` 的浏览器替代实现即可。

## Web 模式已知差异

- 单管理员账号；默认仅监听回环地址，HTTPS 由反向代理提供。
- 托盘、自动更新、开机自启、窗口控件、系统文件对话框、终端启动等桌面能力被隐藏或替换（终端命令改为复制到剪贴板）。
- WebDAV/S3 下载恢复后不会自动执行桌面版的“导入后 live 投影”，建议随后手动切换/同步一次供应商或重启服务。
- `reset_circuit_breaker` 不执行桌面版的“恢复后自动切回更高优先级供应商”。
- 深链接的系统协议注册不可用；解析/合并/导入命令仍可通过界面或 API 使用。
- `import_prompt_from_file` 依赖原生文件对话框，Web 端不可用；SQL 备份已支持浏览器上传/下载。
