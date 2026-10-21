#!/usr/bin/env bash
# 一键构建、安装并启动 CC Switch Web（systemd）。
#
# 用法:
#   sudo bash deploy/install.sh
#   sudo HOST=0.0.0.0 PORT=8787 \
#        ALLOWED_ORIGINS=http://192.168.1.79:8787 \
#        bash deploy/install.sh
#
# 选项:
#   --skip-build   跳过前端/Rust 构建，直接安装现有产物并重启
#   --no-start     只构建并安装，不写 systemd 单元（无 systemd 环境手动启动）
#
# 环境变量:
#   HOST / PORT               仅监听回环时 HOST 可用默认 127.0.0.1；对外监听必须显式
#   ALLOWED_ORIGINS           HOST 非回环时必填，逗号分隔，如 http://192.168.1.79:8787
#   COOKIE_SECURE             auto|true|false（默认 auto，https 反代下自动加 Secure）
#   CCSWITCH_WEB_USER         运行服务的系统用户，默认 sudo 发起者或 root
#   INSTALL_DIR               安装目录，默认 /opt/cc-switch
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SERVICE_NAME="cc-switch-web"
ENV_FILE="/etc/${SERVICE_NAME}.env"
UNIT_FILE="/etc/systemd/system/${SERVICE_NAME}.service"
INSTALL_DIR="${INSTALL_DIR:-/opt/cc-switch}"
HOST="${HOST:-127.0.0.1}"
PORT="${PORT:-8787}"
ALLOWED_ORIGINS="${ALLOWED_ORIGINS:-}"
COOKIE_SECURE="${COOKIE_SECURE:-auto}"
SKIP_BUILD=0
NO_START=0

for arg in "$@"; do
  case "$arg" in
    --skip-build) SKIP_BUILD=1 ;;
    --no-start) NO_START=1 ;;
    -h|--help)
      sed -n '2,20p' "$0"
      exit 0
      ;;
    *)
      echo "未知参数: $arg" >&2
      exit 1
      ;;
  esac
done

info() { printf '\033[1;34m[deploy]\033[0m %s\n' "$*"; }
fail() { printf '\033[1;31m[deploy]\033[0m %s\n' "$*" >&2; exit 1; }

[ "$(id -u)" -eq 0 ] || fail "请使用 root 或 sudo 运行"

if [ "$NO_START" -eq 0 ]; then
  if ! command -v systemctl >/dev/null 2>&1 || [ ! -d /run/systemd/system ]; then
    fail "systemd 不可用。可改用 --no-start 只构建安装，再手动启动（见输出）。"
  fi
fi

SERVICE_USER="${CCSWITCH_WEB_USER:-${SUDO_USER:-root}}"
id "$SERVICE_USER" >/dev/null 2>&1 || fail "用户不存在: $SERVICE_USER"
SERVICE_GROUP="$(id -gn "$SERVICE_USER")"
SERVICE_HOME="$(getent passwd "$SERVICE_USER" | cut -d: -f6)"
[ -n "$SERVICE_HOME" ] || fail "无法获取用户 $SERVICE_USER 的主目录"

case "$HOST" in
  127.0.0.1|::1|localhost) ;;
  *)
    [ -n "$ALLOWED_ORIGINS" ] || fail "HOST=$HOST 属于对外监听，必须设置 ALLOWED_ORIGINS（例如 http://<服务器IP>:$PORT）。"
    ;;
esac

if [ "$SKIP_BUILD" -eq 0 ]; then
  command -v node >/dev/null 2>&1 || fail "未找到 node（需要 Node.js 20+）"
  command -v cargo >/dev/null 2>&1 || fail "未找到 cargo（需要 Rust 1.85+）"
  command -v pkg-config >/dev/null 2>&1 || fail "未找到 pkg-config"

  pkg-config --exists webkit2gtk-4.1 || fail "缺少 libwebkit2gtk-4.1-dev。
Debian/Ubuntu 可执行:
  apt-get update && apt-get install -y build-essential pkg-config curl \\
    libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev"
  pkg-config --exists gtk+-3.0 || fail "缺少 libgtk-3-dev（见上面的 apt 安装列表）。"

  export COREPACK_ENABLE_DOWNLOAD_PROMPT=0
  if command -v pnpm >/dev/null 2>&1; then
    PNPM=pnpm
  elif command -v corepack >/dev/null 2>&1; then
    PNPM="corepack pnpm"
  else
    fail "未找到 pnpm/corepack。请安装 Node.js 20+ 并执行 corepack enable。"
  fi

  info "安装前端依赖（含构建工具）"
  ( cd "$REPO_DIR" && $PNPM install --frozen-lockfile --prod=false )

  info "构建 Web 前端"
  ( cd "$REPO_DIR" && $PNPM build:web )

  info "构建 Rust 服务（release，首次编译耗时较长）"
  ( cd "$REPO_DIR/src-tauri" && cargo build --release --features web )
