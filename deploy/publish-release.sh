#!/usr/bin/env bash
# 把 dist-release/ 里的发布包推到 GitHub Release 和 Gitee 发行版。
#
# 前置：
#   - 包已由 deploy/build-release.sh 生成在 dist-release/
#   - GitHub：已安装并登录 gh（gh auth login）
#   - Gitee ：已安装 gitee CLI 并登录（printf '%s' "$GITEE_TOKEN" | gitee auth login --with-token）
#
# 用法：
#   bash deploy/publish-release.sh <tag>            # 如 v4.0.31-web
#   bash deploy/publish-release.sh <tag> gh         # 只发 GitHub
#   bash deploy/publish-release.sh <tag> gitee      # 只发 Gitee
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_DIR"

TAG="${1:?用法: bash deploy/publish-release.sh <tag> [gh|gitee]}"
TARGET="${2:-both}"

GITHUB_REPO="${GITHUB_REPO:-dowdsa/cc-switch-web}"
GITEE_REPO="${GITEE_REPO:-lian-haoxiong/cc-switch-web}"
NOTES_FILE="${NOTES_FILE:-$REPO_DIR/dist-release/release-notes.md}"
DIST="$REPO_DIR/dist-release"

info() { printf '\033[1;34m[publish]\033[0m %s\n' "$*"; }
fail() { printf '\033[1;31m[publish]\033[0m %s\n' "$*" >&2; exit 1; }

ASSETS=("$DIST"/*-embedded.tar.gz)
[ -e "${ASSETS[0]}" ] || fail "dist-release/ 里没有 *-embedded.tar.gz，请先跑 deploy/build-release.sh"
[ -f "$NOTES_FILE" ] || fail "缺少发行说明: $NOTES_FILE"

info "tag=$TAG  assets=${#ASSETS[@]}  target=$TARGET"

# 1) 推送 tag（GitHub + Gitee，走 SSH）
git rev-parse -q --verify "refs/tags/$TAG" >/dev/null || fail "本地没有 tag $TAG（先 git tag $TAG publish）"
git push --force github "refs/tags/$TAG" 2>&1 | tail -1 || true
git push --force origin "refs/tags/$TAG" 2>&1 | tail -1 || true

# 2) GitHub Release
if [ "$TARGET" = "both" ] || [ "$TARGET" = "gh" ]; then
  if command -v gh >/dev/null && gh auth status --hostname github.com >/dev/null 2>&1; then
    info "GitHub Release（$GITHUB_REPO）"
    if gh release view "$TAG" --repo "$GITHUB_REPO" >/dev/null 2>&1; then
      gh release upload "$TAG" "${ASSETS[@]}" --clobber --repo "$GITHUB_REPO"
      gh release edit "$TAG" --repo "$GITHUB_REPO" --notes-file "$NOTES_FILE"
    else
      gh release create "$TAG" --repo "$GITHUB_REPO" --title "CC Switch Web $TAG" \
        --notes-file "$NOTES_FILE" "${ASSETS[@]}"
    fi
  else
    info "跳过 GitHub：未安装 gh 或未登录"
  fi
fi

# 3) Gitee 发行版（CLI 建发行版；附件走 API multipart）
if [ "$TARGET" = "both" ] || [ "$TARGET" = "gitee" ]; then
  command -v gitee >/dev/null || fail "未安装 gitee CLI：npm install -g @gitee/gitee-cli"
  gitee auth status 2>&1 | grep -q "Logged in" || fail "gitee 未登录：printf '%s' \"\$GITEE_TOKEN\" | gitee auth login --with-token"
  GITEE_TOKEN="${GITEE_TOKEN:-$(gitee auth token 2>/dev/null)}"
  [ -n "$GITEE_TOKEN" ] || fail "拿不到 Gitee token"

  info "Gitee 发行版（$GITEE_REPO）"
  if ! gitee api "/repos/$GITEE_REPO/releases/tags/$TAG" >/dev/null 2>&1; then
    gitee release create -R "$GITEE_REPO" --tag "$TAG" --name "CC Switch Web $TAG" \
      --body "$(cat "$NOTES_FILE")"
  else
    info "  发行版已存在，跳过创建"
  fi

  REL_ID="$(gitee api "/repos/$GITEE_REPO/releases/tags/$TAG" 2>/dev/null \
    | python3 -c 'import sys,json; print(json.load(sys.stdin)["id"])')"
  for f in "${ASSETS[@]}"; do
    info "  上传 $(basename "$f")"
    curl -sS -X POST "https://gitee.com/api/v5/repos/$GITEE_REPO/releases/$REL_ID/attach_files" \
      -F "access_token=$GITEE_TOKEN" -F "file=@$f" \
      -o /dev/null -w "  HTTP %{http_code}\n"
  done
fi

info "完成"
