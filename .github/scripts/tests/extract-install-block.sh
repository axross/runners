#!/usr/bin/env bash
# prints the code of one documented install block: the fenced block that follows
# the marker comment `<!-- install-block: <id> -->` in the Markdown document.
# exits non-zero, printing nothing, unless the marker occurs exactly once and a
# non-empty fence follows it, so a renamed or duplicated marker fails the test.
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "Usage: extract-install-block.sh <markdown file> <block id>" >&2
  exit 2
fi

block="$(awk -v marker="<!-- install-block: $2 -->" '
  $0 == marker { markers++; state = 1; next }
  state == 1 && /^[[:space:]]*$/ { next }
  state == 1 && /^```/ { state = 2; next }
  state == 1 { state = 0 }
  state == 2 && /^```/ { state = 0; fences++; next }
  state == 2 { print }
  END { if (markers != 1 || fences != 1) exit 1 }
' "$1")" || {
  echo "error: $1 does not hold exactly one fenced block after the marker for '$2'" >&2
  exit 1
}
if [[ -z "$block" ]]; then
  echo "error: the block for '$2' is empty" >&2
  exit 1
fi
printf '%s\n' "$block"
