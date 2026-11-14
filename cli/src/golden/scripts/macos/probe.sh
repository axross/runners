#!/bin/sh
# prints what a booted macOS job VM must show before it registers: whether the
# launcher $1 is installed, the version of the runner in $2, the size of the disk
# in bytes, and the bytes its partitions take.
set -eu

launcher=$1
runner=$2
if [ -x "$launcher" ]; then echo "launcher yes"; else echo "launcher no"; fi
echo "runner $("$runner/bin/Runner.Listener" --version 2>/dev/null || echo none)"
bytes() { diskutil info "$1" | sed -n 's/^ *Disk Size: .*(\([0-9]*\) Bytes).*$/\1/p'; }
echo "disk $(bytes disk0)"
total=0
for partition in $(diskutil list disk0 | awk '$NF ~ /^disk0s[0-9]+$/ { print $NF }'); do
  total=$((total + $(bytes "$partition")))
done
echo "partitions $total"
