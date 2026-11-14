#!/usr/bin/env bash
# writes latest.json, the update feed the app reads, to <output>: <version>, the
# time it is written, and one platform entry per <platform>=<file>=<signature>
# argument, each with the address of <file>'s name under the release's tag and
# the text of <signature>, the detached signature `cargo tauri signer sign`
# wrote. the entries are a list, so a release that signs another platform's
# file adds an argument. release.yaml's assemble-signed job runs it, and
# merge-checks.yaml's update-feed job runs it on the dry run; the feed check,
# marshal-feed-check, then checks what it wrote. needs jq, which ubuntu-latest
# has. see docs/operations/app.md.
set -euo pipefail

if [[ $# -lt 3 ]]; then
  echo "Usage: write-update-feed.sh <version> <output> <platform>=<file>=<signature>..." >&2
  exit 2
fi
version="$1"
output="$2"
shift 2

refuse() {
  echo "error: $*" >&2
  exit 1
}

[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] \
  || refuse "the version '${version}' is not X.Y.Z or X.Y.Z-<prerelease>"

platforms='{}'
for entry in "$@"; do
  IFS='=' read -r platform file signature rest <<<"$entry"
  [[ -n "$platform" && -n "$file" && -n "$signature" && -z "$rest" ]] \
    || refuse "'${entry}' is not <platform>=<file>=<signature>"
  [[ "$platform" =~ ^[a-z0-9]+-[a-z0-9_]+$ ]] || refuse "'${platform}' is not a platform name"
  [[ -f "$file" ]] || refuse "${file} is not a file"
  [[ -f "$signature" ]] || refuse "${signature} is not a file"
  if jq --exit-status --arg platform "$platform" 'has($platform)' <<<"$platforms" >/dev/null; then
    refuse "${platform} is given twice"
  fi
  text="$(<"$signature")"
  [[ "$text" =~ ^[A-Za-z0-9+/]+=*$ ]] || refuse "${signature} is not one line of base64"
  platforms="$(jq --compact-output \
    --arg platform "$platform" \
    --arg url "https://github.com/axross/runners/releases/download/v${version}/${file##*/}" \
    --arg signature "$text" \
    '. + {($platform): {url: $url, signature: $signature}}' <<<"$platforms")"
done

jq --null-input \
  --arg version "$version" \
  --arg date "$(date --utc +%Y-%m-%dT%H:%M:%SZ)" \
  --argjson platforms "$platforms" \
  '{version: $version, pub_date: $date, platforms: $platforms}' >"$output"
cat "$output"
