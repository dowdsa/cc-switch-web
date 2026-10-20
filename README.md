# CC Switch Web

在浏览器中管理 Claude Code、Codex、Gemini CLI、Grok、OpenCode、OpenClaw、Hermes、Pi、MiniMax Code 等 AI CLI 的供应商与配置。

本项目基于开源项目 [CC Switch](https://github.com/farion1231/cc-switch)（MIT）进行 Web 二开：保留其完整的管理能力，并在服务端提供网页界面与单管理员登录，适合无图形界面的 Linux 服务器。Web 模式与桌面端共用同一套业务逻辑与数据库。

## 功能

- 供应商新增/编辑/切换，直连、路由、聚合模式，故障转移
- MCP、Skills、提示词、会话浏览/搜索/导出
- 用量统计、模型定价、订阅额度、请求日志
- 授权中心：GitHub Copilot / ChatGPT / xAI OAuth 设备码登录与多账号
- WebDAV / S3 同步、数据库备份、SQL 导入导出
- 单管理员登录：Argon2id 密码哈希、服务端会话、改密、登录限流

桌面专属能力（系统托盘、自动更新、原生文件对话框、终端启动等）在 Web 端隐藏或替换为等价操作；差异清单见 [patches.md](./patches.md)。

## 快速开始

环境要求：Linux x64、Node.js 20+、Rust（以 `rust-toolchain.toml` 为准）、GTK/WebKit 开发库。详见 [DEPLOY.md](./DEPLOY.md)。

```bash
git clone git@gitee.com:lian-haoxiong/cc-switch.git
cd cc-switch

# 默认仅监听 127.0.0.1:8787（推荐配合 SSH 隧道或 Nginx HTTPS）
sudo bash deploy/install.sh

# 内网直接访问
sudo HOST=0.0.0.0 PORT=8787 \
  ALLOWED_ORIGINS=http://<服务器IP>:8787 \
  bash deploy/install.sh
```

首次启动会在服务用户目录生成一次性初始化令牌：

```bash
cat ~/.cc-switch/web/setup-token
```

打开页面，输入令牌并创建管理员账号（密码至少 8 位），令牌随即作废。

常用管理：

```bash
systemctl status cc-switch-web
systemctl restart cc-switch-web
journalctl -u cc-switch-web -f
```

更新：`git pull && sudo bash deploy/install.sh`（数据库与登录账号保留）。

忘记密码：

```bash
sudo -u <service-user> HOME=/home/<service-user> \
  /opt/cc-switch/cc-switch --web-reset-password
```

## 安全提醒

Web 端拥有管理服务器 CLI 配置的能力（部分操作等价于服务用户权限）。默认只监听回环地址；对外访问请使用 Nginx/Caddy 提供 HTTPS，并避免直接暴露端口。反向代理示例见 [deploy/nginx.conf.example](./deploy/nginx.conf.example)。

## 文档

- [DEPLOY.md](./DEPLOY.md)：部署、升级、同步上游
- [patches.md](./patches.md)：相对上游的改动清单与合并注意事项
- `scripts/sync-upstream.sh`：同步上游 release
- `scripts/publish-gitee.sh`：把当前代码发布为 Gitee 上的单提交快照

## 上游与许可

Based on [farion1231/cc-switch](https://github.com/farion1231/cc-switch), Copyright (c) Jason Young, licensed under the MIT License. 本项目保留上游代码与版权声明，Web 二开部分按同一许可证发布，详见 [LICENSE](./LICENSE)。
