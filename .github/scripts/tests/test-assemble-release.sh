#!/usr/bin/env bash
# runs assemble-release.sh against temporary directories: in unsigned mode the
# two SHA256SUMS lines, and in signed mode the three with latest.json, in their
# fixed order that sha256sum accepts, and a refusal for each way the directory,
# the mode or a hash can be wrong. each case checks the exit status and the cause
# the script names, and a refusal also checks that no SHA256SUMS was left.
set -euo pipefail

script="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/assemble-release.sh"
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT

version=1.2.3
windows="marshal-${version}-windows-x64.exe"
windows_msi="marshal-${version}-windows-x64.msi"

hash_of() {
  local line
  line="$(sha256sum "$1")"
  echo "${line%% *}"
}

# make_set <directory>: the two files, each with its own content.
make_set() {
  mkdir -p "$1"
  printf 'windows binary\n' >"$1/$windows"
  printf 'windows installer\n' >"$1/$windows_msi"
}

failures=0

# expect <case> <exit status> <text the output must contain> <directory> <hash arguments...>
# runs the script on the version above, and for a refusal checks that the directory
# holds no SHA256SUMS afterwards.
expect() {
  local name="$1" want_status="$2" want_text="$3" directory="$4" output status=0
  shift 4
  output="$(bash "$script" unsigned "$version" "$directory" "$@" 2>&1)" || status=$?
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
  echo "$(hash_of "$1/$windows")" "$(hash_of "$1/$windows_msi")"
}

make_set "${work}/exact"
# shellcheck disable=SC2046 # the two hashes are separate arguments.
expect "the exact files" 0 "$windows_msi" "${work}/exact" $(hashes "${work}/exact")
want="$(
  printf '%s  %s\n' \
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
  echo "ok   the two lines are in the fixed order and sha256sum accepts them"
fi

# a dry run's version carries a prerelease.
make_set "${work}/dry"
mv "${work}/dry/$windows" "${work}/dry/marshal-0.0.0-dryrun.7-windows-x64.exe"
mv "${work}/dry/$windows_msi" "${work}/dry/marshal-0.0.0-dryrun.7-windows-x64.msi"
dry_hashes="$(cd "${work}/dry" && sha256sum marshal-0.0.0-dryrun.7-windows-x64.exe marshal-0.0.0-dryrun.7-windows-x64.msi | awk '{ printf "%s ", $1 }')"
dry_status=0
# shellcheck disable=SC2086 # the two hashes are separate arguments.
dry_output="$(bash "$script" unsigned 0.0.0-dryrun.7 "${work}/dry" $dry_hashes 2>&1)" || dry_status=$?
if [[ $dry_status -ne 0 || "$(wc -l <"${work}/dry/SHA256SUMS")" -ne 2 ]]; then
  echo "FAIL a dry run's prerelease version: exit ${dry_status}: ${dry_output}" >&2
  failures=$((failures + 1))
else
  echo "ok   a dry run's prerelease version"
fi

make_set "${work}/changed-windows"
# shellcheck disable=SC2046 # the two hashes are separate arguments.
set -- $(hashes "${work}/changed-windows")
printf 'changed\n' >>"${work}/changed-windows/$windows"
expect "a changed app executable" 1 "${windows} has SHA-256" "${work}/changed-windows" "$@"

make_set "${work}/changed-msi"
# shellcheck disable=SC2046 # the two hashes are separate arguments.
set -- $(hashes "${work}/changed-msi")
printf 'changed\n' >>"${work}/changed-msi/$windows_msi"
expect "a changed Windows installer" 1 "${windows_msi} has SHA-256" "${work}/changed-msi" "$@"

make_set "${work}/swapped"
# shellcheck disable=SC2046 # the two hashes are separate arguments.
set -- $(hashes "${work}/swapped")
expect "two hashes handed over in the wrong order" 1 "has SHA-256" "${work}/swapped" "$2" "$1"

