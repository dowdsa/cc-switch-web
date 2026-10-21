#!/usr/bin/env bash
# 把本地 web 分支的当前树发布为 Gitee / GitHub 上的单提交快照。
# Gitee 分支 web，GitHub 分支 main；两个仓库都不含上游历史。
# 本地仍保留完整历史用于同步上游。
#
# 用法:
#   bash scripts/publish-repos.sh          # 交互确认
#   bash scripts/publish-repos.sh --yes    # 非交互
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

YES=0
[ "${1:-}" = "--yes" ] && YES=1

info() { printf '\033[1;34m[publish]\033[0m %s\n' "$*"; }
fail() { printf '\033[1;31m[publish]\033[0m %s\n' "$*" >&2; exit 1; }

[ -n "$(git status --porcelain)" ] && fail "工作区有未提交改动，请先提交。"
[ "$(git branch --show-current)" = "web" ] || fail "请先切换到 web 分支。"

GITEE_REMOTE="${GITEE_REMOTE:-origin}"
GITHUB_REMOTE="${GITHUB_REMOTE:-github}"
BASE_TAG="$(git describe --tags --abbrev=0 2>/dev/null || echo unknown)"
MESSAGE="CC Switch Web snapshot (based on upstream ${BASE_TAG})"
TREE="$(git rev-parse "web^{tree}")"

info "将把 web@$(git rev-parse --short HEAD) 压缩为一个无父提交"
info "提交说明: $MESSAGE"
info "推送到 Gitee ($GITEE_REMOTE/web)；若存在 GitHub 远端则同时推送到 $GITHUB_REMOTE/main"
info "注意：这会重写两个远程仓库的提交历史。"

if [ "$YES" -ne 1 ]; then
  read -r -p "继续？[y/N] " answer
  case "$answer" in
    y|Y|yes|YES) ;;
    *) echo "已取消"; exit 1 ;;
  esac
fi

# 上游 CI / Dependabot / 维护配置不适用于 Web 仓库，即使上游合并后重新出现也从快照中过滤。
FILTER_PATHS=(
  .github/workflows
  .github/actions
  .github/dependabot.yml
  .github/labeler.yml
  .github/CODEOWNERS
  .github/FUNDING.yml
)
INDEX_FILE="$(mktemp)"
GIT_INDEX_FILE="$INDEX_FILE" git read-tree "$TREE"
GIT_INDEX_FILE="$INDEX_FILE" git rm -r --cached --ignore-unmatch "${FILTER_PATHS[@]}" >/dev/null 2>&1 || true
SNAPSHOT_TREE="$(GIT_INDEX_FILE="$INDEX_FILE" git write-tree)"
rm -f "$INDEX_FILE"

# 用过滤后的树创建无父提交，不切换分支、不触碰工作区。
COMMIT="$(git commit-tree "$SNAPSHOT_TREE" -m "$MESSAGE")"
git branch -f publish "$COMMIT" >/dev/null
git push --force "$GITEE_REMOTE" "$COMMIT:refs/heads/web"

if git remote get-url "$GITHUB_REMOTE" >/dev/null 2>&1; then
  if command -v gh >/dev/null 2>&1 && gh auth status --hostname github.com >/dev/null 2>&1; then
    info "推送到 GitHub ($GITHUB_REMOTE/main)"
    git -c credential.helper='!gh auth git-credential' push --force "$GITHUB_REMOTE" "$COMMIT:refs/heads/main"
  else
    info "跳过 GitHub：未安装 gh 或未登录（gh auth login）"
  fi
fi

info "发布完成"
info "  $COMMIT $MESSAGE"
info "本地完整历史仍在 web 分支；快照提交记录在本地 publish 分支。"
