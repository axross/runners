#!/usr/bin/env bash
# runs installer-version.sh against release, dry-run, and refused versions, and
# checks the exit status and the text it prints or the cause it names.
set -euo pipefail

script="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/installer-version.sh"

failures=0

# expect <case> <exit status> <text the output must contain> <version>
expect() {
  local name="$1" want_status="$2" want_text="$3" version="$4" output status=0
  output="$(bash "$script" "$version" 2>&1)" || status=$?
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

# expect_exact <case> <version> <the only line it must print>
expect_exact() {
  local name="$1" version="$2" want="$3" output
  output="$(bash "$script" "$version" 2>&1)" || true
  if [[ "$output" != "$want" ]]; then
    echo "FAIL ${name}: printed '${output}', expected '${want}'" >&2
    failures=$((failures + 1))
  else
    echo "ok   ${name}"
  fi
}

expect_exact "a release version is kept" 1.2.3 1.2.3
expect_exact "the lowest version is kept" 0.0.0 0.0.0
expect_exact "the highest version is kept" 255.255.65535 255.255.65535
expect_exact "a dry-run version maps to 0.0.0" 0.0.0-dryrun.7 0.0.0
expect_exact "a dry-run version with a long run number maps to 0.0.0" 0.0.0-dryrun.1234567 0.0.0

beyond="beyond the installer limits"
expect "a major beyond 255" 1 "$beyond" 256.0.0
expect "a minor beyond 255" 1 "$beyond" 1.256.0
expect "a patch beyond 65535" 1 "$beyond" 1.0.65536
expect "a patch with a very long digit string" 1 "$beyond" 1.0.99999999999999999999999

other="not X.Y.Z or 0.0.0-dryrun.<n>"
expect "another prerelease" 1 "$other" 1.2.3-rc.1
expect "a dry-run form on another version" 1 "$other" 1.2.3-dryrun.4
expect "a dry-run form with no number" 1 "$other" 0.0.0-dryrun.
expect "a dry-run form with a leading zero" 1 "$other" 0.0.0-dryrun.01
expect "the placeholder version" 1 "$other" 0.0.0-dev
expect "build metadata" 1 "$other" 1.2.3+build
expect "a leading zero" 1 "$other" 01.2.3
expect "two parts" 1 "$other" 1.2
expect "four parts" 1 "$other" 1.2.3.4
expect "an empty version" 1 "$other" ""

status=0
bash "$script" >/dev/null 2>&1 || status=$?
if [[ $status -ne 2 ]]; then
  echo "FAIL no argument: exit ${status}, expected 2" >&2
  failures=$((failures + 1))
else
  echo "ok   no argument"
fi

if [[ $failures -ne 0 ]]; then
  echo "${failures} installer version case(s) failed." >&2
  exit 1
fi
echo "All installer version cases passed."
