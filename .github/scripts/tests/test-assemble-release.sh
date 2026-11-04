#!/usr/bin/env bash
# runs assemble-release.sh against temporary directories: the four SHA256SUMS
# lines in their fixed order that sha256sum accepts, and a refusal for each way
# the directory or a hash can be wrong. each case checks the exit status and the
# cause the script names, and a refusal also checks that no SHA256SUMS was left.
set -euo pipefail

script="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/assemble-release.sh"
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT

version=1.2.3
windows="marshal-${version}-windows-x64.exe"
windows_msi="marshal-${version}-windows-x64.msi"
macos="marshal-${version}-macos-arm64"
macos_pkg="marshal-${version}-macos-arm64.pkg"

hash_of() {
  local line
  line="$(sha256sum "$1")"
  echo "${line%% *}"
}

# make_set <directory>: the four files, each with its own content.
make_set() {
  mkdir -p "$1"
  printf 'windows binary\n' >"$1/$windows"
  printf 'windows installer\n' >"$1/$windows_msi"
  printf 'macos binary\n' >"$1/$macos"
  printf 'macos installer\n' >"$1/$macos_pkg"
}

failures=0

# expect <case> <exit status> <text the output must contain> <directory> <hash arguments...>
# runs the script on the version above, and for a refusal checks that the directory
# holds no SHA256SUMS afterwards.
expect() {
  local name="$1" want_status="$2" want_text="$3" directory="$4" output status=0
  shift 4
  output="$(bash "$script" "$version" "$directory" "$@" 2>&1)" || status=$?
  if [[ $status -ne $want_status ]]; then
    echo "FAIL ${name}: exit ${status}, expected ${want_status}: ${output}" >&2
    failures=$((failures + 1))
  elif [[ "$output" != *"$want_text"* ]]; then
    echo "FAIL ${name}: the output does not contain '${want_text}': ${output}" >&2
    failures=$((failures + 1))
  elif [[ $want_status -ne 0 && -e "${directory}/SHA256SUMS" ]]; then
    echo "FAIL ${name}: the refusal left a SHA256SUMS behind" >&2
    failures=$((failures + 1))
  else
    echo "ok   ${name}"
  fi
}

# the hashes in the order the script takes them.
hashes() {
  echo "$(hash_of "$1/$windows")" "$(hash_of "$1/$windows_msi")" "$(hash_of "$1/$macos")" "$(hash_of "$1/$macos_pkg")"
}

make_set "${work}/exact"
# shellcheck disable=SC2046 # the four hashes are separate arguments.
expect "the exact files" 0 "$macos_pkg" "${work}/exact" $(hashes "${work}/exact")
want="$(
  printf '%s  %s\n' \
    "$(hash_of "${work}/exact/$macos")" "$macos" \
    "$(hash_of "${work}/exact/$macos_pkg")" "$macos_pkg" \
    "$(hash_of "${work}/exact/$windows")" "$windows" \
    "$(hash_of "${work}/exact/$windows_msi")" "$windows_msi"
)"
if [[ "$(cat "${work}/exact/SHA256SUMS")" != "$want" ]]; then
  echo "FAIL the SHA256SUMS lines or their order are wrong: $(cat "${work}/exact/SHA256SUMS")" >&2
  failures=$((failures + 1))
elif ! (cd "${work}/exact" && sha256sum --check --strict SHA256SUMS >/dev/null); then
  echo "FAIL sha256sum --check --strict rejects the SHA256SUMS" >&2
  failures=$((failures + 1))
else
  echo "ok   the four lines are in the fixed order and sha256sum accepts them"
fi

# a dry run's version carries a prerelease.
make_set "${work}/dry"
mv "${work}/dry/$macos" "${work}/dry/marshal-0.0.0-dryrun.7-macos-arm64"
mv "${work}/dry/$macos_pkg" "${work}/dry/marshal-0.0.0-dryrun.7-macos-arm64.pkg"
mv "${work}/dry/$windows" "${work}/dry/marshal-0.0.0-dryrun.7-windows-x64.exe"
mv "${work}/dry/$windows_msi" "${work}/dry/marshal-0.0.0-dryrun.7-windows-x64.msi"
dry_hashes="$(cd "${work}/dry" && sha256sum marshal-0.0.0-dryrun.7-windows-x64.exe marshal-0.0.0-dryrun.7-windows-x64.msi marshal-0.0.0-dryrun.7-macos-arm64 marshal-0.0.0-dryrun.7-macos-arm64.pkg | awk '{ printf "%s ", $1 }')"
dry_status=0
# shellcheck disable=SC2086 # the four hashes are separate arguments.
dry_output="$(bash "$script" 0.0.0-dryrun.7 "${work}/dry" $dry_hashes 2>&1)" || dry_status=$?
if [[ $dry_status -ne 0 || "$(wc -l <"${work}/dry/SHA256SUMS")" -ne 4 ]]; then
  echo "FAIL a dry run's prerelease version: exit ${dry_status}: ${dry_output}" >&2
  failures=$((failures + 1))
