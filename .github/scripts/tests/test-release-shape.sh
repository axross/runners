#!/usr/bin/env bash
# runs check-release-shape.mjs against copies of the workflows, first unchanged
# (it must pass) and then with one change at a time (each must fail, and name the
# cause). every change is one the release record forbids: an Environment on any
# job or step but sign-update, in a `.yml` workflow, or in a workflow a pull
# request calls; a secret read in the release workflows outside sign-update's
# sign step, or a signing secret read in any other workflow; release.yaml
# triggered by anything but a dispatch; a job without its guard, its runner, its
# permissions or its place in the chain; in sign-update, a checkout, another
# action, a package manager, a fetcher, a tool called by name, an unmarked
# script or a `${{ }}` in one, or a signer run another way; a feed checked
# against another key; a step condition that could skip a check; a write to
# SHA256SUMS in the build's assemble job; PATH, BASH_ENV, ENV or SYSTEM_BIN set
# in release.yaml; and a way past a failed job, whatever the case of its status
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
assemble_needs=$'    needs: [plan, build, update-tool, sign-update]\n'"$guard"
assemble_runner=$'    runs-on: ubuntu-latest\n    timeout-minutes: 10\n    permissions:\n      contents: read\n    outputs:\n      artifact: marshal-release-signed'
assemble_check=$'      - name: Check The Hashes, Write The Feed And Check It, And Write SHA256SUMS\n        env:\n          VERSION: ${{ needs.plan.outputs.version }}'
sign_needs=$'    needs: [plan, build, update-tool]\n'"$guard"
sign_runner=$'    runs-on: ubuntu-latest\n    timeout-minutes: 15\n    environment: release\n    permissions:\n      contents: read\n'
sign_key=$'          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.UPDATE_FEED_SIGNING_KEY }}\n'
check_marker=$'          # sign-update: check\n'
sign_marker=$'          # sign-update: sign\n'
tool_header=$'  update-tool:\n    name: Build The Update Tools\n'
tool_runner=$'    runs-on: ubuntu-latest\n    timeout-minutes: 40\n    permissions:\n      contents: read\n'
attest_header=$'  attest:\n    name: Attest The Build Provenance\n'
attest_download=$'      - name: Download The Release Files\n        uses: actions/download-artifact@v8\n        with:\n          name: marshal-release-signed\n          path: release-files\n      # the same check'
release_download=$'      - name: Download The Release Files\n        uses: actions/download-artifact@v8\n        with:\n          name: marshal-release-signed\n          path: release-files\n      - name: Release'

echo "-- the Environment --"
# no job declares one but sign-update.
for job in plan build update-tool assemble-signed attest release; do
  expect_fail "an Environment on ${job}" "job ${job} declares an environment" \
    "$release" $'\n  '"${job}"$':\n    name: ' $'\n  '"${job}"$':\n    environment: release\n    name: '
done
expect_fail "an Environment in release-build.yaml" "release-build.yaml: job assemble declares an environment" \
  "$release_build" $'  assemble:\n    name: Assemble The Release Files\n' $'  assemble:\n    name: Assemble The Release Files\n    environment: release\n'
expect_fail "an Environment on the build leg" "release-build.yaml: job build declares an environment" \
  "$release_build" $'  build:\n    name: Build windows-x64\n' $'  build:\n    name: Build windows-x64\n    environment: release\n'
expect_fail "an Environment in merge-checks.yaml" "merge-checks.yaml: job sneaky declares an environment" \
  "$merge_checks" $'jobs:\n' $'jobs:\n  sneaky:\n    runs-on: ubuntu-latest\n    environment: release\n    steps:\n      - run: echo hi\n'
expect_fail "an Environment in a step" "a step of job sign-update declares an environment" \
  "$release" $'      - name: Download The Build\'s Release Files\n' $'      - name: Download The Build\'s Release Files\n        environment: release\n'
expect_fail_new_file "an Environment in a .yml workflow" "extra.yml: job sneaky declares an environment" \
  .github/workflows/extra.yml $'name: Extra\non:\n  workflow_dispatch:\njobs:\n  sneaky:\n    runs-on: ubuntu-latest\n    environment: release\n    steps:\n      - run: echo hi\n'
expect_fail "a pull request workflow that calls a workflow it cannot be checked against" "which this check cannot follow" \
  "$merge_checks" $'jobs:\n' $'jobs:\n  sneaky:\n    uses: someone/else/.github/workflows/sign.yaml@main\n'
expect_fail "release.yaml triggered by pull requests" "release.yaml is triggered by" \
  "$release" $'on:\n  workflow_dispatch:\n' $'on:\n  workflow_dispatch:\n  pull_request:\n'

