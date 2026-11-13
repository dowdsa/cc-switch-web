#!/usr/bin/env bash
# 一键构建 CC Switch Web 发布包（Linux amd64 + arm64，含内嵌前端的单二进制）。
#
# 前置：
#   - 前端产物 dist-web（脚本会自动构建，需要 pnpm）
#   - amd64：本机 rust 工具链即可
#   - arm64：Docker（用 deploy/Dockerfile.cross-arm64 交叉编译，本机为 x86_64 时约 3-5 分钟）
#
# 产物：
#   dist-release/cc-switch-web-<version>-linux-amd64-embedded.tar.gz
#   dist-release/cc-switch-web-<version>-linux-arm64-embedded.tar.gz
#
# 用法：
#   bash deploy/build-release.sh              # 两个架构都打
#   bash deploy/build-release.sh amd64        # 只打 amd64
#   bash deploy/build-release.sh arm64        # 只打 arm64（需要 Docker）
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_DIR"

VERSION="$(node -p "require('./package.json').version" 2>/dev/null || awk -F'"' '/^version/{print $2; exit}' src-tauri/Cargo.toml)"
OUT="$REPO_DIR/dist-release"
if [ $# -eq 0 ]; then
  WANT=(amd64 arm64)
else
  WANT=("$@")
fi

info() { printf '\033[1;34m[release]\033[0m %s\n' "$*"; }
fail() { printf '\033[1;31m[release]\033[0m %s\n' "$*" >&2; exit 1; }

info "构建 Web 前端"
pnpm build:web

package() {
  local arch="$1" bin="$2"
  local dir="$OUT/pkg-$arch"
  rm -rf "$dir"; mkdir -p "$dir"
  cp "$bin" "$dir/cc-switch"
  cp deploy/install.sh deploy/cc-switch-web.service deploy/nginx.conf.example "$dir/"
  cat > "$dir/README.txt" <<EOF
CC Switch Web v$VERSION (Linux $arch) — 单二进制版

前端已内嵌进二进制（rust-embed），解压后直接运行即可访问：
  ./cc-switch --web                     # 默认 127.0.0.1:8787
  ./cc-switch --web --port 9000         # 自定义端口（或 CC_SWITCH_WEB_PORT=9000）
  CC_SWITCH_WEB_HOST=0.0.0.0 CC_SWITCH_WEB_ALLOWED_ORIGINS=http://<IP>:8787 ./cc-switch --web

运行库依赖（Debian/Ubuntu）:
  apt-get install -y libwebkit2gtk-4.1-0 libgtk-3-0 libayatana-appindicator3-1 librsvg2-2

安装为 systemd 服务:
  sudo bash install.sh
EOF
  local tarball="$OUT/cc-switch-web-$VERSION-linux-$arch-embedded.tar.gz"
  tar -czf "$tarball" -C "$OUT" "pkg-$arch"
  rm -rf "$dir"
  info "  $tarball"
  sha256sum "$tarball"
}

mkdir -p "$OUT"

for arch in "${WANT[@]}"; do
  case "$arch" in
    amd64|x86_64)
      info "构建 amd64（本机原生）"
      ( cd src-tauri && cargo build --release --features web )
      package amd64 src-tauri/target/release/cc-switch
      ;;
    arm64|aarch64)
      command -v docker >/dev/null || fail "arm64 构建需要 Docker"
      info "构建 arm64（Docker 交叉编译）"
      docker build --platform linux/amd64 -f deploy/Dockerfile.cross-arm64 -t ccsw-cross-arm64 .
      docker run --rm -v "$REPO_DIR:/src" -v ccsw-cargo-cache-arm64:/usr/local/cargo/registry ccsw-cross-arm64
      package arm64 artifacts/cc-switch
      ;;
    *) fail "未知架构: $arch（可选 amd64 / arm64）" ;;
  esac
done

info "完成，产物在 $OUT"
