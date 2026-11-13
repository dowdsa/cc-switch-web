#!/usr/bin/env bash
# 容器内交叉编译 CC Switch Web（x86_64 宿主 → aarch64 目标，原生编译，无 QEMU）。
# 前端用仓库里已构建好的 dist-web（宿主 `pnpm build:web` 产出，与架构无关）。
set -euo pipefail
cd /src

TARGET=aarch64-unknown-linux-gnu

# crates 走加速镜像 + 交叉链接器配置
mkdir -p "$CARGO_HOME"
cat > "$CARGO_HOME/config.toml" <<EOF
[source.crates-io]
replace-with = "mirror"
[source.mirror]
registry = "sparse+https://rsproxy.cn/index/"
[net]
git-fetch-with-cli = true

[target.$TARGET]
linker = "aarch64-linux-gnu-gcc"

[target.$TARGET.env]
CC_aarch64_unknown_linux_gnu = "aarch64-linux-gnu-gcc"
CXX_aarch64_unknown_linux_gnu = "aarch64-linux-gnu-g++"
AR_aarch64_unknown_linux_gnu = "aarch64-linux-gnu-ar"
EOF

# pkg-config 找 arm64 的 .pc（不是宿主 amd64）
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_PATH="/usr/lib/aarch64-linux-gnu/pkgconfig:/usr/share/pkgconfig"

if [ ! -f /src/dist-web/index.html ]; then
  echo "[cross] 未找到 /src/dist-web/index.html，请先在宿主执行 pnpm build:web" >&2
  exit 1
fi

cd /src/src-tauri
# rust-toolchain.toml 指定工具链版本，rustup 在首次调用时自动安装
echo "[cross] 确保 rustup target $TARGET"
rustup target add "$TARGET"

echo "[cross] cargo build（原生交叉编译，内嵌前端）"
cargo build --release --features web --target "$TARGET"

mkdir -p /src/artifacts
cp "target/$TARGET/release/cc-switch" /src/artifacts/cc-switch
file /src/artifacts/cc-switch
echo "[cross] 完成: /src/artifacts/cc-switch"
