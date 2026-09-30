#!/bin/bash

# No -e: a failing check is reported through the exit code below, and a tool that
# is missing must not abort the hook.
set -uo pipefail

PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$PROJECT_DIR" || exit 0

export PATH="$HOME/.local/bin:$PATH"
command -v mise >/dev/null 2>&1 || exit 0
eval "$(mise activate bash)"

changed() {
  if [ -n "$(git status --porcelain 2>/dev/null)" ]; then
    return 0
  fi
  # Baseline for commits ahead: the upstream, else origin/HEAD, else origin/main.
  local baseline
  baseline="$(git rev-parse --abbrev-ref --symbolic-full-name '@{upstream}' 2>/dev/null || true)"
  [ -n "$baseline" ] || baseline="$(git symbolic-ref --short refs/remotes/origin/HEAD 2>/dev/null || true)"
  [ -n "$baseline" ] || baseline="origin/main"
  if [ -n "$(git diff --name-only "$baseline...HEAD" 2>/dev/null)" ]; then
    return 0
  fi
  return 1
}

changed || exit 0

OUTPUT="$(mktemp)"
if ! mise run check >"$OUTPUT" 2>&1; then
  {
    echo "Pre-completion checks failed (mise run check)."
    echo "Fix the errors below before completing the task:"
    echo
    tail -n 100 "$OUTPUT"
  } >&2
  rm -f "$OUTPUT"
  exit 2
fi

rm -f "$OUTPUT"
exit 0
