#!/usr/bin/env bash
# runs verify-release.sh, the release gate, against temporary release
# directories, and checks that the asset globs in release/.releaserc.json match
# exactly the files the gate passes, so the upload and the gate agree on the
# names. each case checks the exit status and the cause the script names.
set -euo pipefail

gate="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/verify-release.sh"
config="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)/release/.releaserc.json"
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT

version=1.2.3
windows="marshal-${version}-windows-x64.exe"
macos="marshal-${version}-macos-arm64"

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

# the plan phase appends to GITHUB_OUTPUT, so a line already there must stay.
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

# the asset globs @semantic-release/github uploads, relative to release/, must
# each match one file and together the two binaries and SHA256SUMS.
expect_assets() {
  local directory="$1" pattern hits names=() problem=""
  local -a assets
  mapfile -t assets < <(node -e '
    const config = JSON.parse(require("node:fs").readFileSync(process.argv[1], "utf8"));
    const github = config.plugins.find((plugin) => [].concat(plugin)[0] === "@semantic-release/github");
    for (const asset of github[1].assets) console.log(typeof asset === "string" ? asset : asset.path);
  ' "$config")
  if [[ ${#assets[@]} -ne 3 ]]; then
    problem="${config} lists ${#assets[@]} asset globs, not 3"
  fi
  for pattern in "${assets[@]}"; do
    [[ -z "$problem" ]] || break
    if [[ "$pattern" != ../release-files/* ]]; then
      problem="the asset glob ${pattern} is not under ../release-files/"
      break
    fi
    hits="$(cd "$directory" && shopt -s nullglob && for file in ${pattern#../release-files/}; do if [[ -e "$file" ]]; then echo "$file"; fi; done)"
    if [[ -z "$hits" || "$hits" == *$'\n'* ]]; then
      problem="the asset glob ${pattern} matches '${hits//$'\n'/ }', not one file"
      break
    fi
    names+=("$hits")
  done
  if [[ -z "$problem" && "$(printf '%s\n' "${names[@]}" | sort)" != "$(printf '%s\n' SHA256SUMS "$macos" "$windows" | sort)" ]]; then
    problem="the asset globs match ${names[*]}, not ${macos}, ${windows} and SHA256SUMS"
  fi
  if [[ -n "$problem" ]]; then
    echo "FAIL the asset globs match the release files: ${problem}" >&2
    failures=$((failures + 1))
  else
    echo "ok   the asset globs match the release files"
  fi
}
expect_assets "${work}/exact"

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

release_files "${work}/directory"
rm "${work}/directory/${macos}"
mkdir "${work}/directory/${macos}"
expect "a binary that is a directory" 1 "${macos} is not a regular file" \
  release "$version" "${work}/directory"

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