echo "-- the signing secrets --"
expect_fail "a signing secret in attest" "only sign-update's sign step may read a secret in the release workflows" \
  "$release" "$attest_header" "$attest_header"$'    env:\n      PASSWORD: ${{ secrets.MACOS_P12_PASSWORD }}\n'
expect_fail "a signing secret in assemble-signed's step" "release.yaml: jobs.assemble-signed.steps[4].env.P8 reads" \
  "$release" "$assemble_check" $'      - name: Check The Hashes, Write The Feed And Check It, And Write SHA256SUMS\n        env:\n          P8: ${{ secrets.MACOS_NOTARY_KEY_P8 }}\n          VERSION: ${{ needs.plan.outputs.version }}'
expect_fail "a signing secret in the workflow's env" "release.yaml: env.P12 reads" \
  "$release" $'permissions:\n  contents: read\n' $'permissions:\n  contents: read\n\nenv:\n  P12: ${{ secrets.MACOS_INSTALLER_P12_BASE64 }}\n'
expect_fail "a signing secret read by bracket" "only sign-update's sign step may read a secret in the release workflows" \
  "$release" "$attest_header" "$attest_header"$'    env:\n      PASSWORD: ${{ secrets[\'MACOS_P12_PASSWORD\'] }}\n'
expect_fail "every secret read through toJSON" "only sign-update's sign step may read a secret in the release workflows" \
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
expect_fail "assemble-signed that does not call the script" "release.yaml assemble-signed does not run .github/scripts/assemble-release.sh" \
  "$release" 'bash .github/scripts/assemble-release.sh signed "$VERSION" dist "$windows_sha256"' 'bash .github/scripts/other.sh signed "$VERSION" dist "$windows_sha256"'
expect_fail "assemble-signed that writes no feed" "does not write the feed with write-update-feed.sh" \
  "$release" 'bash .github/scripts/write-update-feed.sh' 'bash .github/scripts/other-feed.sh'
expect_fail "assemble-signed that does not check the feed" "does not run the feed check on dist/latest.json" \
  "$release" '"$check" dist/latest.json' 'echo dist/latest.json'
expect_fail "assemble-signed that checks the feed against another key" "the release checks the feed against the committed key" \
  "$release" '"$check" dist/latest.json' '"$check" --public-key other.pub dist/latest.json'
expect_fail "assemble-signed in unsigned mode" "does not run assemble-release.sh in signed mode" \
  "$release" 'assemble-release.sh signed' 'assemble-release.sh unsigned'

echo "-- release-build.yaml's assemble --"
expect_fail "release-build's assemble with a checkout that keeps credentials" "release-build.yaml assemble: a checkout does not set persist-credentials: false" \
  "$release_build" $'    steps:\n      - name: Checkout\n        uses: actions/checkout@v7\n        with:\n          persist-credentials: false\n      - name: Download The Windows Files' $'    steps:\n      - name: Checkout\n        uses: actions/checkout@v7\n      - name: Download The Windows Files'
expect_fail "a build whose assemble job does not call the script" "release-build.yaml assemble does not call assemble-release.sh" \
  "$release_build" 'bash .github/scripts/assemble-release.sh' 'bash .github/scripts/other.sh'
assemble_call='bash .github/scripts/assemble-release.sh unsigned "$VERSION" dist "$WINDOWS_SHA256" "$WINDOWS_MSI_SHA256"'
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
  "$release" "$assemble_needs"$'\n' $'    needs: [plan, build, update-tool, sign-update]\n'
expect_fail "assemble-signed with a weaker guard" "release.yaml assemble-signed must carry" \
  "$release" "$assemble_needs" $'    needs: [plan, build, update-tool, sign-update]\n    if: github.ref == \'refs/heads/main\''
expect_fail "assemble-signed with a guard on another branch" "release.yaml assemble-signed must carry" \
  "$release" "$assemble_needs" $'    needs: [plan, build, update-tool, sign-update]\n    if: github.ref == \'refs/heads/dev\' && needs.plan.outputs.version != \'\''
expect_fail "build without the guard" "release.yaml build must carry" \
  "$release" $'    needs: plan\n'"$guard"$'\n' $'    needs: plan\n'
expect_fail "attest without the guard" "release.yaml attest must carry" \
  "$release" $'    needs: [plan, assemble-signed]\n'"$guard"$'\n' $'    needs: [plan, assemble-signed]\n'
expect_fail "attest with a weaker guard" "release.yaml attest must carry" \
  "$release" $'    needs: [plan, assemble-signed]\n'"$guard" $'    needs: [plan, assemble-signed]\n    if: github.ref == \'refs/heads/main\''
