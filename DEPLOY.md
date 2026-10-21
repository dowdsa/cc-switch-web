# CC Switch Web 部署与维护

Web 二开版：浏览器中管理 Claude Code / Codex / Gemini / Grok / OpenCode / OpenClaw / Hermes / Pi / MiniMax 等 CLI 配置，带单管理员登录。

仓库：

- Gitee：`git@gitee.com:lian-haoxiong/cc-switch-web.git`
- GitHub：`https://github.com/dowdsa/cc-switch-web.git`

## 环境要求

- Linux x64（Web 模式不需要显示器）
- Node.js ≥ 20、corepack/pnpm
- Rust ≥ 1.85（`rust-toolchain.toml` 会指定具体版本）
- Tauri 构建/运行依赖（Debian/Ubuntu）：

```bash
apt-get update && apt-get install -y build-essential pkg-config curl \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev
```

> Web 模式本身不启动窗口，但二进制仍链接 GTK/WebKit 动态库，因此运行环境也需要上述运行库（`libwebkit2gtk-4.1-0`、`libgtk-3-0`）。上面安装 dev 包即可同时满足。

## 一键部署

```bash
git clone git@gitee.com:lian-haoxiong/cc-switch-web.git
cd cc-switch
sudo bash deploy/install.sh
```

脚本会：安装前端依赖 → 构建 `dist-web` → `cargo build --release --features web` → 安装到 `/opt/cc-switch` → 写入 systemd 服务并启动。

默认只监听 `127.0.0.1:8787`，适合本机 + SSH 隧道/Nginx 反代。

- 生产环境需要 systemd；容器等无 systemd 环境用 `--no-start`：脚本只构建并安装，随后打印手动启动命令。
- 已有 release 产物、只想更新安装时加 `--skip-build`。

### 内网直接访问

```bash
sudo HOST=0.0.0.0 PORT=8787 \
  ALLOWED_ORIGINS=http://192.168.1.79:8787 \
  bash deploy/install.sh
```

`ALLOWED_ORIGINS` 可填多个（逗号分隔、不要有空格）。非回环监听必须显式提供，否则程序会拒绝启动。

### 指定服务用户

服务必须读写该用户的 `~/.claude`、`~/.codex`、`~/.gemini` 等目录，默认使用执行 `sudo` 的用户（root 直装则用 root）：

```bash
sudo CCSWITCH_WEB_USER=claude-user bash deploy/install.sh
```

## 首次初始化

首次启动会在服务用户目录生成一次性令牌：

```bash
cat /home/<service-user>/.cc-switch/web/setup-token
```

打开页面，输入令牌并创建管理员账号（密码 ≥ 8 位）。创建后令牌立即作废。

数据位置：

- 业务数据：`~/.cc-switch/cc-switch.db`
- 认证库：`~/.cc-switch/web/auth.db`（独立，不参与 WebDAV/S3 同步）
- 上传临时文件：`/tmp/cc-switch-web-uploads/`

## 服务管理

```bash
systemctl status cc-switch-web
systemctl restart cc-switch-web
systemctl stop cc-switch-web
journalctl -u cc-switch-web -f
```

忘记密码：

```bash
sudo -u <service-user> HOME=/home/<service-user> \
  /opt/cc-switch/cc-switch --web-reset-password
```

## 更新 / 升级

```bash
cd cc-switch
git pull
sudo bash deploy/install.sh          # 重新构建、安装并重启，数据保留
```

只更新前端或已有 release 产物时：

```bash
sudo bash deploy/install.sh --skip-build
```

## 仓库发布方式（Gitee / GitHub 单提交快照）

Gitee 的 `web` 与 GitHub 的 `main` 都是**单提交快照**：包含完整可构建代码与 README，但不包含上游 Git 历史。上游历史只保留在本地仓库，用于同步与合并。

本地 `web` 分支保留完整历史且**不跟踪** `origin/web`（两者历史不同，直接 `git pull/push` 会冲突）；发布统一使用下面的脚本。

公开发布：

```bash
bash scripts/publish-repos.sh --yes
```

该脚本会把当前 `web` 工作树重新提交为一个初始提交，并强制推送到 `origin/web`（Gitee）和 `github/main`（GitHub）。已经 clone 过旧版本的机器需要重新 clone 或重置到对应远端分支。

## 同步上游新版本

```bash
# 合并最新 release tag 到临时分支并跑完整校验
bash scripts/sync-upstream.sh

# 指定版本
bash scripts/sync-upstream.sh v4.1.0

# 校验通过后合并回 web 分支
bash scripts/sync-upstream.sh --apply
```

冲突处理原则见仓库根目录 `patches.md`；Web 代码集中在 `src-tauri/src/web/`、`src/web/`，通常不冲突。`README.md` 已被替换为 Web 版说明，冲突时保留本地版本。

校验包括：`tsc`、Web 构建、桌面与 Web 两种 `cargo check`、命令覆盖检查（必须 0 未覆盖）。

## HTTPS 反向代理

推荐用 Nginx/Caddy 提供 TLS，示例见 `deploy/nginx.conf.example`。要点：

- 反代到 `127.0.0.1:8787`
- 透传 `Host`、`X-Forwarded-Proto`、`X-Forwarded-For`
- 关闭缓冲并延长超时（SSE 事件流）

## 已知差异

桌面专属能力（托盘、自动更新、系统托盘面板、终端启动、原生文件对话框等）在 Web 端被隐藏或替换；详细清单与上游合并注意事项见 `patches.md`。
