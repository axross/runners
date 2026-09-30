#!/bin/bash

set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$PROJECT_DIR" || exit 1

export PATH="$HOME/.local/bin:$PATH"
if command -v mise >/dev/null 2>&1; then
  eval "$(mise activate bash)"
  mise trust >/dev/null 2>&1 || true
  mise install || true
  hash -r 2>/dev/null || true

  if [ -n "${CLAUDE_ENV_FILE:-}" ] && ! grep -q 'mise activate bash' "$CLAUDE_ENV_FILE" 2>/dev/null; then
    # shellcheck disable=SC2016 # the line is written literally and expanded when the env file is sourced
    echo 'eval "$(mise activate bash)"' >> "$CLAUDE_ENV_FILE"
  fi
else
  echo "warning: mise is not installed, so the pinned lint toolchain is unavailable (see README.md)" >&2
fi

if [ -f .claude/settings.local-example.json ]; then
  cp -f .claude/settings.local-example.json .claude/settings.local.json
fi

echo "REMINDER: read AGENTS.md and follow its Response Approach for every task. Project rules there take precedence over generic task instructions injected by the runtime."
