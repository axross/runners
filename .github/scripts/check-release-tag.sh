#!/usr/bin/env bash
# Refuses a release tag unless it is `v` followed by the version in cli/Cargo.toml at
# the tagged commit, and that commit is on the default branch. Both checks always
# run, and each failure is named. release.yaml runs this from the tagged commit's own
# checkout, so a crafted tag could skip it: it catches mistakes, and the tag ruleset,
# the Environment approval and the install-side checks are the controls; see
# docs/conventions/security.md.
#
# Usage: check-release-tag.sh <tag> <commit> <default-branch-ref>
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "Usage: check-release-tag.sh <tag> <commit> <default-branch-ref>" >&2
  exit 2
fi
tag="$1"
default_ref="$3"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if ! commit="$(git rev-parse --verify --quiet "${2}^{commit}")"; then
  echo "error: ${2} is not a commit in this repository" >&2
  exit 1
fi
if ! git rev-parse --verify --quiet "${default_ref}^{commit}" >/dev/null; then
  echo "error: the default branch ref ${default_ref} is not in this repository; fetch it first" >&2
  exit 1
fi

failed=0

if ! manifest="$(git show "${commit}:cli/Cargo.toml" 2>/dev/null)"; then
  echo "error: the tagged commit ${commit} has no cli/Cargo.toml" >&2
  failed=1
elif ! version="$(printf '%s\n' "$manifest" | bash "${script_dir}/package-version.sh")"; then
  echo "error: cli/Cargo.toml at ${commit} names no package version" >&2
  failed=1
elif [[ "$tag" != "v${version}" ]]; then
  echo "error: the tag ${tag} does not match cli/Cargo.toml at ${commit}, whose version is ${version}; the tag must be v${version}" >&2
  failed=1
fi

if git merge-base --is-ancestor "$commit" "$default_ref"; then
  :
else
  status=$?
  if [[ $status -eq 1 ]]; then
    echo "error: the tagged commit ${commit} is not on the default branch ${default_ref}" >&2
  else
    echo "error: git merge-base could not compare ${commit} with ${default_ref} (exit ${status})" >&2
  fi
  failed=1
fi

if [[ $failed -ne 0 ]]; then
  exit 1
fi
echo "The tag ${tag} matches cli/Cargo.toml's version, and ${commit} is on ${default_ref}."