make_set "${work}/bad-hash"
# shellcheck disable=SC2046 # the two hashes are separate arguments.
set -- $(hashes "${work}/bad-hash")
expect "a hash that is not a SHA-256" 1 "is not a SHA-256" "${work}/bad-hash" "$1" "not-a-hash"
expect "an empty hash" 1 "is not a SHA-256" "${work}/bad-hash" "" "$2"

# a sha256sum that rejects the file the script just wrote.
mkdir -p "${work}/rejecting"
real_sha256sum="$(command -v sha256sum)"
cat >"${work}/rejecting/sha256sum" <<REJECTING
#!/usr/bin/env bash
if [[ "\${1:-}" == --check ]]; then
  echo "sha256sum: SHA256SUMS: no properly formatted checksum lines found" >&2
  exit 1
fi
exec "${real_sha256sum}" "\$@"
REJECTING
chmod +x "${work}/rejecting/sha256sum"
make_set "${work}/unchecked"
# shellcheck disable=SC2046 # the two hashes are separate arguments.
set -- $(hashes "${work}/unchecked")
PATH="${work}/rejecting:${PATH}" expect "a SHA256SUMS that sha256sum --check rejects" 1 "does not match the files it was just written from" "${work}/unchecked" "$@"

make_set "${work}/missing"
# shellcheck disable=SC2046 # the two hashes are separate arguments.
set -- $(hashes "${work}/missing")
rm "${work}/missing/$windows"
expect "a missing executable" 1 "are not exactly ${windows} ${windows_msi}:" "${work}/missing" "$@"

make_set "${work}/extra"
# shellcheck disable=SC2046 # the two hashes are separate arguments.
set -- $(hashes "${work}/extra")
printf 'helper\n' >"${work}/extra/fake-docker"
expect "an extra file" 1 "are not exactly" "${work}/extra" "$@"

make_set "${work}/other-version"
# shellcheck disable=SC2046 # the two hashes are separate arguments.
set -- $(hashes "${work}/other-version")
mv "${work}/other-version/$windows_msi" "${work}/other-version/marshal-1.2.4-windows-x64.msi"
expect "an installer named for another version" 1 "are not exactly" "${work}/other-version" "$@"

make_set "${work}/old-sums"
# shellcheck disable=SC2046 # the two hashes are separate arguments.
set -- $(hashes "${work}/old-sums")
(cd "${work}/old-sums" && sha256sum "$windows" >SHA256SUMS)
status=0
output="$(bash "$script" unsigned "$version" "${work}/old-sums" "$@" 2>&1)" || status=$?
if [[ $status -ne 1 || "$output" != *"are not exactly"* ]]; then
  echo "FAIL an earlier SHA256SUMS in the directory: exit ${status}: ${output}" >&2
  failures=$((failures + 1))
else
  echo "ok   an earlier SHA256SUMS in the directory"
fi

make_set "${work}/directory"
# shellcheck disable=SC2046 # the two hashes are separate arguments.
set -- $(hashes "${work}/directory")
rm "${work}/directory/$windows_msi"
mkdir "${work}/directory/$windows_msi"
expect "an installer that is a directory" 1 "${windows_msi} is not a regular file" "${work}/directory" "$@"

make_set "${work}/symlink"
# shellcheck disable=SC2046 # the two hashes are separate arguments.
set -- $(hashes "${work}/symlink")
mv "${work}/symlink/$windows" "${work}/elsewhere"
ln -s "${work}/elsewhere" "${work}/symlink/$windows"
expect "an executable that is a symlink" 1 "${windows} is not a regular file" "${work}/symlink" "$@"

# shellcheck disable=SC2046 # the two hashes are separate arguments.
expect "a directory that does not exist" 1 "is not a directory" "${work}/absent" $(hashes "${work}/exact")

