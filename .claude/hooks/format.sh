#!/bin/bash

# no -e: formatting is best effort, and no failure here may block the edit.
set -uo pipefail

PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
PROJECT_DIR="${PROJECT_DIR%/}"

FILE_PATH="$(jq -r '.tool_input.file_path // empty' 2>/dev/null || true)"

case "$FILE_PATH" in
  "$PROJECT_DIR"/*.md | "$PROJECT_DIR"/*.json | "$PROJECT_DIR"/*.jsonc | "$PROJECT_DIR"/*.yaml | "$PROJECT_DIR"/*.yml | "$PROJECT_DIR"/*.ts | "$PROJECT_DIR"/*.js | "$PROJECT_DIR"/*.mjs | "$PROJECT_DIR"/*.rs) ;;
  *) exit 0 ;;
esac

cd "$PROJECT_DIR" || exit 0

export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"

# run from the crate so rust-toolchain.toml selects rustfmt; RUSTUP_AUTO_INSTALL=0
# keeps an edit from starting a toolchain download.
case "$FILE_PATH" in
  *.rs)
    command -v cargo >/dev/null 2>&1 || exit 0
    (cd cli && RUSTUP_AUTO_INSTALL=0 cargo fmt) >/dev/null 2>&1 || true
    exit 0
    ;;
esac

command -v mise >/dev/null 2>&1 || exit 0
eval "$(mise activate bash)"
command -v prettier >/dev/null 2>&1 || exit 0

FILE_REL="${FILE_PATH#"$PROJECT_DIR"/}"
prettier --write --ignore-unknown -- "$FILE_REL" >/dev/null 2>&1 || true
exit 0
