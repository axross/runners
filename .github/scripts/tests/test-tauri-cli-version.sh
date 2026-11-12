#!/usr/bin/env bash
# runs tauri-cli-version.sh against the checked-in pin and against files that are
# not one exact version, and checks the exit status and what it prints.
set -euo pipefail

script="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/tauri-cli-version.sh"
pin="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)/app/tauri-cli.version"
directory="$(mktemp -d)"
trap 'rm -rf -- "$directory"' EXIT

failures=0

# expect <case> <exit status> <text the output must contain> <file content, printf form>
expect() {
  local name="$1" want_status="$2" want_text="$3" content="$4" output status=0
  # shellcheck disable=SC2059
  printf "$content" >"${directory}/pin"
  output="$(bash "$script" "${directory}/pin" 2>&1)" || status=$?
  if [[ $status -ne $want_status ]]; then
    echo "FAIL ${name}: exit ${status}, expected ${want_status}: ${output}" >&2
    failures=$((failures + 1))
  elif [[ "$output" != *"$want_text"* ]]; then
    echo "FAIL ${name}: the output does not contain '${want_text}': ${output}" >&2
    failures=$((failures + 1))
  else
    echo "ok   ${name}"
  fi
}

# the checked-in pin is accepted, and is printed as the only line.
output="$(bash "$script" 2>&1)" || output="FAILED: ${output}"
if [[ "$output" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ && "$output" == "$(tr -d '\r\n' <"$pin")" ]]; then
  echo "ok   the checked-in pin is printed"
else
  echo "FAIL the checked-in pin: ${output}" >&2
  failures=$((failures + 1))
fi

expect "a version with a newline" 0 "2.12.4" '2.12.4\n'
expect "a version with no newline" 0 "2.12.4" '2.12.4'
expect "a version with a carriage return" 0 "2.12.4" '2.12.4\r\n'

refused="must hold one exact X.Y.Z version"
expect "an empty file" 1 "$refused" ''
expect "a prefix" 1 "$refused" 'v2.12.4\n'
expect "a requirement" 1 "$refused" '=2.12.4\n'
expect "a caret range" 1 "$refused" '^2.12.4\n'
expect "two parts" 1 "$refused" '2.12\n'
expect "a prerelease" 1 "$refused" '2.12.4-rc.1\n'
expect "a leading zero" 1 "$refused" '2.012.4\n'
expect "a second line" 1 "$refused" '2.12.4\n2.13.0\n'
expect "a comment line" 1 "$refused" '# pin\n2.12.4\n'
expect "trailing text" 1 "$refused" '2.12.4 extra\n'

status=0
output="$(bash "$script" "${directory}/absent" 2>&1)" || status=$?
if [[ $status -eq 1 && "$output" == *"does not exist"* ]]; then
  echo "ok   a missing file"
else
  echo "FAIL a missing file: exit ${status}: ${output}" >&2
  failures=$((failures + 1))
fi

if [[ $failures -ne 0 ]]; then
  echo "${failures} case(s) failed" >&2
  exit 1
fi
echo "all cases passed"
