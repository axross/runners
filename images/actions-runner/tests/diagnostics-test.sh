#!/usr/bin/env bash
set -euo pipefail
root="$(mktemp -d)"
trap 'rm -rf -- "$root"' EXIT
g++ -std=c++17 -O2 -Wall -Wextra -Werror images/actions-runner/tests/diagnostics-test.cpp -o "$root/test"
timeout --kill-after=2s 40s "$root/test" "$root/fixtures" "$(basename "$root")"
