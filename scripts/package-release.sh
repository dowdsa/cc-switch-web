#!/usr/bin/env bash
# 构建多架构发布包（amd64 + arm64），输出到 dist-release/。
#
#   amd64: 宿主直接 cargo build（与 deploy/install.sh 一致的构建方式）
#   arm64: Docker 交叉编译（deploy/Dockerfile.build）
#
# 用法:
#   bash scripts/package-release.sh                  # 双架构
#   bash scripts/package-release.sh --arch arm64     # 仅 arm64
#   bash scripts/package-release.sh --arch amd64     # 仅 amd64
#   bash scripts/package-release.sh --skip-frontend  # 复用现有 dist-web
#   bash scripts/package-release.sh --publish v4.0.23  # 打包后创建 GitHub release
#
# 产物:
#   dist-release/cc-switch-web-<version>-linux-amd64.tar.gz
#   dist-release/cc-switch-web-<version>-linux-arm64.tar.gz
#   dist-release/SHA256SUMS
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
REPO_DIR="$(pwd)"

ARCHS="amd64 arm64"
SKIP_FRONTEND=0
PUBLISH_TAG=""
while [ $# -gt 0 ]; do
  case "$1" in
    --arch) ARCHS="$2"; shift 2 ;;
    --skip-frontend) SKIP_FRONTEND=1; shift ;;
    --publish) PUBLISH_TAG="$2"; shift 2 ;;
    -h|--help) sed -n '2,18p' "$0"; exit 0 ;;
    *) echo "未知参数: $1" >&2; exit 1 ;;
  esac
done

info() { printf '\033[1;34m[package]\033[0m %s\n' "$*"; }
fail() { printf '\033[1;31m[package]\033[0m %s\n' "$*" >&2; exit 1; }

VERSION="$(grep -m1 '^version' src-tauri/Cargo.toml | sed 's/.*"\(.*\)".*/\1/')"
[ -n "$VERSION" ] || fail "无法从 src-tauri/Cargo.toml 读取版本"
DIST_DIR="$REPO_DIR/dist-release"
STAGE_ROOT="$DIST_DIR/.stage"
info "版本 $VERSION, 架构: $ARCHS"

mkdir -p "$DIST_DIR"
rm -rf "$STAGE_ROOT"

# ---------- 前端产物（所有架构共用） ----------
if [ "$SKIP_FRONTEND" -eq 0 ]; then
  export COREPACK_ENABLE_DOWNLOAD_PROMPT=0
  if command -v pnpm >/dev/null 2>&1; then PNPM=pnpm; else PNPM="corepack pnpm"; fi
  info "安装前端依赖"
  $PNPM install --frozen-lockfile --prod=false >/dev/null
  info "构建 Web 前端"
  $PNPM build:web >/dev/null
else
  [ -f "$REPO_DIR/dist-web/index.html" ] || fail "--skip-frontend 但 dist-web/index.html 不存在"
fi

# ---------- amd64: 宿主原生构建 ----------
build_amd64() {
  info "amd64: 宿主 cargo build --release --features web"
  ( cd "$REPO_DIR/src-tauri" && cargo build --release --features web )
  mkdir -p "$STAGE_ROOT/amd64"
  cp "$REPO_DIR/src-tauri/target/release/cc-switch" "$STAGE_ROOT/amd64/cc-switch"
  file "$STAGE_ROOT/amd64/cc-switch" | grep -q 'x86-64' || fail "amd64 产物架构异常"
}

# ---------- arm64: Docker 交叉编译 ----------
build_arm64() {
  command -v docker >/dev/null 2>&1 || fail "未找到 docker（arm64 交叉编译需要）"
  docker info >/dev/null 2>&1 || fail "docker daemon 不可用"
  info "arm64: 构建/复用交叉编译镜像 ccsw-build"
  docker image inspect ccsw-build:latest >/dev/null 2>&1 \
    || docker build -t ccsw-build:latest -f "$REPO_DIR/deploy/Dockerfile.build" "$REPO_DIR/deploy/"
  info "arm64: 容器内 cargo build（首次较慢，增量缓存见下方说明）"
  mkdir -p "$STAGE_ROOT/arm64" "$DIST_DIR/.cargo-cache"
  # cargo registry/target 缓存挂在 dist-release/.cargo-cache，重复打包不用重下重编
  docker run --rm \
    -v "$REPO_DIR/src-tauri:/src/src-tauri" \
    -v "$STAGE_ROOT/arm64:/out" \
    -v "$DIST_DIR/.cargo-cache/registry:/usr/local/cargo/registry" \
    -v "$DIST_DIR/.cargo-cache/git:/usr/local/cargo/git" \
    -v "$DIST_DIR/.cargo-cache/target:/src/src-tauri/target-arm64" \
    -e TARGETS=aarch64-unknown-linux-gnu \
    -e CARGO_TARGET_DIR=/src/src-tauri/target-arm64 \
    ccsw-build:latest
  # build.sh 输出到 /out/<triple>/cc-switch，摊平到 stage 根
  [ -f "$STAGE_ROOT/arm64/aarch64-unknown-linux-gnu/cc-switch" ] \
    || fail "容器构建未产出 /out/aarch64-unknown-linux-gnu/cc-switch"
  mv "$STAGE_ROOT/arm64/aarch64-unknown-linux-gnu/cc-switch" "$STAGE_ROOT/arm64/cc-switch"
  rmdir "$STAGE_ROOT/arm64/aarch64-unknown-linux-gnu" 2>/dev/null || true
  file "$STAGE_ROOT/arm64/cc-switch" | grep -q 'aarch64' || fail "arm64 产物架构异常"
}