fi

BIN_SRC="$REPO_DIR/src-tauri/target/release/cc-switch"
DIST_SRC="$REPO_DIR/dist-web"
[ -x "$BIN_SRC" ] || fail "未找到 release 二进制: $BIN_SRC（请先构建，或去掉 --skip-build）"
[ -f "$DIST_SRC/index.html" ] || fail "未找到前端产物: $DIST_SRC/index.html"

info "安装到 $INSTALL_DIR"
install -d -m 0755 "$INSTALL_DIR"
install -m 0755 "$BIN_SRC" "$INSTALL_DIR/cc-switch"
rm -rf "$INSTALL_DIR/dist-web"
cp -a "$DIST_SRC" "$INSTALL_DIR/dist-web"
chmod -R a+rX "$INSTALL_DIR"

info "写入环境配置 $ENV_FILE"
cat > "$ENV_FILE" <<EOF
HOME=$SERVICE_HOME
RUST_LOG=info
CC_SWITCH_WEB_HOST=$HOST
CC_SWITCH_WEB_PORT=$PORT
CC_SWITCH_WEB_ALLOWED_ORIGINS=$ALLOWED_ORIGINS
CC_SWITCH_WEB_COOKIE_SECURE=$COOKIE_SECURE
EOF
chmod 0640 "$ENV_FILE"
chown root:"$SERVICE_GROUP" "$ENV_FILE"

if [ "$NO_START" -eq 1 ]; then
  echo
  info "--no-start：已安装到 $INSTALL_DIR，未配置 systemd。"
  echo "  手动启动命令（可用 systemd/supervisor/nohup 托管）："
  echo "  sudo -u $SERVICE_USER env HOME=$SERVICE_HOME \\"
  echo "    CC_SWITCH_WEB_HOST=$HOST CC_SWITCH_WEB_PORT=$PORT \\"
  echo "    CC_SWITCH_WEB_ALLOWED_ORIGINS='$ALLOWED_ORIGINS' \\"
  echo "    CC_SWITCH_WEB_COOKIE_SECURE=$COOKIE_SECURE \\"
  echo "    $INSTALL_DIR/cc-switch --web --dist $INSTALL_DIR/dist-web"
  echo
  if [ "$HOST" = "127.0.0.1" ]; then
    echo "  访问地址: http://127.0.0.1:$PORT"
  else
    echo "  访问地址: http://<服务器IP>:$PORT"
  fi
  exit 0
fi

info "写入 systemd 服务 $UNIT_FILE"
cat > "$UNIT_FILE" <<EOF
[Unit]
Description=CC Switch Web
Documentation=https://github.com/dowdsa/cc-switch-web
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=$SERVICE_USER
Group=$SERVICE_GROUP
EnvironmentFile=$ENV_FILE
WorkingDirectory=$INSTALL_DIR
ExecStart=$INSTALL_DIR/cc-switch --web --dist $INSTALL_DIR/dist-web
Restart=on-failure
RestartSec=3
UMask=0077
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
EOF

if systemctl is-active --quiet "$SERVICE_NAME"; then
  :
elif ss -tln 2>/dev/null | awk '{print $4}' | grep -q ":$PORT$"; then
  fail "端口 $PORT 已被其他进程占用，请先停止它或改用 PORT 环境变量。"
fi

systemctl daemon-reload
systemctl enable "$SERVICE_NAME" >/dev/null
systemctl restart "$SERVICE_NAME"
sleep 2
if ! systemctl is-active --quiet "$SERVICE_NAME"; then
  echo "服务启动失败，最近日志:"
  journalctl -u "$SERVICE_NAME" -n 30 --no-pager || true
  exit 1
fi

TOKEN_FILE="$SERVICE_HOME/.cc-switch/web/setup-token"
for _ in $(seq 1 30); do
  [ -f "$TOKEN_FILE" ] && break
  sleep 0.5
done
if [ -f "$TOKEN_FILE" ]; then
  echo
  echo "============================================================"
  echo "  首次初始化令牌: $(cat "$TOKEN_FILE")"
  echo "  打开 Web 页面后用该令牌创建管理员账号（一次性）。"
  echo "============================================================"
else
  echo
  info "未生成初始化令牌（通常表示管理员账号已存在，直接登录即可）。"
  echo "  如忘记了密码，可执行 --web-reset-password 重置，见 DEPLOY.md。"
fi

echo
info "部署完成"
echo "  服务用户: $SERVICE_USER（管理该用户的 ~/.claude 等配置）"
if [ "$HOST" = "127.0.0.1" ]; then
  echo "  访问地址: http://127.0.0.1:$PORT（远程请用 SSH 隧道或反向代理）"
else
  echo "  访问地址: http://<服务器IP>:$PORT"
fi
echo "  管理命令: systemctl start|stop|restart|status $SERVICE_NAME"
echo "  日志:     journalctl -u $SERVICE_NAME -f"
