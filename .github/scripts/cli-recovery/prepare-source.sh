#!/usr/bin/env bash
# validates the complete historical checkout before stamping, or revalidates its
# version-only changes after a build. build outputs must live outside the checkout.
set -euo pipefail

if [[ $# -ne 3 || ( "$1" != prepare && "$1" != verify ) ]]; then
  echo 'Usage: prepare-source.sh <prepare|verify> <historical checkout> <version>' >&2
  exit 2
fi
phase="$1"
checkout="$2"
version="$3"
source_commit=dc8e1ebcc0722997206487f3dcca2ae7e519cf02
source_tree=f18a47792fd14c8218224ff29b35d1303499dfd0
cli_tree=0def8a5118212ae2d81669b0d0992b2857351893
lock_blob=b4dd401878839a486110e3674b17e5f713917c72
toolchain_blob=58eef2ccd48f957feeada49b17ad1e3fd667bf8c

refuse() {
  echo "error: $*" >&2
  exit 1
}

command -v git >/dev/null || refuse 'git is required'
[[ -d "$checkout" ]] || refuse 'the historical checkout is missing'
checkout="$(cd "$checkout" && pwd -P)"
root="$(git -C "$checkout" rev-parse --show-toplevel)"
[[ "$(cd "$root" && pwd -P)" == "$checkout" ]] \
  || refuse 'the historical input is not a checkout root'
[[ "$(git -C "$checkout" rev-parse HEAD)" == "$source_commit" ]] \
  || refuse "historical HEAD is not ${source_commit}"
[[ "$(git -C "$checkout" rev-parse 'HEAD^{tree}')" == "$source_tree" ]] \
  || refuse 'the historical source tree identity differs'
[[ "$(git -C "$checkout" rev-parse HEAD:cli)" == "$cli_tree" ]] \
  || refuse 'the historical CLI tree identity differs'
[[ "$(git -C "$checkout" rev-parse HEAD:cli/Cargo.lock)" == "$lock_blob" ]] \
  || refuse 'the historical lock identity differs'
[[ "$(git -C "$checkout" rev-parse HEAD:cli/rust-toolchain.toml)" == "$toolchain_blob" ]] \
  || refuse 'the historical toolchain identity differs'
git -C "$checkout" diff --cached --quiet "$source_commit" \
  || refuse 'the historical index changed'
untracked="$(git -C "$checkout" ls-files --others)"
[[ -z "$untracked" ]] || refuse "the historical input has untracked files (including ignored files): ${untracked}"

if [[ "$phase" == prepare ]]; then
  git -C "$checkout" diff --quiet "$source_commit" \
    || refuse 'the historical input is not clean before version stamping'
fi
git -C "$checkout" diff --quiet "$source_commit" -- . \
  ':(exclude)cli/Cargo.toml' ':(exclude)cli/Cargo.lock' \
  || refuse 'the historical input has a non-version mutation'

for file in Cargo.toml Cargo.lock; do
  [[ -f "${checkout}/cli/${file}" && ! -L "${checkout}/cli/${file}" ]] \
    || refuse "cli/${file} is not a regular non-symlink file"
done
mode_changes="$(git -C "$checkout" diff --summary "$source_commit" -- cli/Cargo.toml cli/Cargo.lock)"
[[ -z "$mode_changes" ]] || refuse "stamped file modes changed: ${mode_changes}"

work="$(mktemp -d "${TMPDIR:-/tmp}/cli-recovery-source.XXXXXX")"
trap 'rm -rf -- "$work"' EXIT
git -C "$checkout" show "${source_commit}:cli/Cargo.toml" >"${work}/Cargo.toml"
git -C "$checkout" show "${source_commit}:cli/Cargo.lock" >"${work}/Cargo.lock"
bash "${checkout}/.github/scripts/set-release-version.sh" "$version" "$work"

if [[ "$phase" == prepare ]]; then
  bash "${checkout}/.github/scripts/set-release-version.sh" "$version" "${checkout}/cli"
fi
for file in Cargo.toml Cargo.lock; do
  expected="$(git hash-object --no-filters "${work}/${file}")"
  actual="$(git -C "$checkout" hash-object --path="cli/${file}" "cli/${file}")"
  [[ "$actual" == "$expected" ]] \
    || refuse "cli/${file} is not the declared version-only stamp for ${version}"
done
echo "Historical input: ${source_commit}; source tree: ${source_tree}; CLI tree: ${cli_tree}"
echo "Checked paths: cli/Cargo.lock (${lock_blob}), cli/rust-toolchain.toml (${toolchain_blob}); version: ${version}"
echo 'The default attestation identifies the workflow/OIDC commit, not every fetched historical input.'
