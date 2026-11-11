#!/usr/bin/env bash
# prints the exact version of `tauri-cli` that app/tauri-cli.version pins, and
# refuses a file that is not one plain X.Y.Z line, so a range or a prefix never
# reaches `cargo install`. the one pin the app's CI job reads, and the release
# build reads later; see docs/operations/app.md.
set -euo pipefail

if [[ $# -gt 1 ]]; then
  echo "Usage: tauri-cli-version.sh [pin-file]" >&2
  exit 2
fi
file="${1:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/app/tauri-cli.version}"

if [[ ! -f "$file" ]]; then
  echo "error: ${file} does not exist" >&2
  exit 1
fi
# one line, so a second version or a comment cannot be read as the pin; a CR from a
# checkout with converted line endings is not part of it.
mapfile -t lines <"$file"
version="${lines[0]-}"
version="${version%$'\r'}"
number='(0|[1-9][0-9]*)'
if [[ ${#lines[@]} -ne 1 || ! "$version" =~ ^${number}\.${number}\.${number}$ ]]; then
  echo "error: ${file} must hold one exact X.Y.Z version and nothing else" >&2
  exit 1
fi
echo "$version"
