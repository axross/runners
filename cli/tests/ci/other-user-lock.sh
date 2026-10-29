#!/usr/bin/env bash
# shows that another OS user cannot take the instance lock while this user holds
# it, with a copy of the debug helper run as nobody through sudo;
# docs/operations/cli.md describes the test. CI runs it on the Linux and macOS
# hosted runners, whose account has passwordless sudo.
set -euo pipefail

helper="${1:?usage: other-user-lock.sh <instance-helper>}"
held_exit_code=3

# under /tmp, not the runner's temporary directory, so the nobody account can
# reach it on macOS too, where a user's own temporary directory is private
work_dir="$(mktemp -d /tmp/marshal-other-user.XXXXXX)"
holder=""
cleanup() {
  if [ -n "$holder" ]; then
    kill "$holder" 2>/dev/null || true
    wait "$holder" 2>/dev/null || true
  fi
  rm -rf -- "$work_dir"
}
trap cleanup EXIT

chmod 755 "$work_dir"
cp "$helper" "$work_dir/instance-helper"
chmod 755 "$work_dir/instance-helper"
mkdir "$work_dir/root"
chmod 755 "$work_dir/root"

"$work_dir/instance-helper" hold --root "$work_dir/root" > "$work_dir/holder.out" &
holder=$!
for _ in $(seq 1 100); do
  if grep -qx locked "$work_dir/holder.out"; then
    break
  fi
  sleep 0.1
done
if ! grep -qx locked "$work_dir/holder.out"; then
  echo "The helper did not take the lock." >&2
  exit 1
fi
ls -l "$work_dir/root/.marshal-instance.lock"

status=0
output="$(sudo -n -u nobody "$work_dir/instance-helper" try --root "$work_dir/root" 2>&1)" \
  || status=$?
printf '%s\n' "$output"
if [ "$status" -eq 0 ]; then
  echo "nobody took the lock while it was held." >&2
  exit 1
fi
if [ "$status" -ne "$held_exit_code" ]; then
  echo "nobody was refused with exit code $status, not $held_exit_code (held)." >&2
  exit 1
fi
grep -q '^refused: the instance lock is held' <<< "$output"
echo "nobody was refused the held lock and exited $status."
