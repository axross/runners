#!/usr/bin/env bash
# exercises the recovery guards on disposable historical inputs and runs the
# pinned CLI's original release-tool suites against the recovery counterparts.
set -euo pipefail

scripts="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
repo="$(git -C "$scripts" rev-parse --show-toplevel)"
source_commit=dc8e1ebcc0722997206487f3dcca2ae7e519cf02
workflow_commit="$(git -C "$repo" rev-parse HEAD)"
work="$(mktemp -d "${TMPDIR:-/tmp}/test-cli-recovery.XXXXXX")"
trap 'rm -rf -- "$work"' EXIT
checkout="${work}/historical"
git clone --quiet --shared --no-checkout "$repo" "$checkout"
if ! git -C "$checkout" cat-file -e "${source_commit}^{commit}" 2>/dev/null; then
  git -C "$checkout" -c credential.helper= fetch --quiet --no-tags --depth=1 \
    https://github.com/axross/runners.git "$source_commit"
fi
git -C "$checkout" -c core.autocrlf=false checkout --quiet --detach "$source_commit"

failures=0

# checks observable refusal and its cause, rather than accepting any failure.
expect() {
  local name="$1" wanted_status="$2" wanted_text="$3" status=0 output
  shift 3
  output="$("$@" 2>&1)" || status=$?
  if [[ "$status" -ne "$wanted_status" || "$output" != *"$wanted_text"* ]]; then
    echo "FAIL ${name}: exit ${status}, expected ${wanted_status} containing '${wanted_text}': ${output}" >&2
    failures=$((failures + 1))
  else
    echo "ok   ${name}"
  fi
}

expect 'a missing checkout' 1 'historical checkout is missing' \
  bash "${scripts}/prepare-source.sh" prepare "${work}/absent" 1.2.3
git -C "$checkout" checkout --quiet --detach "$workflow_commit"
expect 'another historical commit' 1 'historical HEAD is not' \
  bash "${scripts}/prepare-source.sh" prepare "$checkout" 1.2.3
git -C "$checkout" -c core.autocrlf=false checkout --quiet --detach "$source_commit"
expect 'a nested path instead of a checkout root' 1 'not a checkout root' \
  bash "${scripts}/prepare-source.sh" prepare "${checkout}/cli" 1.2.3
expect 'an unstamped version is not a stamped input' 1 'not the declared version-only stamp' \
  bash "${scripts}/prepare-source.sh" verify "$checkout" 1.2.3

printf '\n' >>"${checkout}/cli/Cargo.lock"
expect 'a dirty original lock' 1 'not clean before version stamping' \
  bash "${scripts}/prepare-source.sh" prepare "$checkout" 1.2.3
git -C "$checkout" restore --worktree -- cli/Cargo.lock
printf '\n' >>"${checkout}/cli/rust-toolchain.toml"
expect 'a dirty toolchain' 1 'not clean before version stamping' \
  bash "${scripts}/prepare-source.sh" prepare "$checkout" 1.2.3
git -C "$checkout" restore --worktree -- cli/rust-toolchain.toml
printf 'unexpected input\n' >"${checkout}/unexpected"
expect 'an untracked input' 1 'untracked files' \
  bash "${scripts}/prepare-source.sh" prepare "$checkout" 1.2.3
rm "${checkout}/unexpected"
mkdir -p "${checkout}/cli/target"
printf 'stale build\n' >"${checkout}/cli/target/marshal"
expect 'an ignored stale build is still an untracked input' 1 'untracked files' \
  bash "${scripts}/prepare-source.sh" prepare "$checkout" 1.2.3
rm -rf -- "${checkout}/cli/target"

for invalid in v1.2.3 01.2.3 1.2.3+build.1 ''; do
  expect "invalid version '${invalid}'" 1 'is not X.Y.Z' \
    bash "${scripts}/prepare-source.sh" prepare "$checkout" "$invalid"
  git -C "$checkout" diff --exit-code
done

expect 'the complete pinned production input' 0 "Historical input: ${source_commit}" \
  bash "${scripts}/prepare-source.sh" prepare "$checkout" 1.2.3
