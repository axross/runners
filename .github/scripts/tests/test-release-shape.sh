#!/usr/bin/env bash
# runs check-release-shape.mjs against copies of the workflows, first unchanged
# (it must pass) and then with one change at a time (each must fail, and name the
# cause). every change is one the release record forbids: an Environment on any
# job or step, in a `.yml` workflow, or in a workflow a pull request calls; a
# signing secret read in any workflow; release.yaml triggered by anything but a
# dispatch; a job without its guard, its runner, its permissions or its place in
# the chain; a step condition that could skip assemble-signed's check; a write to
# SHA256SUMS in the build's assemble job; PATH, BASH_ENV or ENV set in
# release.yaml; and a way past a failed job, whatever the case of its status
# function. a change the test cannot find in the files fails the test, so it
# cannot go stale and pass.
# shellcheck disable=SC2016 # the texts the changes find and replace are literal, not expansions.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository="$(cd "${here}/../../.." && pwd)"
check="${repository}/.github/scripts/check-release-shape.mjs"
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT

pristine="${work}/pristine"
mkdir -p "${pristine}/.github"
cp -r "${repository}/.github/workflows" "${pristine}/.github/workflows"

failures=0

# alter <file> <exact text> <replacement>: replaces the first occurrence, and
# fails the test if there is none.
alter() {
  node -e '
const fs = require("node:fs");
const [file, from, to] = process.argv.slice(1);
const text = fs.readFileSync(file, "utf8");
const index = text.indexOf(from);
if (index === -1) {
  console.error("the test cannot find the text to change in " + file + ": " + from);
  process.exit(3);
}
fs.writeFileSync(file, text.slice(0, index) + to + text.slice(index + from.length));
' "$@"
}

# expect_pass <name>
expect_pass() {
  local output status=0
  output="$(node "$check" "$pristine" 2>&1)" || status=$?
  if [[ $status -ne 0 ]]; then
    echo "FAIL ${1}: exit ${status}: ${output}" >&2
    failures=$((failures + 1))
  else
    echo "ok   ${1}"
  fi
}

# expect_pass_with <name> <file under the root> <exact text> <replacement>: the
# check must still pass on a copy with one change, which shows the rule it
# might trip over is not stricter than the change needs.
expect_pass_with() {
  local name="$1" root output status=0
  root="${work}/case-${name//[^a-zA-Z0-9]/-}"
  cp -r "$pristine" "$root"
  alter "${root}/$2" "$3" "$4" || {
    echo "FAIL ${name}: the change could not be made" >&2
    failures=$((failures + 1))
    return
  }
  output="$(node "$check" "$root" 2>&1)" || status=$?
  if [[ $status -ne 0 ]]; then
    echo "FAIL ${name}: exit ${status}: ${output}" >&2
    failures=$((failures + 1))
  else
    echo "ok   ${name}"
  fi
}