else
  echo "ok   a dry run's prerelease version"
fi

make_set "${work}/changed"
# shellcheck disable=SC2046 # the four hashes are separate arguments.
set -- $(hashes "${work}/changed")
printf 'changed\n' >>"${work}/changed/$macos_pkg"
expect "a changed package" 1 "${macos_pkg} has SHA-256" "${work}/changed" "$@"

make_set "${work}/changed-windows"
# shellcheck disable=SC2046 # the four hashes are separate arguments.
set -- $(hashes "${work}/changed-windows")
printf 'changed\n' >>"${work}/changed-windows/$windows"
expect "a changed Windows binary" 1 "${windows} has SHA-256" "${work}/changed-windows" "$@"

make_set "${work}/swapped"
# shellcheck disable=SC2046 # the four hashes are separate arguments.
set -- $(hashes "${work}/swapped")
expect "two hashes handed over in the wrong order" 1 "has SHA-256" "${work}/swapped" "$2" "$1" "$3" "$4"

make_set "${work}/bad-hash"
# shellcheck disable=SC2046 # the four hashes are separate arguments.
set -- $(hashes "${work}/bad-hash")
expect "a hash that is not a SHA-256" 1 "is not a SHA-256" "${work}/bad-hash" "$1" "$2" "$3" "not-a-hash"
expect "an empty hash" 1 "is not a SHA-256" "${work}/bad-hash" "$1" "$2" "" "$4"

make_set "${work}/missing"
# shellcheck disable=SC2046 # the four hashes are separate arguments.
set -- $(hashes "${work}/missing")
rm "${work}/missing/$macos"
expect "a missing binary" 1 "are not exactly ${macos}, ${macos_pkg}, ${windows} and ${windows_msi}" "${work}/missing" "$@"

make_set "${work}/extra"
# shellcheck disable=SC2046 # the four hashes are separate arguments.
set -- $(hashes "${work}/extra")
printf 'helper\n' >"${work}/extra/fake-docker"
expect "an extra file" 1 "are not exactly" "${work}/extra" "$@"

make_set "${work}/other-version"
# shellcheck disable=SC2046 # the four hashes are separate arguments.
set -- $(hashes "${work}/other-version")
mv "${work}/other-version/$windows_msi" "${work}/other-version/marshal-1.2.4-windows-x64.msi"
expect "an installer named for another version" 1 "are not exactly" "${work}/other-version" "$@"

make_set "${work}/old-sums"
# shellcheck disable=SC2046 # the four hashes are separate arguments.
set -- $(hashes "${work}/old-sums")
(cd "${work}/old-sums" && sha256sum "$macos" >SHA256SUMS)
status=0
output="$(bash "$script" "$version" "${work}/old-sums" "$@" 2>&1)" || status=$?
if [[ $status -ne 1 || "$output" != *"are not exactly"* ]]; then
  echo "FAIL an earlier SHA256SUMS in the directory: exit ${status}: ${output}" >&2
  failures=$((failures + 1))
else
  echo "ok   an earlier SHA256SUMS in the directory"
fi

make_set "${work}/directory"
# shellcheck disable=SC2046 # the four hashes are separate arguments.
set -- $(hashes "${work}/directory")
rm "${work}/directory/$macos_pkg"
mkdir "${work}/directory/$macos_pkg"
expect "a package that is a directory" 1 "${macos_pkg} is not a regular file" "${work}/directory" "$@"

make_set "${work}/symlink"
# shellcheck disable=SC2046 # the four hashes are separate arguments.
set -- $(hashes "${work}/symlink")
mv "${work}/symlink/$macos" "${work}/elsewhere"
ln -s "${work}/elsewhere" "${work}/symlink/$macos"
expect "a binary that is a symlink" 1 "${macos} is not a regular file" "${work}/symlink" "$@"

# shellcheck disable=SC2046 # the four hashes are separate arguments.
expect "a directory that does not exist" 1 "is not a directory" "${work}/absent" $(hashes "${work}/exact")

status=0
output="$(bash "$script" "$version" "${work}/exact" 2>&1)" || status=$?
if [[ $status -ne 2 || "$output" != *"Usage:"* ]]; then
  echo "FAIL too few arguments: exit ${status}: ${output}" >&2
  failures=$((failures + 1))
else
  echo "ok   too few arguments"
fi

make_set "${work}/bad-version"
# shellcheck disable=SC2046 # the four hashes are separate arguments.
set -- $(hashes "${work}/bad-version")
status=0
output="$(bash "$script" "1.2" "${work}/bad-version" "$@" 2>&1)" || status=$?
if [[ $status -ne 1 || "$output" != *"is not X.Y.Z"* ]]; then
  echo "FAIL a version that is not X.Y.Z: exit ${status}: ${output}" >&2
  failures=$((failures + 1))
else
  echo "ok   a version that is not X.Y.Z"
fi

if [[ $failures -ne 0 ]]; then
  echo "${failures} assemble case(s) failed." >&2
  exit 1
fi
echo "All assemble cases passed."
