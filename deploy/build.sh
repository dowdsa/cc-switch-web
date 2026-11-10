#!/usr/bin/env bash
# 容器内构建脚本：为一个或多个 Rust target 编译 cc-switch（web feature），
# 并把二进制复制到 /out/<target>/cc-switch。
#
# 环境变量:
#   TARGETS  空格分隔的 Rust target 列表（默认 "x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu"）
#   JOBS     并行编译任务数（默认 nproc）
#   PROFILE  release（默认）
set -euo pipefail

SRC_TAURI="${SRC_TAURI:-/src/src-tauri}"
OUT="${OUT:-/out}"
PROFILE="${PROFILE:-release}"
TARGETS="${TARGETS:-x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu}"
JOBS="${JOBS:-$(nproc)}"

echo "[build.sh] targets: $TARGETS, jobs: $JOBS"
cd "$SRC_TAURI"

for TARGET in $TARGETS; do
  echo "[build.sh] ===== target $TARGET ====="
  # shellcheck disable=SC1091
  source /usr/local/bin/cross-env.sh "$TARGET"
  cargo build --release --features web --target "$TARGET" --jobs "$JOBS"
  # 产物查找: CARGO_TARGET_DIR 已设时位于 "$CARGO_TARGET_DIR/<triple>/release/"，
  # 未设时位于 "target/<triple>/release/"。两种布局都兼容。
  BIN=""
  for CAND in \
    "${CARGO_TARGET_DIR:-target}/$TARGET/$PROFILE/cc-switch" \
    "target/$TARGET/$PROFILE/cc-switch"; do
    [ -x "$CAND" ] && { BIN="$CAND"; break; }
  done
  [ -n "$BIN" ] || { echo "[build.sh] 产物缺失: target/$TARGET/$PROFILE/cc-switch（含 CARGO_TARGET_DIR=$CARGO_TARGET_DIR）" >&2; exit 1; }
  mkdir -p "$OUT/$TARGET"
  cp "$BIN" "$OUT/$TARGET/cc-switch"
  # 用 objdump 自检产物架构（容器里可能没有 file 命令）
  if command -v file >/dev/null 2>&1; then
    file "$OUT/$TARGET/cc-switch"
  elif [ -n "${GNU:-}" ]; then
    "$GNU-objdump" -f "$OUT/$TARGET/cc-switch" | sed -n '1,5p'
  fi
  echo "[build.sh] OK: $OUT/$TARGET/cc-switch"
done
echo "[build.sh] 全部完成"
