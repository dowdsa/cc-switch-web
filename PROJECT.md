# PROJECT.md — CC Switch Web 二开项目约定

本项目是 [farion1231/cc-switch](https://github.com/farion1231/cc-switch) 的 Web 二开 fork，不是上游的一部分。给本仓库做任何改动（人工或 agent）前，先读这份文件。

## 最高准则：Web UI 以 Web 版为准

**上游没有 Web 版。凡是界面（宽、高、字号、间距、布局、交互）相关的样式，一律以本仓库 Web 版的现存实现为准，而不是上游桌面版的实现。**

具体到合并上游时的冲突解决：

- 上游改了某个组件的样式，而 Web fork 对同一处有过自己的调整（如侧栏行高 `h-9` vs 上游 `h-7`）→ **保留 Web fork 的值**，把上游的新机制套在 fork 的值上（例如上游引入 `h-nav-row` CSS 变量时，改的是 `src/index.css` 里变量的值，而不是把组件类名换回上游的）。
- 只有上游新增的功能逻辑、修复、无 Web fork 改动痕迹的样式，才直接取上游。
- 判断依据：`git log web -- src/<file>` 里 fork 是否动过；拿不准时看 `e4a5008d`（Web fork 初始快照）之后的 diff。

历史案例：2026-11 同步 v4.0.31 时，上游把侧栏 `h-7` 重构成 `h-nav-row` 变量（为「紧凑/舒适」密度设置服务），冲突时误取上游值，把 fork 的行高从 36px 打回 28px。教训：**上游重构样式机制 ≠ fork 放弃自己的数值**。

## 项目结构

- 前端：React SPA（`src/`），Web 专属层在 `src/web/`（登录、shims、覆盖实现）
- 后端：Rust axum（`src-tauri/src/web/`），经 cargo feature `web` 启用；上游代码在 `src-tauri/src/` 其余部分
- 数据：`~/.cc-switch/cc-switch.db`（业务）+ `~/.cc-switch/web/auth.db`（认证，独立）

## 合并上游的标准动作

```bash
bash scripts/sync-upstream.sh <tag> --no-verify   # 合并到临时分支，解决冲突
# 按本文件准则解决冲突后：
pnpm exec tsc --noEmit
pnpm exec vite build --config vite.config.web.ts
cd src-tauri && cargo check && cargo check --features web
node scripts/web-parity.mjs --strict   # 必须 0 未覆盖
pnpm exec vitest run
```

- 上游新增的前端 `invoke` 命令必须补 `src-tauri/src/web/rpc.rs` 分支或 `src/web/shims/overrides.ts` 浏览器实现，否则 parity 检查失败。
- 冲突固定模式：`.github/workflows/*`、`README_DE/JA/ZH.md` 保持删除；根 `README.md` 保留 Web 版（多语言说明见 `docs/README.zh-CN.md`）。
- 同步完成后更新 `patches.md` 的基线版本与摘要。

## Web 模式的边界

- 桌面专属能力（托盘、自动更新、系统对话框、终端启动、深链接协议注册）在 Web 端隐藏或替换，浏览器等价实现在 `src/web/shims/overrides.ts`。
- 单管理员认证：Argon2id + 服务端会话 + CSRF + 登录限流，认证库独立于业务库。
- 默认监听回环地址；`ALLOWED_ORIGINS` 显式配置才允许对外。端口可用 `--port` / `CC_SWITCH_WEB_PORT` 自定义（默认 8787），监听地址用 `--host` / `CC_SWITCH_WEB_HOST`。

## 发布打包

前端产物在 release 构建时经 `rust-embed` 编译期嵌入二进制（`src-tauri/src/web/static_dist.rs`），
**发布包是单二进制**，解压 `./cc-switch --web` 即可访问；同目录若有 `dist-web/` 目录则优先用磁盘（便于只更新前端）。

一键构建双架构包：

```bash
bash deploy/build-release.sh          # amd64 + arm64
bash deploy/build-release.sh arm64    # 只打 arm64
```

- amd64：本机 `cargo build --release --features web`
- arm64：`deploy/Dockerfile.cross-arm64` 在 x86_64 上**原生交叉编译**到 aarch64（约 3-5 分钟）。
  **切勿用 `--platform linux/arm64` 走 QEMU 模拟编译**——同一份代码要 1 小时以上。
  交叉编译需要 arm64 的 GTK/WebKit 开发库，全部装在容器里，不改本机环境。

发布到 GitHub Release + Gitee 发行版：

```bash
git tag -f vX.Y.Z-web publish && bash deploy/publish-release.sh vX.Y.Z-web
# 前置：gh auth login；npm install -g @gitee/gitee-cli 且 gitee auth login（PAT 需 projects 权限）
# Gitee 附件走 API multipart（gitee CLI 的 api 子命令只支持 JSON 字段），脚本里用 curl 上传
```

## 文档

- `patches.md`：相对上游的改动清单与合并注意（每次同步后更新）
- `DEPLOY.md`：部署、升级、发布快照
- `scripts/publish-repos.sh`：发布为 Gitee `web` / GitHub `main` 单提交快照
- `deploy/build-release.sh` / `deploy/publish-release.sh`：构建并发布双架构安装包
