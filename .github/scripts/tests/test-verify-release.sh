#!/usr/bin/env bash
# Runs verify-release.sh, the release gate, against temporary release directories:
# the plan phase writing the version to GITHUB_OUTPUT, the release phase passing
# with the exact files and refusing a missing file, an extra file, a binary named
# for another version, a SHA256SUMS that lists one binary only, and a checksum
# mismatch, and an unset or unknown RELEASE_PHASE. Each case checks the exit status
# and the cause the script names.
set -euo pipefail

gate="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/verify-release.sh"
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT

version=1.2.3
windows="marshal-${version}-windows-x64.exe"
macos="marshal-${version}-macos-arm64"

# release_files <directory> [<windows name> [<macos name>]]: two binaries and the
# SHA256SUMS that lists them.
release_files() {
  local directory="$1" windows_name="${2:-$windows}" macos_name="${3:-$macos}"
  mkdir -p "$directory"
  printf 'windows binary\n' >"${directory}/${windows_name}"
  printf 'macos binary\n' >"${directory}/${macos_name}"
  (cd "$directory" && sha256sum "$macos_name" "$windows_name" >SHA256SUMS)
}

failures=0

# expect <case> <exit status> <text the output must contain> <RELEASE_PHASE or -unset> <gate arguments...>
expect() {
  local name="$1" want_status="$2" want_text="$3" phase="$4" output status=0
  shift 4
  if [[ "$phase" == "-unset" ]]; then
    output="$(env -u RELEASE_PHASE bash "$gate" "$@" 2>&1)" || status=$?
  else
    output="$(RELEASE_PHASE="$phase" bash "$gate" "$@" 2>&1)" || status=$?
  fi
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

# The plan phase appends to GITHUB_OUTPUT, so a line already there must stay.
output_file="${work}/github-output"
printf 'earlier=1\n' >"$output_file"
GITHUB_OUTPUT="$output_file" expect "the plan phase hands out the version" 0 "The next release is ${version}" \
  plan "$version" "${work}/no-files"
if [[ "$(cat "$output_file")" != "earlier=1
version=${version}" ]]; then
  echo "FAIL the plan phase wrote to GITHUB_OUTPUT: $(cat "$output_file")" >&2
  failures=$((failures + 1))
else
  echo "ok   the plan phase appends version=${version} to GITHUB_OUTPUT"
fi
# empty rather than unset, since a CI job running this test has its own.
GITHUB_OUTPUT="" expect "the plan phase without GITHUB_OUTPUT" 1 "GITHUB_OUTPUT is not set" \
  plan "$version" "${work}/no-files"

release_files "${work}/exact"
expect "the release phase with the exact files" 0 "are complete and match SHA256SUMS" \
  release "$version" "${work}/exact"

release_files "${work}/missing"
rm "${work}/missing/${macos}"
expect "a missing binary" 1 "are not exactly ${macos}, ${windows} and SHA256SUMS" \
  release "$version" "${work}/missing"

release_files "${work}/extra"
printf 'helper\n' >"${work}/extra/fake-docker"
expect "an extra file" 1 "are not exactly ${macos}, ${windows} and SHA256SUMS" \
  release "$version" "${work}/extra"

release_files "${work}/other-version" "marshal-1.2.4-windows-x64.exe"
expect "a binary named for another version" 1 "are not exactly ${macos}, ${windows} and SHA256SUMS" \
  release "$version" "${work}/other-version"

release_files "${work}/one-listed"
(cd "${work}/one-listed" && sha256sum "$macos" >SHA256SUMS)
expect "a SHA256SUMS that lists one binary" 1 "SHA256SUMS does not list exactly ${macos} and ${windows}" \
  release "$version" "${work}/one-listed"

release_files "${work}/mismatch"
printf 'tampered\n' >>"${work}/mismatch/${windows}"
expect "a checksum mismatch" 1 "a release file does not match SHA256SUMS" \
  release "$version" "${work}/mismatch"

expect "an unset RELEASE_PHASE" 1 "RELEASE_PHASE must be plan or release, not ''" \
  -unset "$version" "${work}/exact"
expect "an unknown RELEASE_PHASE" 1 "RELEASE_PHASE must be plan or release, not 'publish'" \
  publish "$version" "${work}/exact"

if [[ $failures -ne 0 ]]; then
  echo "${failures} release gate case(s) failed." >&2
  exit 1
fi
echo "All release gate cases passed."
