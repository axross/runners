#!/usr/bin/env bash
# prints what a booted macOS job VM must show before it registers: whether the
# launcher $1 is installed, the version of the runner in $2, the size of the disk
# in bytes, and the bytes its partitions take.
set -euo pipefail

launcher=$1
runner=$2
if [ -x "$launcher" ]; then echo "launcher yes"; else echo "launcher no"; fi
echo "runner $("$runner/bin/Runner.Listener" --version 2>/dev/null || echo none)"
bytes() {
  local size
  size=$(diskutil info "$1" | sed -n 's/^ *Disk Size: .*(\([0-9]*\) Bytes).*$/\1/p')
  if [ -z "$size" ]; then
    echo "error: diskutil gave no size for $1" >&2
    exit 1
  fi
  echo "$size"
}
# each result is assigned before use, so a diskutil that fails stops the script
# rather than leaving an empty list or a zero size
disk=$(bytes disk0)
echo "disk $disk"
partitions=$(diskutil list disk0 | awk '$NF ~ /^disk0s[0-9]+$/ { print $NF }')
total=0
for partition in $partitions; do
  size=$(bytes "$partition")
  total=$((total + size))
done
echo "partitions $total"
