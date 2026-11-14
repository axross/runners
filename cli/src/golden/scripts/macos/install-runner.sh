#!/usr/bin/env bash
# downloads the runner archive from $1, refuses it unless its SHA-256 is $2, and
# unpacks it into the new directory $3, owned by the user $4.
set -euo pipefail

url=$1
expected=$2
directory=$3
owner=$4
archive=$(mktemp /tmp/marshal-runner.XXXXXX)
trap 'rm -f "$archive"' EXIT
curl --fail --silent --show-error --location --proto '=https' --max-time 540 \
  --output "$archive" "$url"
actual=$(shasum -a 256 "$archive" | awk '{ print $1 }')
if [ "$actual" != "$expected" ]; then
  echo "error: the runner archive's SHA-256 is $actual, not the recorded $expected" >&2
  exit 1
fi
mkdir "$directory"
tar -xzf "$archive" -C "$directory"
chown -R "$owner:staff" "$directory"
if [ ! -x "$directory/run.sh" ]; then
  echo "error: the runner archive holds no run.sh" >&2
  exit 1
fi