expect_fail "release without the version in its guard" "release.yaml release must carry" \
  "$release" $'    needs: [plan, attest]\n'"$guard" $'    needs: [plan, attest]\n    if: github.ref == \'refs/heads/main\' && needs.plan.outputs.version != \'x\''
expect_fail "assemble-signed without plan in needs" "release.yaml assemble-signed must need plan directly" \
  "$release" "$assemble_needs" $'    needs: [build, update-tool, sign-update]\n'"$guard"
expect_fail "assemble-signed not after build" "release.yaml assemble-signed must need build directly" \
  "$release" "$assemble_needs" $'    needs: [plan, update-tool, sign-update]\n'"$guard"
expect_fail "assemble-signed not after sign-update" "release.yaml assemble-signed must need sign-update directly" \
  "$release" "$assemble_needs" $'    needs: [plan, build, update-tool]\n'"$guard"
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
expect_fail "a step of release.yaml that sets PATH in its env" "sets PATH, BASH_ENV, ENV or SYSTEM_BIN" \
  "$release" "$assemble_check" $'      - name: Check The Hashes, Write The Feed And Check It, And Write SHA256SUMS\n        env:\n          PATH: /tmp/fake\n          VERSION: ${{ needs.plan.outputs.version }}'
expect_fail "a workflow that sets BASH_ENV in its env" "sets PATH, BASH_ENV, ENV or SYSTEM_BIN" \
  "$release" $'permissions:\n  contents: read\n' $'permissions:\n  contents: read\n\nenv:\n  BASH_ENV: /tmp/fake.sh\n'

echo "-- no way past a failed job --"
expect_fail "continue-on-error on assemble-signed" "release.yaml assemble-signed sets continue-on-error" \
  "$release" "$assemble_header" "$assemble_header"$'    continue-on-error: true\n'
expect_fail "continue-on-error on release" "release.yaml release sets continue-on-error" \
  "$release" $'  release:\n    name: Release\n' $'  release:\n    name: Release\n    continue-on-error: true\n'
expect_fail "continue-on-error on a step" "sets continue-on-error" \
  "$release" $'      - name: Attest The Build Provenance\n' $'      - name: Attest The Build Provenance\n        continue-on-error: true\n'
expect_fail "always() in assemble-signed's condition" "has a job-level condition that can run it after a failure" \
  "$release" "$assemble_needs" $'    needs: [plan, build, update-tool, sign-update]\n    if: always() && needs.plan.outputs.version != \'\''
expect_fail "!cancelled() in attest's condition" "has a job-level condition that can run it after a failure" \
  "$release" $'    needs: [plan, assemble-signed]\n'"$guard" $'    needs: [plan, assemble-signed]\n    if: ${{ !cancelled() }}'
expect_fail "failure() in release's condition" "has a job-level condition that can run it after a failure" \
  "$release" $'    needs: [plan, attest]\n'"$guard" $'    needs: [plan, attest]\n    if: failure()'
expect_fail "always() on a step of attest" "can run after a failure" \
  "$release" $'      - name: Attest The Build Provenance\n' $'      - name: Attest The Build Provenance\n        if: always()\n'
expect_fail "Always() in assemble-signed's condition" "has a job-level condition that can run it after a failure" \
  "$release" "$assemble_needs" $'    needs: [plan, build, update-tool, sign-update]\n    if: Always() && needs.plan.outputs.version != \'\''
expect_fail "ALWAYS() on a step of release" "can run after a failure" \
  "$release" $'      - name: Release\n' $'      - name: Release\n        if: ${{ ALWAYS() }}\n'
# a condition that skips a step also skips a check.
expect_fail "a condition on a step of assemble-signed" "release.yaml assemble-signed: step \"Check The Hashes, Write The Feed And Check It, And Write SHA256SUMS\" has a condition" \
  "$release" "$assemble_check" $'      - name: Check The Hashes, Write The Feed And Check It, And Write SHA256SUMS\n        if: false\n        env:\n          VERSION: ${{ needs.plan.outputs.version }}'
expect_fail "a condition on the sign step" "release.yaml sign-update: step \"Sign The Installer\" has a condition" \
  "$release" $'      - name: Sign The Installer\n' $'      - name: Sign The Installer\n        if: false\n'
expect_fail "a condition on a step of update-tool" "release.yaml update-tool: step \"Build The Update Tools\" has a condition" \
  "$release" $'      - name: Build The Update Tools\n' $'      - name: Build The Update Tools\n        if: false\n'

