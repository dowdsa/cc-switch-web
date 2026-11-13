# Web 模式改动清单（用于上游合并）

桌面版构建（默认 feature）行为不变；Web 模式通过 `cargo build --features web` 启用。
上游基线：**v4.0.31**（2026-11 同步；v4.0.23 → v4.0.31 共 229 个上游提交）。

## 本次同步摘要（v4.0.23 → v4.0.31）

- **新应用**：WorkBuddy（官方账号收录/切换、models.json 占用查询、套餐与积分额度）。
- **授权中心**：WorkBuddy 账号 provider（`workbuddy_account`），Copilot CLI 当前登录账号关联（`copilot_cli_signed_in_account`）；Claude 官方登录新增 `open_browser` 参数（Web 端固定 `false`，不弹浏览器）。
- **MCP 连通性检查**：上游改为 `start_mcp_connectivity_checks`（Channel 回调）+ `cancel_mcp_connectivity_checks`；Web 端经 `/api/rpc/stream` NDJSON 流实现（见 `rpc_stream_mcp_checks`）。
- **会话正文搜索**：`sync_session_index` / `search_session_content` / `clear_session_index`，索引进度经 SSE `session-index-status` 事件推送。
- **隐私过滤器**：`get_privacy_filter_config` / `set_privacy_filter_config`（开关打开时后台预热规则）。
- **用量**：`fetch_models_dev`（后端代理拉取定价）、`fetch_exchange_rates`（汇率换算）。
- **工具安装/升级**：上游 `run_tool_lifecycle_action` 改为需要 `AppHandle`；Web 端用 `ToolLifecycleProgress::with_sink` + SSE `tool-lifecycle-progress` 事件等价实现（`run_tool_lifecycle_action_web`），`misc.rs` 相关内部函数放宽为 `pub(crate)`。
- **其余**：Skills「立即重新同步」（`refresh_local_skill_metadata`）、`get_config_status`（新增 Copilot / WorkBuddy 分支）、Gemini Native 流式用量与系统消息修复、Copilot Responses 优化、Skills 符号链接错误信息等；详见 `docs/release-notes/v4.0.24` ～ `v4.0.31`。

## 上一次同步摘要（v4.0.16 → v4.0.23）

- **新应用**：Antigravity CLI（agy）、Kimi Code、DeepSeek Harness、Oh My Pi（供应商、MCP、Skills、提示词、会话、用量按上游支持范围）。
- **授权中心**：Claude 官方多账号与切卡切号、Antigravity 多账号（macOS）、Kimi 官方账号、DeepSeek 账号；Codex 新增浏览器登录（Web 端仍建议设备码登录）。
- **供应商与路由**：Kimi / MiniMax Code / Oh My Pi 原生格式编辑器、供应商原生格式校验、Gemini 思考档位转发、Copilot Claude 原生缓存优化、Codex / agy / Claude 终端启动增强（Web 端受终端限制）、WebDAV 清除配置、Skills 打开目录/文档、发现仓库缓存等。
- **修复**：用量统计与额度查询、代理协议转换（Gemini/Responses/xAI）、MCP 批量操作、托盘与轻量模式、Windows 更新/退出、数据库启动锁等；详见 `docs/release-notes/v4.0.17` ～ `v4.0.23`。

## 修改的上游文件

