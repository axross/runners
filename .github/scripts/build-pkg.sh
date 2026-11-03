#!/usr/bin/env bash
# builds the macOS flat package for marshal with pkgbuild and writes it to
# <output>. it installs /usr/local/bin/marshal, owner root:wheel and mode 0755,
# under the identifier below, and carries no script. <version> is the installer's
# own X.Y.Z as installer-version.sh prints it. docs/operations/cli.md describes
# the installer. macOS ships bash 3.2, so this uses nothing newer.
set -euo pipefail

identifier="io.github.axross.runners.marshal"

if [[ $# -ne 3 ]]; then
  echo "Usage: build-pkg.sh <binary> <version> <output>" >&2
  exit 2
fi
binary="$1"
version="$2"
output="$3"

refuse() {
  echo "error: $*" >&2
  exit 1
}

number='(0|[1-9][0-9]*)'
[[ "$version" =~ ^${number}\.${number}\.${number}$ ]] \
  || refuse "the installer version '${version}' is not X.Y.Z; map a release version with installer-version.sh first"
[[ -f "$binary" ]] || refuse "${binary} is not a file"
[[ ! -e "$output" ]] || refuse "${output} already exists"

stage="$(mktemp -d "${TMPDIR:-/tmp}/build-pkg.XXXXXX")"
trap 'rm -rf -- "$stage"' EXIT
mkdir -p "${stage}/usr/local/bin"
cp "$binary" "${stage}/usr/local/bin/marshal"
chmod 0755 "${stage}/usr" "${stage}/usr/local" "${stage}/usr/local/bin" "${stage}/usr/local/bin/marshal"

pkgbuild \
  --root "$stage" \
  --identifier "$identifier" \
  --version "$version" \
  --install-location / \
  --ownership recommended \
  "$output"
echo "Built ${output} at version ${version}."