# expect_fail <name> <text the output must hold> <file under the root> <exact text> <replacement> [<file> <text> <replacement>]...
expect_fail() {
  local name="$1" want="$2" root output status=0
  shift 2
  root="${work}/case-${name//[^a-zA-Z0-9]/-}"
  cp -r "$pristine" "$root"
  while [[ $# -gt 0 ]]; do
    alter "${root}/$1" "$2" "$3" || {
      echo "FAIL ${name}: the change could not be made" >&2
      failures=$((failures + 1))
      return
    }
    shift 3
  done
  output="$(node "$check" "$root" 2>&1)" || status=$?
  if [[ $status -ne 1 ]]; then
    echo "FAIL ${name}: exit ${status}, expected 1: ${output}" >&2
    failures=$((failures + 1))
  elif [[ "$output" != *"$want"* ]]; then
    echo "FAIL ${name}: the output does not hold '${want}': ${output}" >&2
    failures=$((failures + 1))
  else
    echo "ok   ${name}"
  fi
}

# expect_fail_new_file <name> <text the output must hold> <file under the root> <content>: adds a file the
# unchanged workflows do not have.
expect_fail_new_file() {
  local name="$1" want="$2" root output status=0
  root="${work}/case-${name//[^a-zA-Z0-9]/-}"
  cp -r "$pristine" "$root"
  printf '%s' "$4" >"${root}/$3"
  output="$(node "$check" "$root" 2>&1)" || status=$?
  if [[ $status -ne 1 ]]; then
    echo "FAIL ${name}: exit ${status}, expected 1: ${output}" >&2
    failures=$((failures + 1))
  elif [[ "$output" != *"$want"* ]]; then
    echo "FAIL ${name}: the output does not hold '${want}': ${output}" >&2
    failures=$((failures + 1))
  else
    echo "ok   ${name}"
  fi
}

release=.github/workflows/release.yaml
release_build=.github/workflows/release-build.yaml
merge_checks=.github/workflows/merge-checks.yaml

expect_pass "the unchanged workflows"

# the lines the changes below are made at.
guard="    if: github.ref == 'refs/heads/main' && needs.plan.outputs.version != ''"
assemble_header=$'  assemble-signed:\n    name: Assemble The Signed Release Files\n'
assemble_needs=$'    needs: [plan, build]\n'"$guard"
assemble_runner=$'    runs-on: ubuntu-latest\n    timeout-minutes: 10\n    permissions:\n      contents: read\n    outputs:\n      artifact: marshal-release-signed'
assemble_check=$'      - name: Check The Hashes And Write SHA256SUMS\n        env:\n          VERSION: ${{ needs.plan.outputs.version }}\n        run: |'
attest_header=$'  attest:\n    name: Attest The Build Provenance\n'
attest_download=$'      - name: Download The Release Files\n        uses: actions/download-artifact@v8\n        with:\n          name: marshal-release-signed\n          path: release-files\n      # the same check'
release_download=$'      - name: Download The Release Files\n        uses: actions/download-artifact@v8\n        with:\n          name: marshal-release-signed\n          path: release-files\n      - name: Release'

echo "-- the Environment --"
# no job declares one until the next signing job.
for job in plan build assemble-signed attest release; do
  expect_fail "an Environment on ${job}" "job ${job} declares an environment" \
    "$release" $'\n  '"${job}"$':\n    name: ' $'\n  '"${job}"$':\n    environment: release\n    name: '
done
expect_fail "an Environment in release-build.yaml" "release-build.yaml: job assemble declares an environment" \
  "$release_build" $'  assemble:\n    name: Assemble The Release Files\n' $'  assemble:\n    name: Assemble The Release Files\n    environment: release\n'
expect_fail "an Environment on the build leg" "release-build.yaml: job build declares an environment" \
  "$release_build" $'  build:\n    name: Build windows-x64\n' $'  build:\n    name: Build windows-x64\n    environment: release\n'
expect_fail "an Environment in merge-checks.yaml" "merge-checks.yaml: job sneaky declares an environment" \
  "$merge_checks" $'jobs:\n' $'jobs:\n  sneaky:\n    runs-on: ubuntu-latest\n    environment: release\n    steps:\n      - run: echo hi\n'
expect_fail "an Environment in a step" "a step of job assemble-signed declares an environment" \
  "$release" $'      - name: Download The Build\'s Release Files\n' $'      - name: Download The Build\'s Release Files\n        environment: release\n'
expect_fail_new_file "an Environment in a .yml workflow" "extra.yml: job sneaky declares an environment" \
  .github/workflows/extra.yml $'name: Extra\non:\n  workflow_dispatch:\njobs:\n  sneaky:\n    runs-on: ubuntu-latest\n    environment: release\n    steps:\n      - run: echo hi\n'
expect_fail "a pull request workflow that calls a workflow it cannot be checked against" "which this check cannot follow" \
  "$merge_checks" $'jobs:\n' $'jobs:\n  sneaky:\n    uses: someone/else/.github/workflows/sign.yaml@main\n'
expect_fail "release.yaml triggered by pull requests" "release.yaml is triggered by" \
  "$release" $'on:\n  workflow_dispatch:\n' $'on:\n  workflow_dispatch:\n  pull_request:\n'

echo "-- the signing secrets --"
expect_fail "a signing secret in attest" "no job may read a signing secret" \
  "$release" "$attest_header" "$attest_header"$'    env:\n      PASSWORD: ${{ secrets.MACOS_P12_PASSWORD }}\n'
expect_fail "a signing secret in assemble-signed's step" "release.yaml: jobs.assemble-signed.steps[2].env.P8 reads" \
  "$release" "$assemble_check" $'      - name: Check The Hashes And Write SHA256SUMS\n        env:\n          P8: ${{ secrets.MACOS_NOTARY_KEY_P8 }}\n          VERSION: ${{ needs.plan.outputs.version }}\n        run: |'
expect_fail "a signing secret in the workflow's env" "release.yaml: env.P12 reads" \
  "$release" $'permissions:\n  contents: read\n' $'permissions:\n  contents: read\n\nenv:\n  P12: ${{ secrets.MACOS_INSTALLER_P12_BASE64 }}\n'
expect_fail "a signing secret read by bracket" "no job may read a signing secret" \
  "$release" "$attest_header" "$attest_header"$'    env:\n      PASSWORD: ${{ secrets[\'MACOS_P12_PASSWORD\'] }}\n'
expect_fail "every secret read through toJSON" "no job may read a signing secret" \
  "$release" "$attest_header" "$attest_header"$'    env:\n      ALL: ${{ toJSON(secrets) }}\n'
expect_fail "secrets: inherit on the build job" "release.yaml: jobs.build.secrets passes every secret on" \
  "$release" $'    uses: ./.github/workflows/release-build.yaml\n' $'    uses: ./.github/workflows/release-build.yaml\n    secrets: inherit\n'
expect_fail "a signing secret in a pull request workflow" "merge-checks.yaml: jobs.sneaky.env.PASSWORD reads" \
  "$merge_checks" $'jobs:\n' $'jobs:\n  sneaky:\n    runs-on: ubuntu-latest\n    env:\n      PASSWORD: ${{ secrets.MACOS_P12_PASSWORD }}\n    steps:\n      - run: echo hi\n'
expect_fail_new_file "every secret passed on in a .yml workflow" "extra.yml: jobs.sneaky.secrets passes every secret on" \
  .github/workflows/extra.yml $'name: Extra\non:\n  workflow_dispatch:\njobs:\n  sneaky:\n    uses: ./.github/workflows/release-build.yaml\n    secrets: inherit\n'
expect_fail "a signing secret in release-build.yaml" "release-build.yaml: jobs.assemble" \
  "$release_build" $'  assemble:\n    name: Assemble The Release Files\n' $'  assemble:\n    name: Assemble The Release Files\n    env:\n      PASSWORD: ${{ secrets.MACOS_P12_PASSWORD }}\n'

echo "-- assemble-signed --"
expect_fail "assemble-signed with an extra permission" "release.yaml assemble-signed must hold exactly" \
  "$release" "$assemble_runner" $'    runs-on: ubuntu-latest\n    timeout-minutes: 10\n    permissions:\n      contents: read\n      id-token: write\n    outputs:\n      artifact: marshal-release-signed'
expect_fail "assemble-signed with contents: write" "release.yaml assemble-signed must hold exactly" \
  "$release" "$assemble_runner" $'    runs-on: ubuntu-latest\n    timeout-minutes: 10\n    permissions:\n      contents: write\n    outputs:\n      artifact: marshal-release-signed'
expect_fail "assemble-signed with a timeout beyond 10 minutes" "release.yaml assemble-signed must set timeout-minutes to at most 10" \
  "$release" "$assemble_runner" $'    runs-on: ubuntu-latest\n    timeout-minutes: 30\n    permissions:\n      contents: read\n    outputs:\n      artifact: marshal-release-signed'
expect_fail "assemble-signed on a self-hosted runner" "release.yaml assemble-signed must run on ubuntu-latest" \
  "$release" "$assemble_runner" $'    runs-on: [self-hosted, linux]\n    timeout-minutes: 10\n    permissions:\n      contents: read\n    outputs:\n      artifact: marshal-release-signed'
expect_fail "assemble-signed with a checkout that keeps credentials" "release.yaml assemble-signed: a checkout does not set persist-credentials: false" \
  "$release" "$assemble_runner"$'\n    steps:\n      - name: Checkout\n        uses: actions/checkout@v7\n        with:\n          persist-credentials: false\n' "$assemble_runner"$'\n    steps:\n      - name: Checkout\n        uses: actions/checkout@v7\n'
expect_fail "assemble-signed that does not call the script" "release.yaml assemble-signed does not call assemble-release.sh" \
  "$release" 'bash .github/scripts/assemble-release.sh "$VERSION" dist "$windows_sha256"' 'bash .github/scripts/other.sh "$VERSION" dist "$windows_sha256"'

echo "-- release-build.yaml's assemble --"
expect_fail "release-build's assemble with a checkout that keeps credentials" "release-build.yaml assemble: a checkout does not set persist-credentials: false" \
  "$release_build" $'    steps:\n      - name: Checkout\n        uses: actions/checkout@v7\n        with:\n          persist-credentials: false\n      - name: Download The Windows Files' $'    steps:\n      - name: Checkout\n        uses: actions/checkout@v7\n      - name: Download The Windows Files'
expect_fail "a build whose assemble job does not call the script" "release-build.yaml assemble does not call assemble-release.sh" \
  "$release_build" 'bash .github/scripts/assemble-release.sh' 'bash .github/scripts/other.sh'
assemble_call='bash .github/scripts/assemble-release.sh "$VERSION" dist "$WINDOWS_SHA256" "$WINDOWS_MSI_SHA256"'
for writing in 'sha256sum dist/* > dist/SHA256SUMS' 'sha256sum dist/* >> dist/SHA256SUMS' 'sha256sum dist/* | tee dist/SHA256SUMS' 'cp sums.txt dist/SHA256SUMS' 'mv sums.txt "dist/SHA256SUMS"'; do
  expect_fail "release-build's assemble writing SHA256SUMS with ${writing}" "assemble-release.sh is the only writer" \
    "$release_build" "        run: ${assemble_call}" "        run: |"$'\n'"          ${assemble_call}"$'\n'"          ${writing}"
done
for reading in 'cat dist/SHA256SUMS' 'sha256sum -c dist/SHA256SUMS' 'cp dist/SHA256SUMS sums.txt' 'mv dist/SHA256SUMS dist/SHA256SUMS.old'; do
  expect_pass_with "release-build's assemble reading SHA256SUMS with ${reading}" \
    "$release_build" "        run: ${assemble_call}" "        run: |"$'\n'"          ${assemble_call}"$'\n'"          ${reading}"
done

echo "-- the chain and the guard --"
expect_fail "assemble-signed without the guard" "release.yaml assemble-signed must carry" \
  "$release" "$assemble_needs"$'\n' $'    needs: [plan, build]\n'
expect_fail "assemble-signed with a weaker guard" "release.yaml assemble-signed must carry" \
  "$release" "$assemble_needs" $'    needs: [plan, build]\n    if: github.ref == \'refs/heads/main\''
expect_fail "assemble-signed with a guard on another branch" "release.yaml assemble-signed must carry" \
  "$release" "$assemble_needs" $'    needs: [plan, build]\n    if: github.ref == \'refs/heads/dev\' && needs.plan.outputs.version != \'\''
expect_fail "build without the guard" "release.yaml build must carry" \
  "$release" $'    needs: plan\n'"$guard"$'\n' $'    needs: plan\n'
expect_fail "attest without the guard" "release.yaml attest must carry" \
  "$release" $'    needs: [plan, assemble-signed]\n'"$guard"$'\n' $'    needs: [plan, assemble-signed]\n'
expect_fail "attest with a weaker guard" "release.yaml attest must carry" \
  "$release" $'    needs: [plan, assemble-signed]\n'"$guard" $'    needs: [plan, assemble-signed]\n    if: github.ref == \'refs/heads/main\''
expect_fail "release without the version in its guard" "release.yaml release must carry" \
  "$release" $'    needs: [plan, attest]\n'"$guard" $'    needs: [plan, attest]\n    if: github.ref == \'refs/heads/main\' && needs.plan.outputs.version != \'x\''
expect_fail "assemble-signed without plan in needs" "release.yaml assemble-signed must need plan directly" \
  "$release" "$assemble_needs" $'    needs: [build]\n'"$guard"
expect_fail "assemble-signed not after build" "release.yaml assemble-signed must need build directly" \
  "$release" "$assemble_needs" $'    needs: [plan]\n'"$guard"
expect_fail "attest not after assemble-signed" "release.yaml attest must need assemble-signed directly" \
  "$release" $'    needs: [plan, assemble-signed]' $'    needs: [plan, build]'
expect_fail "release not after attest" "release.yaml release must need attest directly" \
  "$release" $'    needs: [plan, attest]' $'    needs: [plan, build]'
expect_fail "a release.yaml without assemble-signed" "release.yaml has no assemble-signed job" \
  "$release" "$assemble_header" $'  assemble-other:\n    name: Assemble The Signed Release Files\n'
expect_fail "attest reading the unsigned set" "release.yaml attest must download exactly marshal-release-signed" \
  "$release" "$attest_download" $'      - name: Download The Release Files\n        uses: actions/download-artifact@v8\n        with:\n          name: marshal-release\n          path: release-files\n      # the same check'
expect_fail "release reading the unsigned set" "release.yaml release must download exactly marshal-release-signed" \
  "$release" "$release_download" $'      - name: Download The Release Files\n        uses: actions/download-artifact@v8\n        with:\n          name: ${{ needs.build.outputs.artifact }}\n          path: release-files\n      - name: Release'
expect_fail "attest with a further permission" "release.yaml attest must hold exactly" \
  "$release" $'      id-token: write\n      attestations: write\n' $'      id-token: write\n      attestations: write\n      contents: write\n'
expect_fail "a step of release.yaml that sets PATH in its env" "sets PATH, BASH_ENV or ENV" \
  "$release" "$assemble_check" $'      - name: Check The Hashes And Write SHA256SUMS\n        env:\n          PATH: /tmp/fake\n          VERSION: ${{ needs.plan.outputs.version }}\n        run: |'
expect_fail "a workflow that sets BASH_ENV in its env" "sets PATH, BASH_ENV or ENV" \
  "$release" $'permissions:\n  contents: read\n' $'permissions:\n  contents: read\n\nenv:\n  BASH_ENV: /tmp/fake.sh\n'

echo "-- no way past a failed job --"
expect_fail "continue-on-error on assemble-signed" "release.yaml assemble-signed sets continue-on-error" \
  "$release" "$assemble_header" "$assemble_header"$'    continue-on-error: true\n'
expect_fail "continue-on-error on release" "release.yaml release sets continue-on-error" \
  "$release" $'  release:\n    name: Release\n' $'  release:\n    name: Release\n    continue-on-error: true\n'
expect_fail "continue-on-error on a step" "sets continue-on-error" \
  "$release" $'      - name: Attest The Build Provenance\n' $'      - name: Attest The Build Provenance\n        continue-on-error: true\n'
expect_fail "always() in assemble-signed's condition" "has a job-level condition that can run it after a failure" \
  "$release" "$assemble_needs" $'    needs: [plan, build]\n    if: always() && needs.plan.outputs.version != \'\''
expect_fail "!cancelled() in attest's condition" "has a job-level condition that can run it after a failure" \
  "$release" $'    needs: [plan, assemble-signed]\n'"$guard" $'    needs: [plan, assemble-signed]\n    if: ${{ !cancelled() }}'
expect_fail "failure() in release's condition" "has a job-level condition that can run it after a failure" \
  "$release" $'    needs: [plan, attest]\n'"$guard" $'    needs: [plan, attest]\n    if: failure()'
expect_fail "always() on a step of attest" "can run after a failure" \
  "$release" $'      - name: Attest The Build Provenance\n' $'      - name: Attest The Build Provenance\n        if: always()\n'
expect_fail "Always() in assemble-signed's condition" "has a job-level condition that can run it after a failure" \
  "$release" "$assemble_needs" $'    needs: [plan, build]\n    if: Always() && needs.plan.outputs.version != \'\''
expect_fail "ALWAYS() on a step of release" "can run after a failure" \
  "$release" $'      - name: Release\n' $'      - name: Release\n        if: ${{ ALWAYS() }}\n'
# a condition that skips a step also skips a check.
expect_fail "a condition on a step of assemble-signed" "release.yaml assemble-signed: step \"Check The Hashes And Write SHA256SUMS\" has a condition" \
  "$release" "$assemble_check" $'      - name: Check The Hashes And Write SHA256SUMS\n        if: false\n        env:\n          VERSION: ${{ needs.plan.outputs.version }}\n        run: |'

echo "-- a file the check cannot read --"
expect_fail "a workflow in a syntax the check does not support" "cannot be read" \
  "$release" $'permissions:\n  contents: read\n' $'permissions: {contents: read}\n'

if [[ $failures -ne 0 ]]; then
  echo "${failures} release shape case(s) failed." >&2
  exit 1
fi
echo "All release shape cases passed."
