<div align="center">
  <img src="./src/assets/icons/logo.svg" width="88" alt="CC Switch Web" />

  <h1>CC Switch Web</h1>

  <p><strong>A browser UI for CC Switch, built for private servers</strong><br/>
  No CLI/TUI needed · Browser-based · Single-admin login</p>

  <p>
    <img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT" />
    <img src="https://img.shields.io/badge/platform-Linux-lightgrey.svg" alt="Linux" />
    <img src="https://img.shields.io/badge/UI-Web-orange.svg" alt="Web UI" />
    <img src="https://img.shields.io/badge/deploy-systemd-success.svg" alt="systemd" />
  </p>

  <p>
    <a href="https://gitee.com/lian-haoxiong/cc-switch-web">Gitee</a>
    ·
    <a href="https://github.com/dowdsa/cc-switch-web">GitHub</a>
  </p>

  <p>
    <a href="./docs/README.zh-CN.md"><img src="https://img.shields.io/badge/README-%E4%B8%AD%E6%96%87-red.svg" alt="中文 README" /></a>
  </p>
</div>

![Provider management](./docs/screenshots/providers.png)

> Screenshots use demo data and contain no real API keys.

Read this in **[中文](./docs/README.zh-CN.md)**.

## 🎯 Why this project exists

This is a **personal fork**, not a public product:

- The official CC Switch is desktop-first. On a headless server it means CLI/TUI, which I don't want to use.
- My server has **no public bandwidth**, and I don't plan to expose the admin UI to the internet.
- So I turned its provider/config management into a web UI: use it locally, over the LAN, or through an SSH tunnel. All configs and data stay on the server.

