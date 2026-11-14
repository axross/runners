#!/bin/sh
# reads the jit configuration from standard input into the runner's environment and
# starts the runner through its own run.sh in the directory $1, so a runner that
# updates itself is restarted; the value never appears in an argument list and is
# not echoed, logged or written to a file.
# POSIX sh has no pipefail, and this script has no pipeline for it to cover.
set -eu

limit=65536
jit=$(head -c "$((limit + 1))")
if [ -z "$jit" ]; then
  echo "marshal-launch: standard input held no jit configuration" >&2
  exit 64
fi
if [ "${#jit}" -gt "$limit" ]; then
  echo "marshal-launch: the jit configuration on standard input is longer than $limit bytes" >&2
  exit 64
fi
ACTIONS_RUNNER_INPUT_JITCONFIG=$jit
export ACTIONS_RUNNER_INPUT_JITCONFIG
cd "$1"
exec ./run.sh