| 文件 | 改动 |
|---|---|
| `src-tauri/Cargo.toml` | 新增 `web` feature 与可选依赖 `argon2`、`env_logger`；`tower-http/fs`、`axum/multipart` |
| `src-tauri/src/main.rs` | `--web` / `--web-reset-password` 分支，进入 `web::run()`（不初始化 Tauri） |
| `src-tauri/src/lib.rs` | 新增 `#[cfg(feature = "web")] pub mod web;` |
| `src-tauri/src/commands/provider.rs` | `claude_provider_models_are_claude_safe` 与 `query_provider_usage_inner` 改为 `pub(crate)`，供 Web 端 Claude Desktop 导入与额度查询复用 |
| `src-tauri/src/commands/auth.rs` | 授权中心的 `map_account` / `map_xai_account` / `map_agy_account` / `map_claude_account` 改为 `pub(crate)`，供 Web RPC 返回与桌面版一致的账号结构 |
| `README.md` | 根 README 改为英文默认版（含“中文 README”按钮与上游说明） |
| `docs/README.zh-CN.md` | 删除上游版本后重新创建为本项目中文 README（非官方文档，避免 Gitee 中文环境抢占根 README） |
| `README_DE.md` / `README_JA.md` | 删除上游多语言 README（含赞助商内容） |
| `.github/workflows/` / `.github/actions/` | 删除上游 CI/发布/定时工作流及其配套 actions（不适用于 Web 仓库） |
| `.github/dependabot.yml` | 删除上游依赖自动升级配置 |
| `.github/labeler.yml` / `CODEOWNERS` / `FUNDING.yml` | 删除上游维护与赞助配置 |
| `src/main.tsx` | 引入 `WebGate`；Web 未登录时跳过后端启动请求；启动渲染抽成 `renderApp()` |
| `tests/msw/tauriMocks.ts` | 在全局 `@tauri-apps/api/core` mock 中补充 `isTauri: () => true`，避免上游新增的 App 集成测试渲染 Web 侧栏时缺少导出 |
| `src/components/shell/Sidebar.tsx` | 侧栏底部挂载 Web 账号入口（`WebSidebarAccount`） |
| `src/components/shell/WindowControls.tsx` | 非 Tauri 环境返回 `null` |
| `package.json` | 新增 `dev:web` / `build:web` 脚本 |
| `.gitignore` | 忽略 `dist-web/` |

## 新增文件（与上游隔离）

- `src-tauri/src/web/*`：axum 服务、认证（独立 `auth.db`）、SSE 事件总线、RPC 桥（覆盖前端全部 `invoke` 命令）、上传/SQL 导出、会话 NDJSON 流与图片二进制；WebCtx 额外持有 Kimi / Antigravity / Claude 官方账号管理器（Kimi 管理器在桌面版是 Tauri state）。
- `src/web/*`：WebGate / 登录 / 初始化 / 改密界面，以及 `@tauri-apps/*` 浏览器 shim（Vite alias 注入）。
- `vite.config.web.ts`：Web 构建（`dist-web/`），仅 Web 构建注入 favicon。
- `public-web/`：浏览器页签图标（`favicon.svg` / `favicon.ico` / `apple-touch-icon.png`，取自 CC 应用图标）。
- `scripts/web-parity.mjs`：对比前端 `invoke` 命令与 Web RPC/覆盖实现（同时识别 CamelCase 命令，如 `queryProviderUsage`）。
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
node ../scripts/web-parity.mjs --strict   # 必须 0 未覆盖（含 CamelCase 命令）
```

上游新增命令会出现在 parity 的未覆盖列表里；为它补上 `src-tauri/src/web/rpc.rs` 中的分支或 `src/web/shims/overrides.ts` 的浏览器替代实现即可。

## Web 模式已知差异

- 单管理员账号；默认仅监听回环地址，HTTPS 由反向代理提供。账号头像来自各 OAuth 提供方的 CDN（GitHub / Google / xAI / DeepSeek / Kimi），`img-src` 放行 `https:` 图片；脚本、连接来源仍按 CSP 收紧。
- 托盘、自动更新、开机自启、窗口控件、系统文件对话框、终端启动等桌面能力被隐藏或替换（会话恢复命令改为复制到剪贴板）。
- **「以此供应商打开终端」在 Web 模式不可用**，会提示改用 SSH；AGY 的第三方供应商只能从终端启动，因此 Web 界面无法直接使用 AGY 第三方卡。Codex 的浏览器登录依赖本机 `127.0.0.1:1455` 回调，Web 模式请使用设备代码登录。
- 新应用 Antigravity（agy）、Kimi Code、DeepSeek Harness、Oh My Pi 复用通用命令，无需单独的 Web 实现；其账号功能依赖服务器上对应的 CLI / 凭据，Antigravity 多账号切换仍仅 macOS 可用。
- WebDAV/S3 下载恢复后不会自动执行桌面版的“导入后 live 投影”，建议随后手动切换/同步一次供应商或重启服务。
- `reset_circuit_breaker` 不执行桌面版的“恢复后自动切回更高优先级供应商”。
- 深链接的系统协议注册不可用；解析/合并/导入命令仍可通过界面或 API 使用。
- `import_prompt_from_file` 依赖原生文件对话框，Web 端不可用；SQL 备份已支持浏览器上传/下载。
- 浏览器覆盖的桌面命令（文件对话框、导出、剪贴板、托盘空操作等）见 `src/web/shims/overrides.ts`；新增前端 `invoke` 命令必须同时补 `rpc.rs` 或覆盖实现，`scripts/web-parity.mjs --strict` 会检查。