for A in $ARCHS; do
  case "$A" in
    amd64) build_amd64 ;;
    arm64) build_arm64 ;;
    *) fail "未知架构: $A（支持 amd64 arm64）" ;;
  esac
done

# ---------- 组包 ----------
summary_of_changes() { :; }  # 占位：release notes 由 --publish 分支生成

make_tarball() {
  local ARCH="$1" BIN="$STAGE_ROOT/$ARCH/cc-switch"
  local DIR="cc-switch-web-v$VERSION-linux-$ARCH"
  local STAGE="$STAGE_ROOT/$DIR"
  rm -rf "$STAGE"; mkdir -p "$STAGE"
  install -m 0755 "$BIN" "$STAGE/cc-switch"
  cp -a "$REPO_DIR/dist-web" "$STAGE/dist-web"
  cp "$REPO_DIR/deploy/cc-switch-web.service" "$REPO_DIR/deploy/nginx.conf.example" "$STAGE/"
  cp "$REPO_DIR/DEPLOY.md" "$STAGE/"
  ( cd "$STAGE_ROOT" && tar -czf "$DIST_DIR/cc-switch-web-v$VERSION-linux-$ARCH.tar.gz" "$DIR" )
  info "打包完成: cc-switch-web-v$VERSION-linux-$ARCH.tar.gz ($(du -h "$DIST_DIR/cc-switch-web-v$VERSION-linux-$ARCH.tar.gz" | cut -f1))"
}

for A in $ARCHS; do make_tarball "$A"; done

# ---------- 校验和 ----------
( cd "$DIST_DIR" && sha256sum cc-switch-web-v$VERSION-linux-*.tar.gz > SHA256SUMS )
info "SHA256SUMS:"
cat "$DIST_DIR/SHA256SUMS"

# ---------- 发布 ----------
if [ -n "$PUBLISH_TAG" ]; then
  info "创建 GitHub release $PUBLISH_TAG"
  command -v gh >/dev/null 2>&1 || fail "未找到 gh CLI"
  gh auth status --hostname github.com >/dev/null 2>&1 || fail "gh 未登录（gh auth login）"
  NOTES="$DIST_DIR/.release-notes-$PUBLISH_TAG.md"
  cat > "$NOTES" <<EOF
基于上游 [farion1231/cc-switch v$VERSION](https://github.com/farion1231/cc-switch/releases/tag/v$VERSION) 的 Web 二开版发布。

## 下载

| 文件 | 适用 |
| --- | --- |
| \`cc-switch-web-v$VERSION-linux-amd64.tar.gz\` | x86_64 Linux |
| \`cc-switch-web-v$VERSION-linux-arm64.tar.gz\` | aarch64 Linux |

均为免构建部署包（二进制 + 前端产物 + systemd 单元与 Nginx 示例）。
要求 Ubuntu 22.04+/Debian 12+/RHEL 9+（GLIBC 2.35），并安装
\`libwebkit2gtk-4.1-0\`、\`libgtk-3-0\` 运行库（Debian 系:
\`apt-get install -y libwebkit2gtk-4.1-0 libgtk-3-0\`）。

## 部署

\`\`\`bash
tar -xzf cc-switch-web-v$VERSION-linux-<arch>.tar.gz
cd cc-switch-web-v$VERSION-linux-<arch>
# 详见包内 DEPLOY.md
sudo install -m 0755 cc-switch /opt/cc-switch/cc-switch
sudo cp -a dist-web /opt/cc-switch/dist-web
# systemd 单元示例见 cc-switch-web.service，默认监听 127.0.0.1:8787
\`\`\`

非回环监听必须设置 \`CC_SWITCH_WEB_ALLOWED_ORIGINS\`；首次启动在
\`~/.cc-switch/web/setup-token\` 生成一次性初始化令牌。

## SHA-256

\`\`\`
$(cat "$DIST_DIR/SHA256SUMS")
\`\`\`
EOF
  gh release create "$PUBLISH_TAG" \
    --repo dowdsa/cc-switch-web --target main \
    --title "CC Switch Web v$VERSION" \
    --notes-file "$NOTES" \
    "$DIST_DIR"/cc-switch-web-v$VERSION-linux-*.tar.gz "$DIST_DIR/SHA256SUMS"
  info "release 已发布: https://github.com/dowdsa/cc-switch-web/releases/tag/$PUBLISH_TAG"
fi

rm -rf "$STAGE_ROOT"
info "全部完成，产物在 $DIST_DIR/"
