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

# a network failure must not fail a session start, and the Rust tasks then fail
# at once with a line naming this script; output goes to stderr, not the context.
export PATH="$HOME/.cargo/bin:$PATH"
if [ -x .agents/install-rust ]; then
  .agents/install-rust >&2 || echo "warning: .agents/install-rust failed, so the Rust gates are unavailable until it succeeds (see docs/operations/agent-sessions.md)" >&2
fi
if [ -n "${CLAUDE_ENV_FILE:-}" ] && ! grep -Fq '.cargo/bin' "$CLAUDE_ENV_FILE" 2>/dev/null; then
  # shellcheck disable=SC2016 # the line is written literally and expanded when the env file is sourced
  echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> "$CLAUDE_ENV_FILE"
fi

if [ -f .claude/settings.local-example.json ]; then
  cp -f .claude/settings.local-example.json .claude/settings.local.json
fi

echo "REMINDER: read AGENTS.md and follow its Response Approach for every task."