changed="$(git -C "$checkout" diff --name-only | LC_ALL=C sort)"
[[ "$changed" == $'cli/Cargo.lock\ncli/Cargo.toml' ]] || {
  echo "FAIL stamping changed files other than the declared manifest and lock: ${changed}" >&2
  failures=$((failures + 1))
}
expect 'only the declared version changes pass revalidation' 0 'version: 1.2.3' \
  bash "${scripts}/prepare-source.sh" verify "$checkout" 1.2.3
expect 'a conflicting version fails revalidation' 1 'not the declared version-only stamp' \
  bash "${scripts}/prepare-source.sh" verify "$checkout" 1.2.4
expect 'stamping twice cannot hide changed inputs' 1 'not clean before version stamping' \
  bash "${scripts}/prepare-source.sh" prepare "$checkout" 1.2.3

printf '\n' >>"${checkout}/cli/Cargo.lock"
expect 'a non-version lock mutation after stamping' 1 'cli/Cargo.lock is not the declared version-only stamp' \
  bash "${scripts}/prepare-source.sh" verify "$checkout" 1.2.3
git -C "$checkout" restore --worktree -- cli/Cargo.toml cli/Cargo.lock
bash "${scripts}/prepare-source.sh" prepare "$checkout" 1.2.3 >/dev/null
printf '\n' >>"${checkout}/cli/Cargo.toml"
expect 'an extra manifest mutation after stamping' 1 'cli/Cargo.toml is not the declared version-only stamp' \
  bash "${scripts}/prepare-source.sh" verify "$checkout" 1.2.3
git -C "$checkout" restore --worktree -- cli/Cargo.toml cli/Cargo.lock
bash "${scripts}/prepare-source.sh" prepare "$checkout" 1.2.3 >/dev/null

for substitution in helper app; do
  if [[ "$substitution" == helper ]]; then
    cp "${checkout}/cli/helpers/instance_helper.rs" "${checkout}/cli/src/main.rs"
  else
    git -C "$repo" show HEAD:app/crates/app/src/main.rs >"${checkout}/cli/src/main.rs"
  fi
  expect "a ${substitution} substituted for the production entrypoint" 1 'non-version mutation' \
    bash "${scripts}/prepare-source.sh" verify "$checkout" 1.2.3
done
git -C "$checkout" restore --worktree -- cli/src/main.rs
printf '\n' >>"${checkout}/images/actions-runner/Dockerfile"
expect 'a changed external embedded input' 1 'non-version mutation' \
  bash "${scripts}/prepare-source.sh" verify "$checkout" 1.2.3
git -C "$checkout" restore --worktree -- images/actions-runner/Dockerfile
printf 'unexpected after build\n' >"${checkout}/unexpected"
expect 'an untracked input after build' 1 'untracked files' \
  bash "${scripts}/prepare-source.sh" verify "$checkout" 1.2.3
rm "${checkout}/unexpected"
git -C "$checkout" restore --worktree -- cli/Cargo.toml cli/Cargo.lock
expect 'the dry-run prerelease stamp' 0 'version: 0.0.0-dryrun.42' \
  bash "${scripts}/prepare-source.sh" prepare "$checkout" 0.0.0-dryrun.42
expect 'the dry-run stamp is revalidated' 0 'version: 0.0.0-dryrun.42' \
  bash "${scripts}/prepare-source.sh" verify "$checkout" 0.0.0-dryrun.42

harness="${work}/legacy-tests"
mkdir "$harness"
git -C "$checkout" archive "$source_commit" \
  .github/scripts/tests/test-assemble-release.sh \
  .github/scripts/tests/test-verify-release.sh \
  .github/scripts/tests/test-set-release-version.sh \
  .github/scripts/tests/test-installer-version.sh \
  .github/scripts/tests/test-fetch-wix.sh \
  .github/scripts/set-release-version.sh .github/scripts/installer-version.sh \
  .github/scripts/fetch-wix.sh release/.releaserc.json cli/Cargo.toml cli/Cargo.lock \
  | tar -x -C "$harness"
cp "${scripts}/assemble-release.sh" "${scripts}/verify-release.sh" "${harness}/.github/scripts/"
for suite in assemble-release verify-release set-release-version installer-version fetch-wix; do
  bash "${harness}/.github/scripts/tests/test-${suite}.sh" || failures=$((failures + 1))
done

if [[ "$failures" -ne 0 ]]; then
  echo "${failures} recovery case(s) or legacy suites failed." >&2
  exit 1
fi
echo 'All recovery source guards and legacy release-tool suites passed.'