echo "-- sign-update --"
expect_fail "sign-update without the Environment" "release.yaml sign-update must declare \`environment: release\`" \
  "$release" "$sign_runner" $'    runs-on: ubuntu-latest\n    timeout-minutes: 15\n    permissions:\n      contents: read\n'
expect_fail "sign-update in another Environment" "only release.yaml's sign-update may, and only \`release\`" \
  "$release" "$sign_runner" $'    runs-on: ubuntu-latest\n    timeout-minutes: 15\n    environment: production\n    permissions:\n      contents: read\n'
expect_fail "sign-update with contents: write" "release.yaml sign-update must hold exactly" \
  "$release" "$sign_runner" $'    runs-on: ubuntu-latest\n    timeout-minutes: 15\n    environment: release\n    permissions:\n      contents: write\n'
expect_fail "sign-update on a self-hosted runner" "release.yaml sign-update must run on ubuntu-latest" \
  "$release" "$sign_runner" $'    runs-on: [self-hosted, linux]\n    timeout-minutes: 15\n    environment: release\n    permissions:\n      contents: read\n'
expect_fail "sign-update with a timeout beyond 15 minutes" "release.yaml sign-update must set timeout-minutes to at most 15" \
  "$release" "$sign_runner" $'    runs-on: ubuntu-latest\n    timeout-minutes: 30\n    environment: release\n    permissions:\n      contents: read\n'
expect_fail "sign-update without plan in needs" "release.yaml sign-update must need plan directly" \
  "$release" "$sign_needs" $'    needs: [build, update-tool]\n'"$guard"
expect_fail "sign-update not after update-tool" "release.yaml sign-update must need update-tool directly" \
  "$release" "$sign_needs" $'    needs: [plan, build]\n'"$guard"
expect_fail "sign-update without the guard" "release.yaml sign-update must carry" \
  "$release" "$sign_needs"$'\n' $'    needs: [plan, build, update-tool]\n'
expect_fail "a checkout in sign-update" "release.yaml sign-update: step \"Checkout\" checks out the repository" \
  "$release" "$sign_runner"$'    steps:\n' "$sign_runner"$'    steps:\n      - name: Checkout\n        uses: actions/checkout@v7\n        with:\n          persist-credentials: false\n'
expect_fail "another action in sign-update" "uses actions/setup-node@v7; only actions/download-artifact and actions/upload-artifact are allowed" \
  "$release" "$sign_runner"$'    steps:\n' "$sign_runner"$'    steps:\n      - name: Node\n        uses: actions/setup-node@v7\n'
expect_fail "an artifact download from another repository" "gives actions/download-artifact the input repository" \
  "$release" $'          name: update-tools\n          path: ${{ runner.temp }}/update-tools\n      - name: Check The Installer' $'          name: update-tools\n          repository: someone/else\n          path: ${{ runner.temp }}/update-tools\n      - name: Check The Installer'
expect_fail "a package manager in sign-update" "runs the package manager npm" \
  "$release" "$check_marker" "$check_marker"$'          npm ci\n'
expect_fail "a download in sign-update" "runs curl; the job downloads nothing but its artifacts" \
  "$release" "$check_marker" "$check_marker"$'          curl -o x https://example.com/x\n'
expect_fail "an interpreter in sign-update" "runs bash, which could run a file from the repository" \
  "$release" "$check_marker" "$check_marker"$'          bash other.sh\n'
expect_fail "a tool called by name in sign-update" "calls sha256sum by name, not by its system path" \
  "$release" "$check_marker" "$check_marker"$'          sha256sum x\n'
expect_fail "a system tool sign-update does not use" "calls \$SYSTEM_BIN/python3, which is not one of" \
  "$release" "$check_marker" "$check_marker"$'          "$SYSTEM_BIN/python3" x\n'
expect_fail "another default for SYSTEM_BIN" "gives SYSTEM_BIN a default other than /usr/bin" \
  "$release" "$check_marker"$'          set -euo pipefail\n          : "${SYSTEM_BIN:=/usr/bin}"' "$check_marker"$'          set -euo pipefail\n          : "${SYSTEM_BIN:=/tmp/bin}"'
expect_fail "a script without its marker" "does not start with a \"# sign-update: <name>\" marker line" \
  "$release" "$check_marker" $'          # check\n'
expect_fail "a \${{ }} inside a sign-update script" "expands \${{ }} inside run" \
  "$release" "$check_marker" "$check_marker"$'          echo "${{ github.ref }}"\n'
expect_fail "a write to GITHUB_PATH in sign-update" "refers to GITHUB_PATH" \
  "$release" "$check_marker" "$check_marker"$'          echo /tmp >> "$GITHUB_PATH"\n'