status=0
output="$(bash "$script" unsigned "$version" "${work}/exact" 2>&1)" || status=$?
if [[ $status -ne 2 || "$output" != *"Usage:"* ]]; then
  echo "FAIL too few arguments: exit ${status}: ${output}" >&2
  failures=$((failures + 1))
else
  echo "ok   too few arguments"
fi

make_set "${work}/bad-version"
# shellcheck disable=SC2046 # the two hashes are separate arguments.
set -- $(hashes "${work}/bad-version")
status=0
output="$(bash "$script" unsigned "1.2" "${work}/bad-version" "$@" 2>&1)" || status=$?
if [[ $status -ne 1 || "$output" != *"is not X.Y.Z"* ]]; then
  echo "FAIL a version that is not X.Y.Z: exit ${status}: ${output}" >&2
  failures=$((failures + 1))
else
  echo "ok   a version that is not X.Y.Z"
fi

# signed mode: the update feed joins the set, as the third line.
# signed <case> <exit status> <text> <directory> <hash arguments...>
signed() {
  local name="$1" want_status="$2" want_text="$3" directory="$4" output status=0
  shift 4
  output="$(bash "$script" signed "$version" "$directory" "$@" 2>&1)" || status=$?
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
make_signed_set() {
  make_set "$1"
  printf '{"version":"%s"}\n' "$version" >"$1/latest.json"
}

make_signed_set "${work}/signed"
# shellcheck disable=SC2046 # the hashes are separate arguments.
signed "the exact signed files" 0 "latest.json" "${work}/signed" $(hashes "${work}/signed") "$(hash_of "${work}/signed/latest.json")"
want="$(
  printf '%s  %s\n' \
    "$(hash_of "${work}/signed/$windows")" "$windows" \
    "$(hash_of "${work}/signed/$windows_msi")" "$windows_msi" \
    "$(hash_of "${work}/signed/latest.json")" latest.json
)"
if [[ "$(cat "${work}/signed/SHA256SUMS")" != "$want" ]]; then
  echo "FAIL the signed SHA256SUMS lines or their order are wrong: $(cat "${work}/signed/SHA256SUMS")" >&2
  failures=$((failures + 1))
else
  echo "ok   the three signed lines are in the fixed order"
fi

make_signed_set "${work}/signed-changed"
# shellcheck disable=SC2046 # the hashes are separate arguments.
set -- $(hashes "${work}/signed-changed") "$(hash_of "${work}/signed-changed/latest.json")"
printf 'changed\n' >>"${work}/signed-changed/latest.json"
signed "a changed feed" 1 "latest.json has SHA-256" "${work}/signed-changed" "$@"

make_set "${work}/signed-missing"
# shellcheck disable=SC2046 # the hashes are separate arguments.
set -- $(hashes "${work}/signed-missing") "$(hash_of "${work}/exact/$windows")"
signed "a signed set without the feed" 1 "are not exactly" "${work}/signed-missing" "$@"

make_signed_set "${work}/unsigned-feed"
# shellcheck disable=SC2046 # the two hashes are separate arguments.
expect "an unsigned set with a feed" 1 "are not exactly" "${work}/unsigned-feed" $(hashes "${work}/unsigned-feed")

make_signed_set "${work}/signed-short"
# shellcheck disable=SC2046 # the two hashes are separate arguments.
signed "signed mode without the feed's hash" 2 "Usage:" "${work}/signed-short" $(hashes "${work}/signed-short")

status=0
# shellcheck disable=SC2046 # the two hashes are separate arguments.
output="$(bash "$script" "$version" "${work}/exact" $(hashes "${work}/exact") 2>&1)" || status=$?
if [[ $status -ne 2 || "$output" != *"Usage:"* ]]; then
  echo "FAIL no mode: exit ${status}: ${output}" >&2
  failures=$((failures + 1))
else
  echo "ok   no mode"
fi

if [[ $failures -ne 0 ]]; then
  echo "${failures} assemble case(s) failed." >&2
  exit 1
fi
echo "All assemble cases passed."
