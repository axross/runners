#!/usr/bin/env bash
# exercises the recovery assembler, publication gate and actual inline workflow
# checks with text fixtures, not signatures or native installer evidence.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository="$(cd "${here}/../../.." && pwd)"
scripts="${repository}/.github/scripts/cli-recovery"
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT

node --input-type=module - "$repository" "$work" <<'NODE'
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
const [root, work] = process.argv.slice(2);
const { readWorkflow } = await import(pathToFileURL(join(root, ".github/scripts/workflow-reader.mjs")));
const workflow = readWorkflow(join(root, ".github/workflows/release.yaml"));
for (const job of ["assemble-signed", "attest"]) {
  const scripts = workflow.jobs[job].steps.filter((step) => typeof step.run === "string");
  if (scripts.length !== 1) throw new Error(`${job} must have one inline check`);
  writeFileSync(join(work, `${job}.sh`), scripts[0].run);
}
NODE

version=1.2.3
windows="marshal-${version}-windows-x64.exe"
msi="marshal-${version}-windows-x64.msi"
macos="marshal-${version}-macos-arm64"
pkg="${macos}.pkg"
names=("$macos" "$pkg" "$windows" "$msi")
mkdir "${work}/inputs"
for file in "${names[@]}"; do printf 'unsigned stand-in %s\n' "$file" >"${work}/inputs/${file}"; done
hashes=()
for file in "$windows" "$msi" "$macos" "$pkg"; do
  hash="$(sha256sum "${work}/inputs/${file}")"
  hashes+=("${hash%% *}")
done
bash "${scripts}/assemble-release.sh" "$version" "${work}/inputs" "${hashes[@]}"
RELEASE_PHASE=release bash "${scripts}/verify-release.sh" "$version" "${work}/inputs"
expected="$(printf '%s\n' "${names[@]}")"
[[ "$(awk '{ print $2 }' "${work}/inputs/SHA256SUMS")" == "$expected" ]]

