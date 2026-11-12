#!/usr/bin/env bash
# prints the version an installer carries for a release version. a Windows
# Installer ProductVersion is major.minor.build with major and minor at most 255
# and build at most 65535, so a release X.Y.Z beyond those limits is refused, the
# dry-run form 0.0.0-dryrun.<n> maps to 0.0.0, and any other prerelease form is
# refused. both installer build legs call it, so the pkg carries the same rule.
# see docs/operations/cli.md.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: installer-version.sh <version>" >&2
  exit 2
fi
version="$1"

refuse() {
  echo "error: $*" >&2
  exit 1
}

number='(0|[1-9][0-9]*)'
if [[ "$version" =~ ^${number}\.${number}\.${number}$ ]]; then
  major="${BASH_REMATCH[1]}"
  minor="${BASH_REMATCH[2]}"
  patch="${BASH_REMATCH[3]}"
  # a digit string longer than the limit's is beyond it, and is never handed to
  # the arithmetic, which would overflow on a very long one.
  if [[ ${#major} -gt 3 || ${#minor} -gt 3 || ${#patch} -gt 5 ||
    major -gt 255 || minor -gt 255 || patch -gt 65535 ]]; then
    refuse "${version} is beyond the installer limits: major and minor at most 255, patch at most 65535"
  fi
  echo "$version"
elif [[ "$version" =~ ^0\.0\.0-dryrun\.${number}$ ]]; then
  echo "0.0.0"
else
  refuse "${version} is not X.Y.Z or 0.0.0-dryrun.<n>; an installer carries no other prerelease form"
fi
