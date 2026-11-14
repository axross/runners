#!/usr/bin/env bash
# the feed round trip that merge-checks.yaml's update-feed job runs on the
# release dry run: the release run's own sign-update scripts sign the dry run's
# MSI with a throwaway key made for this run, write-update-feed.sh writes the
# feed, and the feed check passes it against the throwaway public key. the feed
# check must refuse a changed MSI, another key's signature, a signature for
# another version, and the throwaway signature against the committed key. no
# line of this run's output may hold the throwaway key or its password.
#
#   feed-round-trip.sh <update tools> <dry run release files> <version> <output>
#
# <output> receives what the update-feed-app job hands to the app's updater
# test: the MSI, update.pub, latest.json, latest-another-key.json and
# latest-another-version.json.
set -euo pipefail

if [[ $# -ne 4 ]]; then
  echo "Usage: feed-round-trip.sh <update tools> <dry run release files> <version> <output>" >&2
  exit 2
fi
tools="$1"
release="$2"
version="$3"
output="$4"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
scripts="$(cd "${here}/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT
log="${work}/output.log"

refuse() {
  echo "error: $*" >&2
  exit 1
}

signer="${tools}/cargo-tauri"
check="${tools}/marshal-feed-check"
chmod 0755 "$signer" "$check"
msi="marshal-${version}-windows-x64.msi"
[[ -f "${release}/${msi}" && -f "${release}/SHA256SUMS" ]] \
  || refuse "${release} holds no ${msi} and SHA256SUMS"

# two throwaway keys, under a password made for this run.
password="$(head -c 24 /dev/urandom | base64 | tr -d '/+=')"
"$signer" signer generate --ci --password "$password" --write-keys "${work}/update.key" >/dev/null
"$signer" signer generate --ci --password "$password" --write-keys "${work}/other.key" >/dev/null
key="$(<"${work}/update.key")"

line="$(sha256sum "$signer")"
signature="$(SIGN_VERSION="$version" SIGN_TAURI_CLI_SHA256="${line%% *}" \
  SIGN_KEY="$key" SIGN_PASSWORD="$password" \
  bash "${here}/run-sign-update.sh" "${work}/sign" "${release}/${msi}" "${release}/SHA256SUMS" "$signer" 2>&1 \
  | tee -a "$log" | tail -n 1)"
[[ -f "$signature" ]] || refuse "sign-update's scripts left no signature"

mkdir -p "$output"
cp -- "${release}/${msi}" "${output}/${msi}"
cp -- "${work}/update.key.pub" "${output}/update.pub"
bash "${scripts}/write-update-feed.sh" "$version" "${output}/latest.json" \
  "windows-x86_64=${output}/${msi}=${signature}" 2>&1 | tee -a "$log"
"$check" --public-key "${output}/update.pub" "${output}/latest.json" "$version" \
  "${release}/SHA256SUMS" "windows-x86_64=${output}/${msi}" 2>&1 | tee -a "$log"
echo "ok   the feed check passes the feed the release run's scripts signed" | tee -a "$log"

failures=0
# refused <case> <feed check arguments...>: the feed check must fail.
refused() {
  local name="$1" status=0 result
  shift
  result="$("$check" "$@" 2>&1)" || status=$?
  echo "$result" >>"$log"
  if [[ $status -eq 0 ]]; then
    echo "FAIL the feed check passed ${name}" | tee -a "$log" >&2
    failures=$((failures + 1))
  else
    echo "ok   the feed check refuses ${name}" | tee -a "$log"
  fi
}

mkdir "${work}/changed"
cp -- "${release}/${msi}" "${work}/changed/${msi}"
printf 'changed' >>"${work}/changed/${msi}"
refused "a changed MSI" --public-key "${output}/update.pub" "${output}/latest.json" "$version" \
  "${release}/SHA256SUMS" "windows-x86_64=${work}/changed/${msi}"

# feed_with <signature file> <output name>: the feed with another signature.
feed_with() {
  bash "${scripts}/write-update-feed.sh" "$version" "${output}/$2" \
    "windows-x86_64=${output}/${msi}=$1" >>"$log" 2>&1
}
mkdir "${work}/other" "${work}/version"
cp -- "${release}/${msi}" "${work}/other/${msi}"
cp -- "${release}/${msi}" "${work}/version/${msi}"
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$password" "$signer" signer sign \
  --private-key-path "${work}/other.key" --app-version "$version" "${work}/other/${msi}" >>"$log" 2>&1
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$password" "$signer" signer sign \
  --private-key-path "${work}/update.key" --app-version 9.9.9 "${work}/version/${msi}" >>"$log" 2>&1
feed_with "${work}/other/${msi}.sig" latest-another-key.json
feed_with "${work}/version/${msi}.sig" latest-another-version.json
refused "another key's signature" --public-key "${output}/update.pub" "${output}/latest-another-key.json" \
  "$version" "${release}/SHA256SUMS" "windows-x86_64=${output}/${msi}"
refused "a signature for another version" --public-key "${output}/update.pub" \
  "${output}/latest-another-version.json" "$version" "${release}/SHA256SUMS" "windows-x86_64=${output}/${msi}"
refused "the throwaway signature against the committed key" "${output}/latest.json" "$version" \
  "${release}/SHA256SUMS" "windows-x86_64=${output}/${msi}"

if grep -qF -e "$key" -e "$password" "$log"; then
  echo "FAIL the throwaway key or its password appears in the output" >&2
  failures=$((failures + 1))
else
  echo "ok   the output holds neither the throwaway key nor its password"
fi

if [[ $failures -ne 0 ]]; then
  echo "${failures} feed round trip case(s) failed." >&2
  exit 1
fi
echo "The feed round trip passed."
