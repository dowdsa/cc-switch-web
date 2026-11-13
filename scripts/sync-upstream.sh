#!/usr/bin/env bash
# 同步上游 farion1231/cc-switch 到本仓库的 Web 二开分支。
#
# 用法:
#   bash scripts/sync-upstream.sh                  # 合并最新 release tag 到临时分支
#   bash scripts/sync-upstream.sh v4.1.0           # 合并指定 tag 或 branch
#   bash scripts/sync-upstream.sh --no-verify      # 跳过编译/类型/覆盖率检查
#   bash scripts/sync-upstream.sh --apply          # 校验通过后合并回 web 分支
#
# 合并成功后，按输出提示重建并重启服务（或执行 deploy/install.sh）。
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

TARGET=""
VERIFY=1
APPLY=0

for arg in "$@"; do
  case "$arg" in
    --no-verify) VERIFY=0 ;;
    --apply) APPLY=1 ;;
    -h|--help)
      sed -n '2,11p' "$0"
      exit 0
      ;;
    -*)
      echo "未知参数: $arg" >&2
      exit 1
      ;;
    *)
      TARGET="$arg"
      ;;
  esac
done

info() { printf '\033[1;34m[sync]\033[0m %s\n' "$*"; }
fail() { printf '\033[1;31m[sync]\033[0m %s\n' "$*" >&2; exit 1; }

if [ -n "$(git status --porcelain)" ]; then
  fail "工作区有未提交的改动，请先 commit 或 stash，再执行同步。"
fi

if ! git rev-parse --verify -q refs/heads/web >/dev/null; then
  fail "未找到 web 分支（这是 Web 二开分支）。"
fi

if ! git remote get-url upstream >/dev/null 2>&1; then
  info "添加 upstream remote"
  git remote add upstream https://github.com/farion1231/cc-switch.git
fi

if [ "$(git rev-parse --is-shallow-repository)" = "true" ]; then
  info "补全上游历史（首次同步必须）…"
  git fetch --unshallow upstream
fi

info "拉取上游 tags 与 main"
git fetch upstream --tags --prune

if [ -z "$TARGET" ]; then
  TARGET="$(git tag -l 'v*' --sort=-v:refname | head -n 1)"
  if [ -z "$TARGET" ]; then
    TARGET="main"
  fi
fi

if git rev-parse -q --verify "refs/tags/$TARGET" >/dev/null; then
  REF="refs/tags/$TARGET"
elif git rev-parse -q --verify "refs/remotes/upstream/$TARGET" >/dev/null; then
  REF="upstream/$TARGET"
elif [ "$TARGET" = "main" ]; then
  REF="upstream/main"
else
  fail "找不到上游 tag/branch: $TARGET"
fi

BASE="$(git rev-parse --short web)"
BRANCH="sync/upstream-$(date +%Y%m%d-%H%M%S)"
info "基于 web@$BASE 创建 $BRANCH，合并 $TARGET"
git switch -c "$BRANCH" web

set +e
git merge --no-ff -m "Merge upstream $TARGET into web" "$REF"
MERGE_STATUS=$?
set -e

if [ "$MERGE_STATUS" -ne 0 ]; then
  echo
  fail "合并有冲突。冲突文件：
$(git diff --name-only --diff-filter=U | sed 's/^/  - /')

处理方式：
  1. 按 patches.md 收口：以上游版本为底，把 Web 改动重新贴回。
  2. Web 代码集中在 src-tauri/src/web/、src/web/，通常不冲突。
  3. 解决后 git add -A && git commit，再手动执行验证命令。"
fi

if [ "$VERIFY" -eq 1 ]; then
  export COREPACK_ENABLE_DOWNLOAD_PROMPT=0
  if command -v pnpm >/dev/null 2>&1; then
    PNPM=pnpm
  elif command -v corepack >/dev/null 2>&1; then
    PNPM="corepack pnpm"
  else
    fail "未找到 pnpm/corepack，无法验证。可加 --no-verify 跳过。"
  fi

  info "安装依赖"
  $PNPM install --frozen-lockfile --prod=false || fail "依赖安装失败（上游可能改了 lockfile，请手动 pnpm install 后重试）。"

  info "前端类型检查"
  $PNPM exec tsc --noEmit || fail "TypeScript 类型检查失败。"

  info "构建 Web 前端"
  $PNPM exec vite build --config vite.config.web.ts || fail "Web 前端构建失败。"

  info "检查桌面构建"
  ( cd src-tauri && cargo check ) || fail "桌面模式 cargo check 失败。"

  info "检查 Web 构建"
  ( cd src-tauri && cargo check --features web ) || fail "Web 模式 cargo check 失败。"

  info "命令覆盖检查"
  node scripts/web-parity.mjs --strict || fail "存在未覆盖的前端命令，请在 src-tauri/src/web/rpc.rs 或 src/web/shims/overrides.ts 中补齐。"

  info "验证通过"
fi

if [ "$APPLY" -eq 1 ]; then
  info "合并回 web"
  git switch web
  git merge --no-ff -m "Sync upstream $TARGET" "$BRANCH"
  echo
  info "已合并到 web。下一步："
  echo "  pnpm build:web"
  echo "  (cd src-tauri && cargo build --release --features web)"
  echo "  sudo bash deploy/install.sh --skip-build   # 重新安装并重启服务"
  echo "  bash scripts/publish-repos.sh --yes        # 发布单提交快照到 Gitee/GitHub（重写远程历史）"
  echo "  # 或者你当前机器上的: /root/cc-switch-web.sh restart"
else
  echo
  info "合并已在 $BRANCH 完成。确认无误后执行："
  echo "  git switch web && git merge --no-ff $BRANCH"
  echo "  # 重新构建并用 deploy/install.sh 部署"
  echo "  # 最后: bash scripts/publish-repos.sh --yes"
fi
