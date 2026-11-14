#!/usr/bin/env bash
# writes standard input to the path $1 as a root-owned file that every user can
# run but only root can change.
set -euo pipefail

launcher=$1
mkdir -p "$(dirname "$launcher")"
staging=$(mktemp "$launcher.XXXXXX")
trap 'rm -f "$staging"' EXIT
cat >"$staging"
test -s "$staging"
chown root:wheel "$staging"
chmod 0755 "$staging"
mv -f "$staging" "$launcher"
