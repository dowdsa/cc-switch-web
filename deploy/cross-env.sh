#!/usr/bin/env bash
# 交叉编译环境变量助手（容器内使用）。
# 用法: source cross-env.sh aarch64-unknown-linux-gnu
#
# Ubuntu multiarch 布局: arm64 库在 /usr/lib/aarch64-linux-gnu，头文件共享
# /usr/include（webkit 的 .pc prefix=/usr）。因此不能用 PKG_CONFIG_SYSROOT_DIR
# （它会把 /usr/include 前缀成不存在的路径），只用 PKG_CONFIG_LIBDIR 切换库目录。
set -euo pipefail

TARGET="$1"
case "$TARGET" in
  x86_64-unknown-linux-gnu)
    # 原生构建，无需交叉配置
    ;;
  aarch64-unknown-linux-gnu)
    TRIPLE=aarch64-unknown-linux-gnu
    GNU=aarch64-linux-gnu
    # 链接器（cargo）与 C/C++ 编译器（cc crate 及 aws-lc-sys/ring 等构建脚本）。
    # cc crate 认下划线形式 CARGO_TARGET_<TRIPLE>_CC / CC_<triple>；
    # 带连字符的旧形式不是合法 shell 标识符，不能 export。
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$GNU-gcc"
    export CC_aarch64_unknown_linux_gnu="$GNU-gcc"
    export CXX_aarch64_unknown_linux_gnu="$GNU-g++"
    export AR_aarch64_unknown_linux_gnu="$GNU-ar"
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_AR="$GNU-ar"
    # pkg-config 只看 arm64 的库目录；共享头文件目录 /usr/share/pkgconfig 保留
    export PKG_CONFIG_ALLOW_CROSS=1
    export PKG_CONFIG_LIBDIR="/usr/lib/aarch64-linux-gnu/pkgconfig:/usr/share/pkgconfig"
    unset PKG_CONFIG_PATH 2>/dev/null || true
    ;;
  *) echo "未知目标: $TARGET" >&2; exit 1 ;;
esac
