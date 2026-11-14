#!/usr/bin/env bash
# builds the two programs the release run's update signing runs, into
# <directory>: `cargo-tauri`, from the `tauri-cli` crate archive whose SHA-256
# app/tauri-cli.crate.sha256 commits, and `marshal-feed-check`, from the app's
# workspace. release.yaml's update-tool job and merge-checks.yaml's update-feed
# job run it, on ubuntu-latest, with the repository's pinned toolchain and no
# cache. it refuses an archive whose SHA-256 differs from the committed one,
# builds with --locked, so Cargo checks every dependency against the archive's
# own lockfile, and stops on any advisory cargo-deny reports for that lockfile
# that app/tauri-cli.deny.toml does not list as accepted.
# --archive takes the crate archive from a file instead of crates.io, for the
# script's test. see docs/operations/app.md and docs/conventions/security.md.
set -euo pipefail

usage() {
  echo "Usage: build-update-tools.sh <directory> [--archive <file>]" >&2
  exit 2
}
[[ $# -eq 1 || ($# -eq 3 && "$2" == "--archive") ]] || usage
output="$1"
archive="${3-}"

refuse() {
  echo "error: $*" >&2
  exit 1
}

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository="$(cd "${here}/../.." && pwd)"
read -r version sha256 < <(bash "${here}/tauri-cli-crate.sh")
[[ -n "$version" && -n "$sha256" ]] || refuse "the committed tauri-cli checksum could not be read"

work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT
crate="tauri-cli-${version}.crate"

if [[ -z "$archive" ]]; then
  curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 \
    --output "${work}/${crate}" "https://static.crates.io/crates/tauri-cli/${crate}"
else
  cp -- "$archive" "${work}/${crate}"
fi
line="$(sha256sum "${work}/${crate}")"
actual="${line%% *}"
if [[ "$actual" != "$sha256" ]]; then
  refuse "${crate} has SHA-256 ${actual}, not the ${sha256} app/tauri-cli.crate.sha256 commits"
fi
echo "${crate} has the committed SHA-256 ${sha256}."

mkdir "${work}/source"
tar --extract --gzip --file "${work}/${crate}" --directory "${work}/source" --no-same-owner
source="${work}/source/tauri-cli-${version}"
[[ -f "${source}/Cargo.toml" && -f "${source}/Cargo.lock" ]] \
  || refuse "${crate} holds no tauri-cli-${version}/Cargo.toml and Cargo.lock"

# rustup picks the toolchain from the directory a command runs in, so both builds
# run from app/, whose rust-toolchain.toml names the repository's channel.
cd "${repository}/app"
cargo deny --manifest-path "${source}/Cargo.toml" --locked \
  --config "${repository}/app/tauri-cli.deny.toml" check advisories
cargo build --manifest-path "${source}/Cargo.toml" --locked --release \
  --bin cargo-tauri --target-dir "${work}/target"
cargo build --locked --release --package marshal-feed --bin marshal-feed-check \
  --target-dir "${work}/feed-target"

mkdir -p "$output"
cp -- "${work}/target/release/cargo-tauri" "${output}/cargo-tauri"
cp -- "${work}/feed-target/release/marshal-feed-check" "${output}/marshal-feed-check"
(cd "$output" && sha256sum cargo-tauri marshal-feed-check)
