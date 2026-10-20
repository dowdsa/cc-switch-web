#!/usr/bin/env bash
# 把本地 web 分支的当前树发布为 Gitee 上的单提交快照。
# Gitee 仓库不包含上游提交历史；本地仍保留完整历史用于同步上游。
#
# 用法:
#   bash scripts/publish-gitee.sh          # 交互确认
#   bash scripts/publish-gitee.sh --yes    # 非交互
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

YES=0
[ "${1:-}" = "--yes" ] && YES=1

info() { printf '\033[1;34m[publish]\033[0m %s\n' "$*"; }
fail() { printf '\033[1;31m[publish]\033[0m %s\n' "$*" >&2; exit 1; }

[ -n "$(git status --porcelain)" ] && fail "工作区有未提交改动，请先提交。"
[ "$(git branch --show-current)" = "web" ] || fail "请先切换到 web 分支。"

REMOTE="${REMOTE:-origin}"
BASE_TAG="$(git describe --tags --abbrev=0 2>/dev/null || echo unknown)"
MESSAGE="CC Switch Web snapshot (based on upstream ${BASE_TAG})"
TREE="$(git rev-parse "web^{tree}")"

info "将把 web@$(git rev-parse --short HEAD) 压缩为一个无父提交并强制推送到 $REMOTE/web"
info "提交说明: $MESSAGE"
info "注意：这会重写 Gitee 上的提交历史。"

if [ "$YES" -ne 1 ]; then
  read -r -p "继续？[y/N] " answer
  case "$answer" in
    y|Y|yes|YES) ;;
    *) echo "已取消"; exit 1 ;;
  esac
fi

# 直接用当前树创建无父提交，不切换分支、不触碰工作区/索引。
COMMIT="$(git commit-tree "$TREE" -m "$MESSAGE")"
git branch -f publish "$COMMIT" >/dev/null
git push --force "$REMOTE" "$COMMIT:refs/heads/web"

info "发布完成：$REMOTE/web 现在只包含一个提交"
info "  $COMMIT $MESSAGE"
info "本地完整历史仍在 web 分支；快照提交记录在本地 publish 分支。"
