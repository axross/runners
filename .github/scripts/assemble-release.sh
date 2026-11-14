#!/usr/bin/env bash
# writes SHA256SUMS for a set of release files and is the only thing that does.
# in `unsigned` mode, which release-build.yaml's assemble job runs, the set is
# the app executable and the MSI named for <version>; in `signed` mode, which
# release.yaml's assemble-signed job runs, it is those two and latest.json, the
# update feed. <directory> holds exactly the set and no SHA256SUMS. each file's
# SHA-256 must equal the hash handed over for it, so a file that changed since
# its hash was taken is refused. the lines come out in a fixed order, and the
# result must pass `sha256sum --check --strict`.
# runs on Linux, where sha256sum is GNU's, as verify-release.sh does.
set -euo pipefail

usage() {
  echo "Usage: assemble-release.sh unsigned <version> <directory> <windows exe sha256> <windows msi sha256>" >&2
  echo "       assemble-release.sh signed <version> <directory> <windows exe sha256> <windows msi sha256> <latest.json sha256>" >&2
  exit 2
}
case "${1:-}" in
  unsigned) [[ $# -eq 5 ]] || usage ;;
  signed) [[ $# -eq 6 ]] || usage ;;
  *) usage ;;
esac
mode="$1"
version="$2"
directory="$3"
shift 3
hashes=("$@")

refuse() {
  echo "error: $*" >&2
  exit 1
}

[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] \
  || refuse "the version '${version}' is not X.Y.Z or X.Y.Z-<prerelease>"
[[ -d "$directory" ]] || refuse "${directory} is not a directory"

files=("marshal-${version}-windows-x64.exe" "marshal-${version}-windows-x64.msi")
if [[ "$mode" == signed ]]; then
  files+=(latest.json)
fi

expected="$(printf '%s\n' "${files[@]}" | LC_ALL=C sort)"
present="$(find "$directory" -mindepth 1 -maxdepth 1 -printf '%P\n' | LC_ALL=C sort)"
if [[ "$present" != "$expected" ]]; then
  refuse "the files are not exactly ${files[*]}:
${present}"
fi
for file in "${files[@]}"; do
  if [[ -L "${directory}/${file}" || ! -f "${directory}/${file}" ]]; then
    refuse "${file} is not a regular file"
  fi
done

for hash in "${hashes[@]}"; do
  [[ "$hash" =~ ^[0-9a-f]{64}$ ]] || refuse "'${hash}' is not a SHA-256"
done

lines=()
for index in "${!files[@]}"; do
  file="${files[$index]}"
  line="$(sha256sum "${directory}/${file}")"
  actual="${line%% *}"
  [[ "$actual" == "${hashes[$index]}" ]] \
    || refuse "${file} has SHA-256 ${actual}, not the ${hashes[$index]} it was handed"
  lines+=("${hashes[$index]}" "$file")
done

printf '%s  %s\n' "${lines[@]}" >"${directory}/SHA256SUMS"
if ! (cd "$directory" && sha256sum --check --strict SHA256SUMS); then
  rm -f "${directory}/SHA256SUMS"
  refuse "SHA256SUMS does not match the files it was just written from"
fi
cat "${directory}/SHA256SUMS"