expect_fail "the signer run outside unshare" "the sign step does not run the signer as" \
  "$release" '"$SYSTEM_BIN/unshare" --map-root-user --net "${RUNNER_TEMP}/update-tools/cargo-tauri" signer sign' '"${RUNNER_TEMP}/update-tools/cargo-tauri" signer sign'
expect_fail "the signer run in the check step" "runs the signer outside the sign step" \
  "$release" "$check_marker" "$check_marker"$'          "$SYSTEM_BIN/unshare" --map-root-user --net "${RUNNER_TEMP}/update-tools/cargo-tauri" signer sign x\n'
expect_fail "a sign step without its marker" "must have exactly one \"# sign-update: sign\" step" \
  "$release" "$sign_marker" $'          # sign-update: signing\n'
expect_fail "the key read from another secret" "the sign step must set TAURI_SIGNING_PRIVATE_KEY to" \
  "$release" "$sign_key" $'          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.OTHER_KEY }}\n'
expect_fail "a secret in sign-update's check step" "release.yaml: jobs.sign-update.steps[2].env.KEY reads" \
  "$release" $'          TAURI_CLI_SHA256: ${{ needs.update-tool.outputs.tauri_cli_sha256 }}\n' $'          TAURI_CLI_SHA256: ${{ needs.update-tool.outputs.tauri_cli_sha256 }}\n          KEY: ${{ secrets.UPDATE_FEED_SIGNING_KEY }}\n'
expect_fail "another secret in the sign step" "release.yaml: jobs.sign-update.steps[3].env.TOKEN reads" \
  "$release" "$sign_key" "$sign_key"$'          TOKEN: ${{ secrets.GITHUB_TOKEN }}\n'
expect_fail "any secret in update-tool" "release.yaml: jobs.update-tool.env.TOKEN reads" \
  "$release" "$tool_header" "$tool_header"$'    env:\n      TOKEN: ${{ secrets.NPM_TOKEN }}\n'
expect_fail "any secret in release-build.yaml" "release-build.yaml: jobs.assemble.env.TOKEN reads" \
  "$release_build" $'  assemble:\n    name: Assemble The Release Files\n' $'  assemble:\n    name: Assemble The Release Files\n    env:\n      TOKEN: ${{ secrets.NPM_TOKEN }}\n'
expect_fail "the update-feed key in a pull request workflow" "merge-checks.yaml: jobs.sneaky.env.KEY reads" \
  "$merge_checks" $'jobs:\n' $'jobs:\n  sneaky:\n    runs-on: ubuntu-latest\n    env:\n      KEY: ${{ secrets.UPDATE_FEED_SIGNING_KEY }}\n    steps:\n      - run: echo hi\n'
expect_pass_with "another workflow's own secret" \
  "$merge_checks" $'jobs:\n' $'jobs:\n  sneaky:\n    runs-on: ubuntu-latest\n    env:\n      TOKEN: ${{ secrets.OTHER_TOKEN }}\n    steps:\n      - run: echo hi\n'

echo "-- update-tool --"
expect_fail "update-tool with contents: write" "release.yaml update-tool must hold exactly" \
  "$release" "$tool_runner" $'    runs-on: ubuntu-latest\n    timeout-minutes: 40\n    permissions:\n      contents: write\n'
expect_fail "update-tool in the Environment" "job update-tool declares an environment" \
  "$release" "$tool_runner" $'    runs-on: ubuntu-latest\n    timeout-minutes: 40\n    environment: release\n    permissions:\n      contents: read\n'
expect_fail "update-tool with mise's cache" "release.yaml update-tool: mise-action must set cache: false" \
  "$release" $'          install_args: cargo-deny\n          cache: false\n' $'          install_args: cargo-deny\n'
expect_fail "update-tool that does not run the script" "release.yaml update-tool does not run .github/scripts/build-update-tools.sh" \
  "$release" 'run: bash .github/scripts/build-update-tools.sh' 'run: bash .github/scripts/other-tools.sh'
expect_fail "update-tool without the guard" "release.yaml update-tool must carry" \
  "$release" "$tool_header"$'    needs: plan\n'"$guard"$'\n' "$tool_header"$'    needs: plan\n'

echo "-- a file the check cannot read --"
expect_fail "a workflow in a syntax the check does not support" "cannot be read" \
  "$release" $'permissions:\n  contents: read\n' $'permissions: {contents: read}\n'

if [[ $failures -ne 0 ]]; then
  echo "${failures} release shape case(s) failed." >&2
  exit 1
fi
echo "All release shape cases passed."
