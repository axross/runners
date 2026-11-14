#!/usr/bin/env bash
# runs the scripts of the release run's update signing that need no Rust build:
# tauri-cli-crate.sh, which must print the committed version and SHA-256 and
# refuse a checksum file whose version is not the pin or whose line is not one
# `<sha256>  tauri-cli-<X.Y.Z>.crate`; build-update-tools.sh, which must refuse
# a crate archive whose SHA-256 is not the committed one before it builds
# anything; and write-update-feed.sh, which must write the feed's version and
# one entry per signed file at its release address, and refuse what is not a
# file, a platform or a signature. each case checks the exit status and the
# cause the script names.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
scripts="$(cd "${here}/.." && pwd)"
repository="$(cd "${scripts}/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT

failures=0
# expect <case> <exit status> <text the output must contain> <command...>
expect() {
  local name="$1" want_status="$2" want_text="$3" output status=0
  shift 3
  output="$("$@" 2>&1)" || status=$?
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

pin="$(bash "${scripts}/tauri-cli-version.sh")"
hash="$(printf 'a%.0s' {1..64})"

echo "-- tauri-cli-crate.sh --"
read -r committed_version committed_sha256 < <(bash "${scripts}/tauri-cli-crate.sh")
if [[ "$committed_version" != "$pin" || ! "$committed_sha256" =~ ^[0-9a-f]{64}$ ]]; then
  echo "FAIL the committed checksum: '${committed_version} ${committed_sha256}' for the pin ${pin}" >&2
  failures=$((failures + 1))
else
  echo "ok   the committed checksum names the pin, ${pin}"
fi
printf '%s  tauri-cli-%s.crate\n' "$hash" "$pin" >"${work}/same.sha256"
expect "a checksum at the pin" 0 "${pin} ${hash}" bash "${scripts}/tauri-cli-crate.sh" "${work}/same.sha256"
printf '%s  tauri-cli-9.9.9.crate\n' "$hash" >"${work}/other.sha256"
expect "a checksum for another version than the pin" 1 "move the checksum with the pin" \
  bash "${scripts}/tauri-cli-crate.sh" "${work}/other.sha256"
printf '9.9.9\n' >"${work}/moved.version"
expect "a pin moved without the checksum" 1 "move the checksum with the pin" \
  bash "${scripts}/tauri-cli-crate.sh" "${work}/same.sha256" "${work}/moved.version"
printf '%s tauri-cli-%s.crate\n' "$hash" "$pin" >"${work}/one-space.sha256"
expect "a line with one space" 1 "must hold one" bash "${scripts}/tauri-cli-crate.sh" "${work}/one-space.sha256"
printf '%s  tauri-cli-%s.crate\n%s  tauri-cli-%s.crate\n' "$hash" "$pin" "$hash" "$pin" >"${work}/two.sha256"
expect "two lines" 1 "must hold one" bash "${scripts}/tauri-cli-crate.sh" "${work}/two.sha256"
printf '%s  tauri-cli-%s.crate\n' "${hash:1}" "$pin" >"${work}/short.sha256"
expect "a hash that is not a SHA-256" 1 "must hold one" bash "${scripts}/tauri-cli-crate.sh" "${work}/short.sha256"
expect "a checksum file that does not exist" 1 "does not exist" bash "${scripts}/tauri-cli-crate.sh" "${work}/absent"

echo "-- build-update-tools.sh --"
printf 'not the crate\n' >"${work}/forged.crate"
expect "a crate archive with another SHA-256" 1 "not the ${committed_sha256} app/tauri-cli.crate.sha256 commits" \
  bash "${scripts}/build-update-tools.sh" "${work}/tools" --archive "${work}/forged.crate"
if [[ -e "${work}/tools" ]]; then
  echo "FAIL the refused archive left tools behind" >&2
  failures=$((failures + 1))
fi
expect "build-update-tools.sh with an unknown option" 2 "Usage:" \
  bash "${scripts}/build-update-tools.sh" "${work}/tools" --other "${work}/forged.crate"

echo "-- write-update-feed.sh --"
version=1.2.3
msi="${work}/marshal-${version}-windows-x64.msi"
printf 'msi\n' >"$msi"
printf 'c2lnbmF0dXJl\n' >"${work}/msi.sig"
expect "the feed for one signed file" 0 '"version": "1.2.3"' \
  bash "${scripts}/write-update-feed.sh" "$version" "${work}/latest.json" "windows-x86_64=${msi}=${work}/msi.sig"
want="https://github.com/axross/runners/releases/download/v1.2.3/marshal-1.2.3-windows-x64.msi c2lnbmF0dXJl"
got="$(jq -r '.platforms["windows-x86_64"] | "\(.url) \(.signature)"' "${work}/latest.json")"
if [[ "$got" != "$want" || "$(jq -r '.platforms | keys | join(" ")' "${work}/latest.json")" != windows-x86_64 ]]; then
  echo "FAIL the feed's entry: ${got}" >&2
  failures=$((failures + 1))
elif [[ ! "$(jq -r .pub_date "${work}/latest.json")" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$ ]]; then
  echo "FAIL the feed's pub_date: $(jq -r .pub_date "${work}/latest.json")" >&2
  failures=$((failures + 1))
else
  echo "ok   the entry is the file's release address and the signature's text"
fi
printf 'archive\n' >"${work}/marshal-${version}-macos-arm64.tar.gz"
expect "the feed for two signed files" 0 '"darwin-aarch64"' \
  bash "${scripts}/write-update-feed.sh" "$version" "${work}/two.json" \
  "windows-x86_64=${msi}=${work}/msi.sig" "darwin-aarch64=${work}/marshal-${version}-macos-arm64.tar.gz=${work}/msi.sig"
expect "a platform given twice" 1 "is given twice" \
  bash "${scripts}/write-update-feed.sh" "$version" "${work}/twice.json" \
  "windows-x86_64=${msi}=${work}/msi.sig" "windows-x86_64=${msi}=${work}/msi.sig"
expect "a file that does not exist" 1 "is not a file" \
  bash "${scripts}/write-update-feed.sh" "$version" "${work}/x.json" "windows-x86_64=${work}/absent=${work}/msi.sig"
printf 'two\nlines\n' >"${work}/bad.sig"
expect "a signature that is not one line of base64" 1 "is not one line of base64" \
  bash "${scripts}/write-update-feed.sh" "$version" "${work}/x.json" "windows-x86_64=${msi}=${work}/bad.sig"
expect "an entry without a signature" 1 "is not <platform>=<file>=<signature>" \
  bash "${scripts}/write-update-feed.sh" "$version" "${work}/x.json" "windows-x86_64=${msi}"
expect "a platform name with a slash" 1 "is not a platform name" \
  bash "${scripts}/write-update-feed.sh" "$version" "${work}/x.json" "windows/x86_64=${msi}=${work}/msi.sig"
expect "a version that is not X.Y.Z" 1 "is not X.Y.Z" \
  bash "${scripts}/write-update-feed.sh" 1.2 "${work}/x.json" "windows-x86_64=${msi}=${work}/msi.sig"

# the archive's lockfile check reads its list of accepted advisories from here.
[[ -f "${repository}/app/tauri-cli.deny.toml" ]] || {
  echo "FAIL app/tauri-cli.deny.toml is missing" >&2
  failures=$((failures + 1))
}

if [[ $failures -ne 0 ]]; then
  echo "${failures} update tool case(s) failed." >&2
  exit 1
fi
echo "All update tool cases passed."
