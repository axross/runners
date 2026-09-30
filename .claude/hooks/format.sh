#!/bin/bash

set -uo pipefail

PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
PROJECT_DIR="${PROJECT_DIR%/}"

FILE_PATH="$(jq -r '.tool_input.file_path // empty' 2>/dev/null || true)"

case "$FILE_PATH" in
  "$PROJECT_DIR"/*.md | "$PROJECT_DIR"/*.json | "$PROJECT_DIR"/*.jsonc | "$PROJECT_DIR"/*.yaml | "$PROJECT_DIR"/*.yml | "$PROJECT_DIR"/*.ts | "$PROJECT_DIR"/*.js | "$PROJECT_DIR"/*.mjs) ;;
  *) exit 0 ;;
esac

cd "$PROJECT_DIR" || exit 0

export PATH="$HOME/.local/bin:$PATH"
command -v mise >/dev/null 2>&1 || exit 0
eval "$(mise activate bash)"
command -v prettier >/dev/null 2>&1 || exit 0

FILE_REL="${FILE_PATH#"$PROJECT_DIR"/}"
prettier --write --ignore-unknown -- "$FILE_REL" >/dev/null 2>&1 || true
exit 0