Forked from **[farion1231/cc-switch](https://github.com/farion1231/cc-switch)** (baseline v4.0.16). This is **not an official web edition** and has no affiliation with the upstream maintainers.

## ✨ Why the web version

CC Switch was originally a desktop app. This project brings the same provider management and routing capabilities to the browser:

- **Works on headless Linux servers**: one-command deploy, then manage everything from a browser
- **Full CC Switch capabilities**, not a cut-down UI: providers, routing, MCP, skills, prompts, sessions, usage
- **Built for servers**: single admin account, password change, session invalidation, login throttling, CSRF protection
- **Same data as desktop**: reads and writes real CLI configs and the same SQLite database
- **One-command deploy/update**: `deploy/install.sh` builds and installs a systemd service

## 🚀 Features

### Providers & routing

- Create / edit / delete / reorder / switch providers
- Direct, route and stack modes; proxy takeover and hot switching
- Automatic failover, circuit breaker, health checks, endpoint speed tests
- Official provider presets and universal providers
- Claude Desktop / Codex / Gemini / Grok / OpenCode / OpenClaw / Hermes / Pi / MiniMax Code

### MCP · Skills · Prompts

- Unified MCP management, synced to each CLI by per-app toggles
- Skill repositories, install/update/backup and multi-app enable
- Prompt library with enable/disable and target file sync (e.g. `CLAUDE.md`)
- Project profiles: snapshot and apply a whole provider set

### Sessions & usage

- Browse, search and export CLI sessions, with streaming reads for large logs
- Request logs, token/cost statistics by provider and model
- Subscription quotas (Claude / Codex / Copilot / xAI / coding plans) and balances

### Accounts, sync & security

- Auth center: GitHub Copilot / ChatGPT / xAI OAuth device flows and multi-account management
- WebDAV / S3 sync, database backups, SQL import/export
- Single admin: Argon2id hashing, server-side sessions, password change invalidates old sessions; the account menu lives at the bottom of the sidebar
- Loopback-first deployment, one-time setup token, login throttling, CSRF checks

## 🧩 Supported CLIs

| CLI | What is managed |
| --- | --- |
| Claude Code | Provider switching, MCP, prompts, sessions, usage |
| Claude Desktop | Official/relay configs, model routing |
| Codex | Providers, auth, stack models, sessions and usage |
| Gemini CLI | Providers, MCP, sessions |
| Grok Build | Providers and official login |
| OpenCode | Providers, default model, MCP |
| OpenClaw | Providers, agents/env/tools, workspace files |
| Hermes | Providers, models, memory files |
| Pi | Providers, models, prompt files, sessions |
| MiniMax Code | Provider management |

## 🖥 UI preview

| MCP management | Prompt library |
| --- | --- |
| ![MCP](./docs/screenshots/mcp.png) | ![Prompts](./docs/screenshots/prompts.png) |

| Auth center | Settings |
| --- | --- |
| ![Auth center](./docs/screenshots/auth-center.png) | ![Settings](./docs/screenshots/settings.png) |

| Login | Account menu |
| --- | --- |
| ![Login](./docs/screenshots/login.png) | ![Account](./docs/screenshots/account-menu.png) |

## 📦 Quick start

Requirements: Linux x64, Node.js 20+, Rust (see `rust-toolchain.toml`), GTK/WebKit dependencies. See [DEPLOY.md](./DEPLOY.md) for the full guide (Chinese).

```bash
# Gitee
git clone git@gitee.com:lian-haoxiong/cc-switch-web.git
# or GitHub
# git clone https://github.com/dowdsa/cc-switch-web.git

cd cc-switch-web

# Listen on 127.0.0.1:8787 by default (use SSH tunnel or Nginx HTTPS)
sudo bash deploy/install.sh

# Direct LAN access
sudo HOST=0.0.0.0 PORT=8787 \
  ALLOWED_ORIGINS=http://<server-ip>:8787 \
  bash deploy/install.sh
```

On first start, a one-time setup token is generated. Use it to create the admin account:

```bash
cat ~/.cc-switch/web/setup-token
```

Service management:

```bash
systemctl status cc-switch-web
systemctl restart cc-switch-web
journalctl -u cc-switch-web -f
```

Update: `git pull && sudo bash deploy/install.sh` (database and login state are preserved).

## 🏗 Architecture

```text
Browser (React SPA, Vite)
   │  /api/rpc · SSE · NDJSON streams · uploads/downloads
   ▼
Rust axum server (cargo feature "web")
   │  reuses CC Switch services / database / proxy
   ▼
~/.cc-switch/cc-switch.db   +   ~/.claude, ~/.codex, ... CLI configs
~/.cc-switch/web/auth.db (separate auth database)
```

Desktop-only features (tray, auto-update, native file dialogs, etc.) are hidden or replaced in the web UI. See [patches.md](./patches.md) for the change list and upstream merge notes.

## 🔒 Security notes

The web UI manages server-side CLI configs; some operations run with the service user's privileges. Please:

- Keep the default loopback binding and access it through an SSH tunnel or Nginx/Caddy HTTPS
- Do not expose it directly to the public internet; see [deploy/nginx.conf.example](./deploy/nginx.conf.example)
- The service runs as a specific user and manages that user's `~/.claude`, `~/.codex`, etc.

## 📚 Docs

- [DEPLOY.md](./DEPLOY.md): deployment, upgrades, upstream sync, snapshot publishing (Chinese)
- [patches.md](./patches.md): changes relative to upstream (Chinese)
- `scripts/sync-upstream.sh`: sync upstream releases
- `scripts/publish-repos.sh`: publish a single-commit snapshot to Gitee and GitHub

## 🙏 Upstream & license

- Upstream project: **CC Switch**
- Upstream repository: **https://github.com/farion1231/cc-switch**
- Baseline version: v4.0.16
- Upstream author: Jason Young

This is a community fork, **not an official web edition**, and is not affiliated with the upstream maintainers. The code and database structure are based on upstream; the web UI, authentication, deployment scripts and related changes are added by this repository.

Based on [farion1231/cc-switch](https://github.com/farion1231/cc-switch), Copyright (c) Jason Young, licensed under the MIT License.

Upstream code and copyright notices are retained. The web fork is released under the same license. See [LICENSE](./LICENSE).
