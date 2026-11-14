#!/usr/bin/env bash
# removes the runner the base ships at $1, so the golden VM holds only the declared
# runner.
set -euo pipefail

directory=$1
rm -rf "$directory"
if [ -e "$directory" ]; then
  echo "error: $directory is still there" >&2
  exit 1
fi