mapfile -t assets < <(node -e '
  const fs = require("node:fs");
  const config = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
  const github = config.plugins.find((plugin) => plugin[0] === "@semantic-release/github");
  for (const asset of github[1].assets) console.log(typeof asset === "string" ? asset : asset.path);
' "${repository}/release/.releaserc.json")
[[ ${#assets[@]} -eq 5 ]]
matched=()
for asset in "${assets[@]}"; do
  [[ "$asset" == ../release-files/* ]]
  matches=()
  for file in SHA256SUMS "${names[@]}"; do
    # shellcheck disable=SC2053 # the release asset is a glob, not a literal filename.
    if [[ "$file" == ${asset#../release-files/} ]]; then matches+=("$file"); fi
  done
  [[ ${#matches[@]} -eq 1 ]]
  matched+=("${matches[0]}")
done
[[ "$(printf '%s\n' "${matched[@]}" | LC_ALL=C sort)" == "$(printf '%s\n' SHA256SUMS "${names[@]}" | LC_ALL=C sort)" ]]
echo 'ok   selected release asset globs match exactly the five CLI recovery files'

failures=0

# expect_failure <name> <diagnostic> <command...> distinguishes a refusal from a
# harness failure by requiring both its status and its named diagnostic.
expect_failure() {
  local name="$1" diagnostic="$2" output status=0
  shift 2
  output="$("$@" 2>&1)" || status=$?
  if [[ $status -ne 1 || "$output" != *"$diagnostic"* ]]; then
    echo "FAIL ${name}: exit ${status}: ${output}" >&2
    failures=$((failures + 1))
  else
    echo "ok   ${name}"
  fi
}

for file in "${names[@]}" SHA256SUMS; do
  case_dir="${work}/missing-${file}"
  cp -r "${work}/inputs" "$case_dir"
  rm "${case_dir}/${file}"
  expect_failure "missing ${file}" 'not exactly' env RELEASE_PHASE=release bash "${scripts}/verify-release.sh" "$version" "$case_dir"
done
for extra in marshal-app.exe marshal-helper marshal.dmg other.txt; do
  case_dir="${work}/extra-${extra}"
  cp -r "${work}/inputs" "$case_dir"
  printf 'extra stand-in\n' >"${case_dir}/${extra}"
  expect_failure "extra ${extra}" 'not exactly' env RELEASE_PHASE=release bash "${scripts}/verify-release.sh" "$version" "$case_dir"
done
for file in "${names[@]}"; do
  case_dir="${work}/changed-${file}"
  cp -r "${work}/inputs" "$case_dir"
  printf 'changed bytes\n' >>"${case_dir}/${file}"
  expect_failure "changed ${file}" 'does not match SHA256SUMS' env RELEASE_PHASE=release bash "${scripts}/verify-release.sh" "$version" "$case_dir"
done
for phase in '' unknown; do
  expect_failure "invalid phase '${phase}'" 'RELEASE_PHASE must be plan or release' env RELEASE_PHASE="$phase" bash "${scripts}/verify-release.sh" "$version" "${work}/inputs"
done
env RELEASE_PHASE=plan GITHUB_OUTPUT="${work}/plan-output" bash "${scripts}/verify-release.sh" "$version" "${work}/absent"
grep -qxF "version=${version}" "${work}/plan-output"

mkdir -p "${work}/assembly/.github/scripts/cli-recovery" "${work}/assembly/signed"
cp "${scripts}/assemble-release.sh" "${work}/assembly/.github/scripts/cli-recovery/"
cp -r "${work}/inputs" "${work}/assembly/unsigned"
for file in "$macos" "$pkg"; do printf 'signed stand-in %s\n' "$file" >"${work}/assembly/signed/${file}"; done
macos_hash="$(sha256sum "${work}/assembly/signed/${macos}")"
pkg_hash="$(sha256sum "${work}/assembly/signed/${pkg}")"
export VERSION="$version" MACOS_SHA256="${macos_hash%% *}" MACOS_PKG_SHA256="${pkg_hash%% *}"
(
  cd "${work}/assembly"
  bash -eo pipefail "${work}/assemble-signed.sh"
)
cmp "${work}/assembly/signed/${macos}" "${work}/assembly/dist/${macos}"
cmp "${work}/assembly/signed/${pkg}" "${work}/assembly/dist/${pkg}"
if cmp -s "${work}/assembly/unsigned/${macos}" "${work}/assembly/dist/${macos}"; then
  echo 'FAIL signed assembly used the unsigned binary' >&2
  exit 1
fi
RELEASE_PHASE=release bash "${scripts}/verify-release.sh" "$version" "${work}/assembly/dist"
(
  cd "${work}/assembly/dist"
  GITHUB_OUTPUT="${work}/attest-output" bash -eo pipefail "${work}/attest.sh"
)
for line in "windows=${windows}" "windows_msi=${msi}" "macos=${macos}" "macos_pkg=${pkg}"; do grep -qxF "$line" "${work}/attest-output"; done
[[ "$(wc -l <"${work}/attest-output")" -eq 4 ]]
echo 'ok   actual assembly and attestation check consume signed stand-in bytes'

for change in missing extra unsigned-bytes; do
  case_dir="${work}/attest-${change}"
  cp -r "${work}/assembly/dist" "$case_dir"
  diagnostic='not exactly'
  case "$change" in
    missing) rm "${case_dir}/${pkg}" ;;
    extra) printf 'extra\n' >"${case_dir}/marshal-app.exe" ;;
    unsigned-bytes) cp "${work}/assembly/unsigned/${macos}" "${case_dir}/${macos}"; diagnostic='FAILED' ;;
  esac
  (
    cd "$case_dir"
    expect_failure "inline attestation: ${change}" "$diagnostic" env GITHUB_OUTPUT="${work}/refused-output" bash -eo pipefail "${work}/attest.sh"
    [[ $failures -eq 0 ]]
  ) || failures=$((failures + 1))
done
for change in missing extra unsigned-hash; do
  case_dir="${work}/assembly-${change}"
  cp -r "${work}/assembly" "$case_dir"
  rm -rf "${case_dir}/dist"
  case "$change" in
    missing) rm "${case_dir}/signed/${pkg}" ;;
    extra) printf 'extra\n' >"${case_dir}/signed/marshal-app.exe" ;;
    unsigned-hash) cp "${case_dir}/unsigned/${macos}" "${case_dir}/signed/${macos}" ;;
  esac
  (
    cd "$case_dir"
    expect_failure "inline signed assembly: ${change}" 'error:' bash -eo pipefail "${work}/assemble-signed.sh"
    [[ $failures -eq 0 ]]
  ) || failures=$((failures + 1))
done

[[ $failures -eq 0 ]] || { echo "${failures} recovery release cases failed." >&2; exit 1; }
echo 'All CLI recovery release checks passed (stand-ins only).'
