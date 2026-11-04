#!/usr/bin/env bash
# downloads the key at $1 to the path $2, which becomes readable by every user
# only once the whole download is in place.
set -euo pipefail

url=$1
destination=$2
install --directory --mode 0755 "$(dirname "$destination")"
staging=$(mktemp "$destination.XXXXXX")
trap 'rm -f "$staging"' EXIT
curl --fail --silent --show-error --location \
  --proto '=https' --proto-redir '=https' --tlsv1.2 \
  --max-filesize 65536 --max-time 60 --retry 3 \
  --output "$staging" "$url"
test -s "$staging"
chmod 0644 "$staging"
mv -f "$staging" "$destination"
