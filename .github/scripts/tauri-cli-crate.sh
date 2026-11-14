#!/usr/bin/env bash
# prints the version and the SHA-256 of the `tauri-cli` crate archive that
# app/tauri-cli.crate.sha256 commits, as `<version> <sha256>`, and refuses the
# file unless it is one `<sha256>  tauri-cli-<version>.crate` line whose version
# is the pin in app/tauri-cli.version. the release run's update-tool job builds
# the update signer from that archive only; `mise run check` runs this, so a
# change of the pin without the checksum fails. see docs/operations/app.md.
set -euo pipefail

if [[ $# -gt 2 ]]; then
  echo "Usage: tauri-cli-crate.sh [checksum-file [pin-file]]" >&2
  exit 2
fi
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
app="$(cd "${here}/../.." && pwd)/app"
file="${1:-${app}/tauri-cli.crate.sha256}"
pin="$(bash "${here}/tauri-cli-version.sh" "${2:-${app}/tauri-cli.version}")"

if [[ ! -f "$file" ]]; then
  echo "error: ${file} does not exist" >&2
  exit 1
fi
mapfile -t lines <"$file"
line="${lines[0]-}"
line="${line%$'\r'}"
number='(0|[1-9][0-9]*)'
pattern="^([0-9a-f]{64})  tauri-cli-(${number}\.${number}\.${number})\.crate$"
if [[ ${#lines[@]} -ne 1 || ! "$line" =~ $pattern ]]; then
  echo "error: ${file} must hold one '<sha256>  tauri-cli-<X.Y.Z>.crate' line and nothing else" >&2
  exit 1
fi
sha256="${BASH_REMATCH[1]}"
version="${BASH_REMATCH[2]}"
if [[ "$version" != "$pin" ]]; then
  echo "error: ${file} commits tauri-cli ${version}, and app/tauri-cli.version pins ${pin}; move the checksum with the pin" >&2
  exit 1
fi
echo "${version} ${sha256}"
