#!/usr/bin/env bash
# runs check-release-shape.mjs against copies of the workflows, first unchanged
# (it must pass) and then with one change at a time (each must fail, and name the
# cause). every change is one the release record forbids: the signing job's
# permissions, an action it uses, a checkout, a `${{ }}` inside run, a package
# manager or fetch or repository file, a tool called by name, a command that is
# quoted, expanded, a bare command substitution, run by a trap or by find, held
# in the test of a `[[ ]]` or hidden in an arithmetic expansion, or found through
# PATH, BASH_ENV, ENV, GITHUB_PATH or GITHUB_ENV (set or read into), a shell of
# its own, an artifact action's other inputs, the stand-in prefixes, pkgbuild's
# arguments; an Environment in another job, in a `.yml` workflow, or reachable
# from a pull request; a signing secret read outside sign-macos, in any workflow;
# a job without its guard, its runner or its place in the chain; a step condition
# that could skip a signing or verification step; a write to SHA256SUMS in the
# build's assemble job; and a way past a failed job, whatever the case of its
# status function. a change the test cannot find in the files fails the test, so
# it cannot go stale and pass.
# shellcheck disable=SC2016,SC1003 # the texts the changes find and replace are literal, not expansions.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository="$(cd "${here}/../../.." && pwd)"
check="${repository}/.github/scripts/check-release-shape.mjs"
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT

pristine="${work}/pristine"
mkdir -p "${pristine}/.github/scripts/tests"
cp -r "${repository}/.github/workflows" "${pristine}/.github/workflows"
cp "${repository}/.github/scripts/build-pkg.sh" "${pristine}/.github/scripts/build-pkg.sh"
cp "${repository}/.github/scripts/tests/test-signed-pkg.sh" "${pristine}/.github/scripts/tests/test-signed-pkg.sh"

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
build_pkg=.github/scripts/build-pkg.sh

expect_pass "the unchanged workflows"

# the lines the changes below are made at.
sign_header=$'    environment: release\n    permissions:\n      contents: read\n'
first_step=$'    steps:\n      - name: Download The Build\'s Release Files\n'
import_echo='          echo "Imported the Developer ID G2 certificate and both identities."'
create_keychain='"$SYSTEM_BIN/security" create-keychain'
package_flags=$'            --install-location / \\\n            --ownership recommended \\\n'

echo "-- sign-macos: what it holds --"
expect_fail "sign-macos with contents: write" "release.yaml sign-macos must hold exactly \`contents: read\`" \
  "$release" "$sign_header" $'    environment: release\n    permissions:\n      contents: write\n'
expect_fail "sign-macos with an extra permission" "release.yaml sign-macos must hold exactly \`contents: read\`" \
  "$release" "$sign_header" $'    environment: release\n    permissions:\n      contents: read\n      id-token: write\n'
expect_fail "sign-macos without permissions" "release.yaml sign-macos must hold exactly \`contents: read\`" \
  "$release" "$sign_header" $'    environment: release\n'
expect_fail "sign-macos without the Environment" "release.yaml sign-macos must declare \`environment: release\`" \
  "$release" "$sign_header" $'    permissions:\n      contents: read\n'
expect_fail "sign-macos in another Environment" "release.yaml sign-macos must declare \`environment: release\`" \
  "$release" "$sign_header" $'    environment: staging\n    permissions:\n      contents: read\n'
expect_fail "sign-macos on a self-hosted runner" "release.yaml sign-macos must run on macos-latest" \
  "$release" $'    runs-on: macos-latest\n    timeout-minutes: 60' $'    runs-on: self-hosted\n    timeout-minutes: 60'
expect_fail "sign-macos with a third-party action" "only actions/download-artifact and actions/upload-artifact are allowed" \
  "$release" $'        uses: actions/upload-artifact@v7\n        with:\n          name: marshal-macos-arm64-signed' $'        uses: someone/sign-action@v1\n        with:\n          name: marshal-macos-arm64-signed'
expect_fail "sign-macos with another actions/ action" "only actions/download-artifact and actions/upload-artifact are allowed" \
  "$release" $'        uses: actions/upload-artifact@v7\n        with:\n          name: marshal-macos-arm64-signed' $'        uses: actions/cache@v4\n        with:\n          name: marshal-macos-arm64-signed'
expect_fail "sign-macos with an action on a moving ref" "not a major tag" \
  "$release" $'        uses: actions/upload-artifact@v7\n        with:\n          name: marshal-macos-arm64-signed' $'        uses: actions/upload-artifact@main\n        with:\n          name: marshal-macos-arm64-signed'
expect_fail "sign-macos with a checkout" "checks out the repository" \
  "$release" "$first_step" $'    steps:\n      - name: Checkout\n        uses: actions/checkout@v7\n        with:\n          persist-credentials: false\n      - name: Download The Build\'s Release Files\n'
expect_fail "a \${{ }} inside sign-macos's run" "expands \${{ }} inside run" \
  "$release" $'          identifier="app.axross.marshal"\n          binary="signed/' $'          identifier="${{ vars.MACOS_TEAM_ID }}"\n          binary="signed/'

for manager in "npm ci" "npx prettier ." "brew install jq" "pip install requests" "cargo build" "mise install" "gem install bundler"; do
  expect_fail "sign-macos running ${manager%% *}" "runs the package manager ${manager%% *}" \
    "$release" "$import_echo" "          ${manager}"$'\n'"$import_echo"
done
for fetcher in "curl -O https://example.com/tool" "wget https://example.com/tool"; do
  expect_fail "sign-macos running ${fetcher%% *}" "runs ${fetcher%% *}; the job downloads nothing" \
    "$release" "$import_echo" "          ${fetcher}"$'\n'"$import_echo"
done
expect_fail "sign-macos running a repository script" "refers to the repository's files" \
  "$release" "$import_echo" $'          bash .github/scripts/build-pkg.sh a b c\n'"$import_echo"
expect_fail "sign-macos running bash" "which could run a file from the repository" \
  "$release" "$import_echo" $'          bash helper.sh\n'"$import_echo"
expect_fail "sign-macos running a file by its path" "which could run a file from the repository" \
  "$release" "$import_echo" $'          ./helper.sh\n'"$import_echo"
expect_fail "sign-macos running node" "which could run a file from the repository" \
  "$release" "$import_echo" $'          node -e 1\n'"$import_echo"
expect_fail "sign-macos running an unknown command" "neither a shell built-in, a base utility nor a tool at its system path" \
  "$release" "$import_echo" $'          jq . file.json\n'"$import_echo"
expect_fail "sign-macos calling security by name" "calls security by name, not by its system path" \
  "$release" "$create_keychain" 'security create-keychain'
expect_fail "sign-macos calling codesign through a literal path" "runs /usr/bin/codesign" \
  "$release" '"$SYSTEM_BIN/codesign" --force' '/usr/bin/codesign --force'
expect_fail "sign-macos calling a tool after a pipe by name" "calls openssl by name" \
  "$release" '"$SYSTEM_BIN/openssl" rand -hex 24)"' $'"$SYSTEM_BIN/openssl" rand -hex 24 | openssl base64)"'
expect_fail "sign-macos calling a tool in a command substitution by name" "calls shasum by name" \
  "$release" $'          line="$("$SYSTEM_BIN/shasum" -a 256 "${dir}/DeveloperIDG2CA.cer")"' $'          line="$(shasum -a 256 "${dir}/DeveloperIDG2CA.cer")"'
expect_fail "sign-macos using a tool the job does not list" "which is not one of" \
  "$release" "$import_echo" $'          "$SYSTEM_BIN/ruby" -e 1\n'"$import_echo"
expect_fail "sign-macos using a tool the check script does not check" "the check script does not check that" \
  "$release" 'bin_tools="security codesign pkgbuild openssl plutil shasum xcrun"' 'bin_tools="security codesign pkgbuild openssl plutil shasum"'
expect_fail "sign-macos using a tool the check script does not check in the system directory" "the check script does not check that pkgutil exists" \
  "$release" 'sbin_tools="pkgutil"' 'sbin_tools=""'
expect_fail "a sign-macos script the check cannot analyse" "cannot be analysed" \
  "$release" "$import_echo" $'          echo "an unterminated string\n'"$import_echo"
expect_fail "sign-macos without the xcrun lookups" "the check script does not look for notarytool with xcrun --find" \
  "$release" 'for tool in notarytool stapler; do' 'for tool in stapler; do'

echo "-- sign-macos: ways past the command analysis --"
# a quoted or expanded word is a command only as "$SYSTEM_BIN/<tool>".
expect_fail "sign-macos running a quoted command word" "runs a quoted or expanded word as a command" \
  "$release" "$import_echo" $'          "npm" ci\n'"$import_echo"
expect_fail "sign-macos running a single-quoted command word" "runs a quoted or expanded word as a command" \
  "$release" "$import_echo" $'          \'npm\' ci\n'"$import_echo"
expect_fail "sign-macos running a command held in a variable" "runs a quoted or expanded word as a command" \
  "$release" "$import_echo" $'          "$cmd" ci\n'"$import_echo"
expect_fail "sign-macos running an unquoted command held in a variable" "which is neither a shell built-in" \
  "$release" "$import_echo" $'          $cmd ci\n'"$import_echo"
expect_fail "sign-macos running the output of a command as the command" "runs a quoted command substitution as a command" \
  "$release" "$import_echo" $'          "$(printf npm)" ci\n'"$import_echo"
expect_fail "sign-macos running a tool through a path that leaves the system directory" "which is neither a shell built-in" \
  "$release" "$import_echo" $'          $SYSTEM_BIN/../../tmp/tool\n'"$import_echo"
expect_fail "sign-macos running a quoted tool through a path that leaves the system directory" "runs a quoted or expanded word as a command" \
  "$release" "$import_echo" $'          "$SYSTEM_BIN/../../tmp/tool"\n'"$import_echo"
expect_fail "sign-macos using a backtick substitution" "cannot be analysed: a backtick command substitution" \
  "$release" "$import_echo" $'          echo `npm ci`\n'"$import_echo"
# the shell runs a command substitution inside the test of a `[[ ]]`.
expect_fail "sign-macos with a fetch inside a [[ ]] test" "runs curl; the job downloads nothing" \
  "$release" "$import_echo" $'          [[ -z "$(curl -s x)" ]] || true\n'"$import_echo"
expect_fail "sign-macos with a package manager inside an if [[ ]] test" "runs the package manager npm" \
  "$release" "$import_echo" $'          if [[ "$(npm view x)" == y ]]; then :; fi\n'"$import_echo"
expect_fail "sign-macos with an unquoted command substitution as a [[ ]] test" "runs the package manager npm" \
  "$release" "$import_echo" $'          [[ $(npm ci) ]]\n'"$import_echo"
expect_pass_with "sign-macos with a harmless command substitution inside a [[ ]] test" \
  "$release" "$import_echo" $'          [[ -n "$(echo x)" ]] || true\n'"$import_echo"
# a command substitution that stands where the command word would runs what it prints.
expect_fail "sign-macos running a bare command substitution as the command" "runs a command substitution as a command" \
  "$release" "$import_echo" $'          $(printf npm)\n'"$import_echo"
expect_fail "sign-macos running a bare command substitution with a quoted argument" "runs a command substitution as a command" \
  "$release" "$import_echo" $'          $(printf "npm ci")\n'"$import_echo"
expect_fail "sign-macos running a command substitution after an assignment" "runs a command substitution as a command" \
  "$release" "$import_echo" $'          x=1 $(printf npm)\n'"$import_echo"
expect_fail "sign-macos running a command substitution after a semicolon" "runs a command substitution as a command" \
  "$release" "$import_echo" $'          true; $(printf npm)\n'"$import_echo"
expect_fail "sign-macos running a command substitution after then" "runs a command substitution as a command" \
  "$release" "$import_echo" $'          if true; then $(printf npm); fi\n'"$import_echo"
expect_fail "sign-macos running a command substitution after an assignment of one" "runs a command substitution as a command" \
  "$release" "$import_echo" $'          x=$(true) $(printf npm)\n'"$import_echo"
expect_pass_with "sign-macos assigning two adjacent command substitutions" \
  "$release" "$import_echo" $'          x=$(true)$(true)\n'"$import_echo"
# the body of a trap runs as commands.
expect_fail "sign-macos with a trap that runs a package manager" "runs the package manager npm" \
  "$release" $'          trap \'rm -f "$key_file"\' EXIT' $'          trap \'npm ci\' EXIT'
expect_fail "sign-macos with a double-quoted trap that calls a tool by name" "calls security by name" \
  "$release" $'          trap \'rm -f "$key_file"\' EXIT' $'          trap "security delete-keychain x" EXIT'
expect_fail "sign-macos with a trap whose body is a variable" "which is neither a shell built-in" \
  "$release" $'          trap \'rm -f "$key_file"\' EXIT' $'          trap "$handler" EXIT'
expect_fail "sign-macos with a trap that names no function of the script" "sets a trap to handler" \
  "$release" $'          trap \'rm -f "$key_file"\' EXIT' $'          trap handler EXIT'
# find may list files, not run commands.
expect_fail "sign-macos with find -exec" "runs find with -exec" \
  "$release" $'find unsigned -mindepth 1 -maxdepth 1 |' $'find unsigned -mindepth 1 -maxdepth 1 -exec npm ci {} \';\' |'
expect_fail "sign-macos with find -execdir" "runs find with -execdir" \
  "$release" $'find unsigned -mindepth 1 -maxdepth 1 |' $'find unsigned -mindepth 1 -maxdepth 1 -execdir npm ci {} \';\' |'
expect_fail "sign-macos with find -exec quoted" "runs find with -exec" \
  "$release" $'find unsigned -mindepth 1 -maxdepth 1 |' $'find unsigned -mindepth 1 -maxdepth 1 "-exec" npm ci {} \';\' |'
expect_fail "sign-macos with find -exec spelled in two quoted parts" "runs find with -e" \
  "$release" $'find unsigned -mindepth 1 -maxdepth 1 |' $'find unsigned -mindepth 1 -maxdepth 1 -e"xec" npm ci {} \';\' |'
expect_fail "sign-macos with find -delete" "runs find with -delete" \
  "$release" $'find unsigned -mindepth 1 -maxdepth 1 |' $'find unsigned -mindepth 1 -maxdepth 1 -delete |'
# an arithmetic expansion is not read as commands, so it may not hold one.
expect_fail "sign-macos with a command substitution inside an arithmetic expansion" "cannot be analysed: an arithmetic expansion that contains a command" \
  "$release" "$import_echo" $'          count=$(( $(npm ci) + 1 ))\n'"$import_echo"
expect_fail "sign-macos with a backtick inside an arithmetic expansion" "cannot be analysed: an arithmetic expansion that contains a command" \
  "$release" "$import_echo" $'          count=$(( `npm ci` + 1 ))\n'"$import_echo"
# a command that names PATH, BASH_ENV or ENV can assign it without a `NAME=`.
for naming in "read -r PATH" "read -r first BASH_ENV" "declare -n ENV" "typeset BASH_ENV" "local PATH" "export PATH" "readonly BASH_ENV" "for PATH in /tmp/fake; do :; done"; do
  expect_fail "sign-macos with ${naming}" "which could assign it" \
    "$release" "$import_echo" "          ${naming}"$'\n'"$import_echo"
done
expect_fail "sign-macos with printf -v naming PATH" "names PATH to printf -v" \
  "$release" "$import_echo" $'          printf -v PATH %s /tmp/fake\n'"$import_echo"
# a shell of the job's own could run a program or read a file first.
expect_fail "a sign-macos step with a shell" "step \"Sign The Binary\" sets shell" \
  "$release" $'      - name: Sign The Binary\n' $'      - name: Sign The Binary\n        shell: bash -e {0}\n'
expect_fail "sign-macos with a default shell" "sign-macos sets defaults.run.shell" \
  "$release" $'    timeout-minutes: 60\n    environment: release' $'    timeout-minutes: 60\n    defaults:\n      run:\n        shell: bash -e {0}\n    environment: release'
expect_fail "a workflow with a default shell" "the workflow sets defaults.run.shell" \
  "$release" $'permissions:\n  contents: read\n' $'permissions:\n  contents: read\n\ndefaults:\n  run:\n    shell: bash -e {0}\n'
# the artifact actions may take only the inputs the job uses.
expect_fail "sign-macos downloading from another repository" "gives actions/download-artifact the input repository" \
  "$release" $'          name: ${{ needs.build.outputs.artifact }}\n          path: unsigned\n' $'          name: ${{ needs.build.outputs.artifact }}\n          path: unsigned\n          repository: someone/else\n'
expect_fail "sign-macos downloading with a token" "gives actions/download-artifact the input github-token" \
  "$release" $'          name: ${{ needs.build.outputs.artifact }}\n          path: unsigned\n' $'          name: ${{ needs.build.outputs.artifact }}\n          path: unsigned\n          github-token: ${{ github.token }}\n'
expect_fail "sign-macos downloading another run's artifact" "gives actions/download-artifact the input run-id" \
  "$release" $'          name: ${{ needs.build.outputs.artifact }}\n          path: unsigned\n' $'          name: ${{ needs.build.outputs.artifact }}\n          path: unsigned\n          run-id: 1\n'
expect_fail "sign-macos uploading with an extra input" "gives actions/upload-artifact the input overwrite" \
  "$release" $'          path: signed\n          retention-days: 14\n' $'          path: signed\n          overwrite: true\n          retention-days: 14\n'
# nothing may change where commands are found, or what the shell runs first.
expect_fail "a step of sign-macos that sets PATH in its env" "sets PATH, BASH_ENV or ENV" \
  "$release" $'        env:\n          VERSION: ${{ needs.plan.outputs.version }}\n        run: |\n          # sign-macos: sign' $'        env:\n          VERSION: ${{ needs.plan.outputs.version }}\n          PATH: /tmp/fake\n        run: |\n          # sign-macos: sign'
expect_fail "a step of sign-macos that sets BASH_ENV in its env" "sets PATH, BASH_ENV or ENV" \
  "$release" $'        env:\n          VERSION: ${{ needs.plan.outputs.version }}\n        run: |\n          # sign-macos: sign' $'        env:\n          VERSION: ${{ needs.plan.outputs.version }}\n          BASH_ENV: /tmp/fake.sh\n        run: |\n          # sign-macos: sign'
expect_fail "sign-macos that sets PATH in its env" "sets PATH, BASH_ENV or ENV" \
  "$release" $'    timeout-minutes: 60\n    environment: release' $'    timeout-minutes: 60\n    env:\n      PATH: /tmp/fake\n    environment: release'
expect_fail "sign-macos that sets BASH_ENV in its env" "sets PATH, BASH_ENV or ENV" \
  "$release" $'    timeout-minutes: 60\n    environment: release' $'    timeout-minutes: 60\n    env:\n      BASH_ENV: /tmp/fake.sh\n    environment: release'
expect_fail "a step of sign-macos that sets ENV in its env" "sets PATH, BASH_ENV or ENV" \
  "$release" $'        env:\n          VERSION: ${{ needs.plan.outputs.version }}\n        run: |\n          # sign-macos: sign' $'        env:\n          VERSION: ${{ needs.plan.outputs.version }}\n          ENV: /tmp/fake.sh\n        run: |\n          # sign-macos: sign'
expect_fail "a workflow that sets ENV in its env" "sets PATH, BASH_ENV or ENV" \
  "$release" $'permissions:\n  contents: read\n' $'permissions:\n  contents: read\n\nenv:\n  ENV: /tmp/fake.sh\n'
expect_fail "a workflow that sets PATH in its env" "sets PATH, BASH_ENV or ENV" \
  "$release" $'permissions:\n  contents: read\n' $'permissions:\n  contents: read\n\nenv:\n  PATH: /tmp/fake\n'
expect_fail "a script that assigns PATH" "assigns PATH" \
  "$release" "$import_echo" $'          PATH="/tmp/fake:${PATH}"\n'"$import_echo"
expect_fail "a script that exports BASH_ENV" "assigns BASH_ENV" \
  "$release" "$import_echo" $'          export BASH_ENV=/tmp/fake.sh\n'"$import_echo"
expect_fail "a script that writes to GITHUB_PATH" "refers to GITHUB_PATH" \
  "$release" "$import_echo" $'          echo "/tmp/fake" >>"$GITHUB_PATH"\n'"$import_echo"
expect_fail "a script that writes PATH to GITHUB_ENV" "refers to GITHUB_ENV other than" \
  "$release" "$import_echo" $'          echo "PATH=/tmp/fake" >>"$GITHUB_ENV"\n'"$import_echo"
expect_fail "the identities step that writes a third name to GITHUB_ENV" "refers to GITHUB_ENV other than" \
  "$release" $'            echo "INSTALLER_IDENTITY_NAME=${installer_name}"\n' $'            echo "INSTALLER_IDENTITY_NAME=${installer_name}"\n            echo "PATH=${installer_name}"\n'
expect_fail "the identities step that writes to GITHUB_ENV again" "writes GITHUB_ENV outside its block" \
  "$release" $'          echo "team_id=${MACOS_TEAM_ID}" >>"$GITHUB_OUTPUT"' $'          echo "PATH=/tmp/fake" >>"$GITHUB_ENV"\n          echo "team_id=${MACOS_TEAM_ID}" >>"$GITHUB_OUTPUT"'

echo "-- sign-macos: the stand-in prefixes --"
expect_fail "a prefix default other than /usr/bin" "uses SYSTEM_BIN without the line" \
  "$release" $'          # sign-macos: sign\n          set -euo pipefail\n          : "${SYSTEM_BIN:=/usr/bin}"' $'          # sign-macos: sign\n          set -euo pipefail\n          : "${SYSTEM_BIN:=/opt/homebrew/bin}"'
expect_fail "a prefix without its default line" "uses SYSTEM_BIN without the line" \
  "$release" $'          # sign-macos: hash\n          set -euo pipefail\n          : "${SYSTEM_BIN:=/usr/bin}"\n' $'          # sign-macos: hash\n          set -euo pipefail\n'
expect_fail "a prefix default of /usr/sbin changed" "uses SYSTEM_SBIN without the line" \
  "$release" ': "${SYSTEM_SBIN:=/usr/sbin}"' ': "${SYSTEM_SBIN:=/sbin}"'
expect_fail "a step that sets SYSTEM_BIN in its env" "sets SYSTEM_BIN or SYSTEM_SBIN; only the stand-in test may" \
  "$release" $'        env:\n          VERSION: ${{ needs.plan.outputs.version }}\n        run: |\n          # sign-macos: sign' $'        env:\n          VERSION: ${{ needs.plan.outputs.version }}\n          SYSTEM_BIN: /tmp/fake\n        run: |\n          # sign-macos: sign'
expect_fail "a job that sets SYSTEM_SBIN in its env" "sets SYSTEM_BIN or SYSTEM_SBIN; only the stand-in test may" \
  "$release" $'    timeout-minutes: 60\n    environment: release' $'    timeout-minutes: 60\n    env:\n      SYSTEM_SBIN: /tmp/fake\n    environment: release'
expect_fail "a workflow that sets SYSTEM_BIN in its env" "sets SYSTEM_BIN or SYSTEM_SBIN; only the stand-in test may" \
  "$release" $'permissions:\n  contents: read\n' $'permissions:\n  contents: read\n\nenv:\n  SYSTEM_BIN: /tmp/fake\n'
expect_fail "a script that assigns SYSTEM_BIN" "assigns SYSTEM_BIN" \
  "$release" "$import_echo" $'          SYSTEM_BIN=/tmp/fake\n'"$import_echo"
expect_fail "a script that writes SYSTEM_SBIN to GITHUB_ENV" "assigns SYSTEM_SBIN" \
  "$release" "$import_echo" $'          echo "SYSTEM_SBIN=/tmp/fake" >>"$GITHUB_ENV"\n'"$import_echo"
expect_fail "a script without its marker line" "does not start with a \"# sign-macos: <name>\" marker line" \
  "$release" $'          # sign-macos: hash\n' $'          # hash\n'

echo "-- sign-macos: pkgbuild's arguments --"
expect_fail "pkgbuild without --ownership" "pkgbuild's arguments differ from build-pkg.sh's" \
  "$release" "$package_flags" $'            --install-location / \\\n'
expect_fail "pkgbuild with another install location" "pkgbuild's arguments differ from build-pkg.sh's" \
  "$release" $'            --install-location / \\\n' $'            --install-location /usr/local \\\n'
expect_fail "pkgbuild with another identifier variable" "pkgbuild's arguments differ from build-pkg.sh's" \
  "$release" $'            --identifier "$identifier" \\\n            --version "$version"' $'            --identifier "other" \\\n            --version "$version"'
expect_fail "pkgbuild with an added scripts directory" "pkgbuild's arguments differ from build-pkg.sh's" \
  "$release" "$package_flags" $'            --install-location / \\\n            --ownership recommended \\\n            --scripts scripts \\\n'
expect_fail "pkgbuild without --sign" "must pass --sign and --keychain to pkgbuild once each" \
  "$release" $'            --sign "$INSTALLER_IDENTITY_NAME" \\\n' ''
expect_fail "pkgbuild without --keychain" "must pass --sign and --keychain to pkgbuild once each" \
  "$release" $'            --keychain "$keychain" \\\n            "$output"' $'            "$output"'
expect_fail "staging with another mode" "the package script's staging lines differ from build-pkg.sh's" \
  "$release" 'chmod 0755 "$stage" "${stage}/usr"' 'chmod 0777 "$stage" "${stage}/usr"'
expect_fail "staging a different path" "the package script's staging lines differ from build-pkg.sh's" \
  "$release" $'cp "$binary" "${stage}/usr/local/bin/marshal"' $'cp "$binary" "${stage}/usr/local/bin/marshal-cli"'
expect_fail "an identifier that is not build-pkg.sh's" "sets identifier" \
  "$release" $'          identifier="app.axross.marshal"\n          binary="signed/' $'          identifier="app.axross.other"\n          binary="signed/'
expect_fail "build-pkg.sh with another identifier" "sets identifier" \
  "$build_pkg" 'identifier="app.axross.marshal"' 'identifier="app.axross.changed"'
expect_fail "build-pkg.sh with another install location" "pkgbuild's arguments differ from build-pkg.sh's" \
  "$build_pkg" '--install-location / \' '--install-location /opt \'
expect_fail "build-pkg.sh with another staging mode" "the package script's staging lines differ from build-pkg.sh's" \
  "$build_pkg" 'chmod 0755 "$stage"' 'chmod 0750 "$stage"'

echo "-- the signing secrets --"
expect_fail "a signing secret in attest" "only sign-macos may read a signing secret" \
  "$release" $'  attest:\n    name: Attest The Build Provenance\n' $'  attest:\n    name: Attest The Build Provenance\n    env:\n      PASSWORD: ${{ secrets.MACOS_P12_PASSWORD }}\n'
expect_fail "a signing secret in verify-macos's step" "release.yaml: jobs.verify-macos.steps[5].env.P8 reads" \
  "$release" $'      - name: Test The Signed Package\n        timeout-minutes: 10\n        env:\n' $'      - name: Test The Signed Package\n        timeout-minutes: 10\n        env:\n          P8: ${{ secrets.MACOS_NOTARY_KEY_P8 }}\n'
expect_fail "a signing secret in the workflow's env" "release.yaml: env.P12 reads" \
  "$release" $'permissions:\n  contents: read\n' $'permissions:\n  contents: read\n\nenv:\n  P12: ${{ secrets.MACOS_INSTALLER_P12_BASE64 }}\n'
expect_fail "a signing secret read by bracket" "only sign-macos may read a signing secret" \
  "$release" $'  attest:\n    name: Attest The Build Provenance\n' $'  attest:\n    name: Attest The Build Provenance\n    env:\n      PASSWORD: ${{ secrets[\'MACOS_P12_PASSWORD\'] }}\n'
expect_fail "every secret read through toJSON" "only sign-macos may read a signing secret" \
  "$release" $'  attest:\n    name: Attest The Build Provenance\n' $'  attest:\n    name: Attest The Build Provenance\n    env:\n      ALL: ${{ toJSON(secrets) }}\n'
expect_fail "secrets: inherit on the build job" "release.yaml: jobs.build.secrets passes every secret on" \
  "$release" $'    uses: ./.github/workflows/release-build.yaml\n' $'    uses: ./.github/workflows/release-build.yaml\n    secrets: inherit\n'
expect_fail "a signing secret in a pull request workflow" "merge-checks.yaml: jobs.sneaky.env.PASSWORD reads" \
  "$merge_checks" $'jobs:\n' $'jobs:\n  sneaky:\n    runs-on: ubuntu-latest\n    env:\n      PASSWORD: ${{ secrets.MACOS_P12_PASSWORD }}\n    steps:\n      - run: echo hi\n'
expect_fail_new_file "every secret passed on in a .yml workflow" "extra.yml: jobs.sneaky.secrets passes every secret on" \
  .github/workflows/extra.yml $'name: Extra\non:\n  workflow_dispatch:\njobs:\n  sneaky:\n    uses: ./.github/workflows/release-build.yaml\n    secrets: inherit\n'
expect_fail "a signing secret in release-build.yaml" "release-build.yaml: jobs.assemble" \
  "$release_build" $'  assemble:\n    name: Assemble The Release Files\n' $'  assemble:\n    name: Assemble The Release Files\n    env:\n      PASSWORD: ${{ secrets.MACOS_P12_PASSWORD }}\n'

echo "-- the Environment elsewhere --"
expect_fail "an Environment on attest" "job attest declares an environment" \
  "$release" $'  attest:\n    name: Attest The Build Provenance\n' $'  attest:\n    name: Attest The Build Provenance\n    environment: release\n'
expect_fail "an Environment on release" "job release declares an environment" \
  "$release" $'  release:\n    name: Release\n' $'  release:\n    name: Release\n    environment: release\n'
expect_fail "an Environment on verify-macos" "job verify-macos declares an environment" \
  "$release" $'  verify-macos:\n    name: Verify The Signed macOS Files\n' $'  verify-macos:\n    name: Verify The Signed macOS Files\n    environment: release\n'
expect_fail "an Environment on assemble-signed" "job assemble-signed declares an environment" \
  "$release" $'  assemble-signed:\n    name: Assemble The Signed Release Files\n' $'  assemble-signed:\n    name: Assemble The Signed Release Files\n    environment: release\n'
expect_fail "an Environment on the plan job" "job plan declares an environment" \
  "$release" $'  plan:\n    name: Plan The Release\n' $'  plan:\n    name: Plan The Release\n    environment: release\n'
expect_fail "an Environment in release-build.yaml" "release-build.yaml: job assemble declares an environment" \
  "$release_build" $'  assemble:\n    name: Assemble The Release Files\n' $'  assemble:\n    name: Assemble The Release Files\n    environment: release\n'
expect_fail "an Environment in merge-checks.yaml" "merge-checks.yaml: job" \
  "$merge_checks" $'jobs:\n' $'jobs:\n  sneaky:\n    runs-on: ubuntu-latest\n    environment: release\n    steps:\n      - run: echo hi\n'
expect_fail "an Environment in a step" "a step of job" \
  "$release" $'      - name: Download The Notary Log\n' $'      - name: Download The Notary Log\n        environment: release\n'
expect_fail_new_file "an Environment in a .yml workflow that pull requests trigger" "extra.yml: job sneaky declares an environment" \
  .github/workflows/extra.yml $'name: Extra\non:\n  pull_request:\njobs:\n  sneaky:\n    runs-on: ubuntu-latest\n    environment: release\n    steps:\n      - run: echo hi\n'
expect_fail_new_file "a .yml pull request workflow that calls release.yaml" "extra.yml: a pull request triggers job sneaky" \
  .github/workflows/extra.yml $'name: Extra\non:\n  pull_request:\njobs:\n  sneaky:\n    uses: ./.github/workflows/release.yaml\n'
expect_fail "a pull request workflow that calls a workflow it cannot be checked against" "which this check cannot follow" \
  "$merge_checks" $'jobs:\n' $'jobs:\n  sneaky:\n    uses: someone/else/.github/workflows/sign.yaml@main\n'
expect_fail "a pull request workflow that calls release.yaml" "a workflow that declares an environment" \
  "$merge_checks" $'jobs:\n' $'jobs:\n  sneaky:\n    uses: ./.github/workflows/release.yaml\n'
expect_fail "release.yaml triggered by pull requests" "release.yaml is triggered by" \
  "$release" $'on:\n  workflow_dispatch:\n' $'on:\n  workflow_dispatch:\n  pull_request:\n'
expect_fail "a called workflow that declares an Environment" "release-build.yaml: job build declares an environment" \
  "$release_build" $'  build:\n    name: Build ${{ matrix.target }}\n' $'  build:\n    name: Build ${{ matrix.target }}\n    environment: release\n'

echo "-- the other new jobs --"
expect_fail "verify-macos with contents: write" "release.yaml verify-macos must hold exactly" \
  "$release" $'    timeout-minutes: 20\n    permissions:\n      contents: read\n' $'    timeout-minutes: 20\n    permissions:\n      contents: write\n'
expect_fail "assemble-signed with an extra permission" "release.yaml assemble-signed must hold exactly" \
  "$release" $'    timeout-minutes: 10\n    permissions:\n      contents: read\n    outputs:\n      artifact: marshal-release-signed' $'    timeout-minutes: 10\n    permissions:\n      contents: read\n      id-token: write\n    outputs:\n      artifact: marshal-release-signed'
expect_fail "verify-macos with a checkout that keeps credentials" "a checkout does not set persist-credentials: false" \
  "$release" $'  verify-macos:\n    name: Verify The Signed macOS Files\n    needs: [plan, build, sign-macos]\n    if: github.ref == \'refs/heads/main\' && needs.plan.outputs.version != \'\'\n    runs-on: macos-latest\n    timeout-minutes: 20\n    permissions:\n      contents: read\n    steps:\n      - name: Checkout\n        uses: actions/checkout@v7\n        with:\n          persist-credentials: false\n' $'  verify-macos:\n    name: Verify The Signed macOS Files\n    needs: [plan, build, sign-macos]\n    if: github.ref == \'refs/heads/main\' && needs.plan.outputs.version != \'\'\n    runs-on: macos-latest\n    timeout-minutes: 20\n    permissions:\n      contents: read\n    steps:\n      - name: Checkout\n        uses: actions/checkout@v7\n'
expect_fail "release-build's assemble with a checkout that keeps credentials" "release-build.yaml assemble: a checkout does not set persist-credentials: false" \
  "$release_build" $'    steps:\n      - name: Checkout\n        uses: actions/checkout@v7\n        with:\n          persist-credentials: false\n      - name: Download The Windows Files' $'    steps:\n      - name: Checkout\n        uses: actions/checkout@v7\n      - name: Download The Windows Files'
expect_fail "release-build's assemble writing SHA256SUMS itself" "assemble-release.sh is the only writer" \
  "$release_build" $'        run: bash .github/scripts/assemble-release.sh "$VERSION" dist "$WINDOWS_SHA256" "$WINDOWS_MSI_SHA256" "$MACOS_SHA256" "$MACOS_PKG_SHA256"' $'        run: |\n          bash .github/scripts/assemble-release.sh "$VERSION" dist "$WINDOWS_SHA256" "$WINDOWS_MSI_SHA256" "$MACOS_SHA256" "$MACOS_PKG_SHA256"\n          sha256sum dist/* > dist/SHA256SUMS'
assemble_call='bash .github/scripts/assemble-release.sh "$VERSION" dist "$WINDOWS_SHA256" "$WINDOWS_MSI_SHA256" "$MACOS_SHA256" "$MACOS_PKG_SHA256"'
for writing in 'sha256sum dist/* >> dist/SHA256SUMS' 'sha256sum dist/* | tee dist/SHA256SUMS' 'cp sums.txt dist/SHA256SUMS' 'mv sums.txt "dist/SHA256SUMS"'; do
  expect_fail "release-build's assemble writing SHA256SUMS with ${writing}" "assemble-release.sh is the only writer" \
    "$release_build" "        run: ${assemble_call}" "        run: |"$'\n'"          ${assemble_call}"$'\n'"          ${writing}"
done
for reading in 'cat dist/SHA256SUMS' 'sha256sum -c dist/SHA256SUMS' 'cp dist/SHA256SUMS sums.txt' 'mv dist/SHA256SUMS dist/SHA256SUMS.old'; do
  expect_pass_with "release-build's assemble reading SHA256SUMS with ${reading}" \
    "$release_build" "        run: ${assemble_call}" "        run: |"$'\n'"          ${assemble_call}"$'\n'"          ${reading}"
done
expect_fail "sign-macos with a timeout beyond 60 minutes" "release.yaml sign-macos must set timeout-minutes to at most 60" \
  "$release" $'    runs-on: macos-latest\n    timeout-minutes: 60' $'    runs-on: macos-latest\n    timeout-minutes: 61'
expect_fail "verify-macos without a timeout" "release.yaml verify-macos must set timeout-minutes to at most 20" \
  "$release" $'    timeout-minutes: 20\n    permissions:' $'    permissions:'
expect_fail "assemble-signed with a timeout beyond 10 minutes" "release.yaml assemble-signed must set timeout-minutes to at most 10" \
  "$release" $'    runs-on: ubuntu-latest\n    timeout-minutes: 10\n    permissions:\n      contents: read\n    outputs:\n      artifact: marshal-release-signed' $'    runs-on: ubuntu-latest\n    timeout-minutes: 30\n    permissions:\n      contents: read\n    outputs:\n      artifact: marshal-release-signed'

expect_fail "verify-macos on a self-hosted runner" "release.yaml verify-macos must run on macos-latest" \
  "$release" $'    runs-on: macos-latest\n    timeout-minutes: 20' $'    runs-on: self-hosted\n    timeout-minutes: 20'
expect_fail "assemble-signed on a self-hosted runner" "release.yaml assemble-signed must run on ubuntu-latest" \
  "$release" $'    runs-on: ubuntu-latest\n    timeout-minutes: 10\n    permissions:\n      contents: read\n    outputs:\n      artifact: marshal-release-signed' $'    runs-on: [self-hosted, linux]\n    timeout-minutes: 10\n    permissions:\n      contents: read\n    outputs:\n      artifact: marshal-release-signed'

echo "-- the chain and the guard --"
guard="    if: github.ref == 'refs/heads/main' && needs.plan.outputs.version != ''"
expect_fail "sign-macos without the guard" "release.yaml sign-macos must carry" \
  "$release" $'    needs: [plan, build]\n'"$guard"$'\n    runs-on: macos-latest' $'    needs: [plan, build]\n    runs-on: macos-latest'
expect_fail "sign-macos with a weaker guard" "release.yaml sign-macos must carry" \
  "$release" $'    needs: [plan, build]\n'"$guard"$'\n    runs-on: macos-latest' $'    needs: [plan, build]\n    if: github.ref == \'refs/heads/main\'\n    runs-on: macos-latest'
expect_fail "verify-macos with a guard on another branch" "release.yaml verify-macos must carry" \
  "$release" $'    needs: [plan, build, sign-macos]\n'"$guard" $'    needs: [plan, build, sign-macos]\n    if: github.ref == \'refs/heads/dev\' && needs.plan.outputs.version != \'\''
expect_fail "attest without the guard" "release.yaml attest must carry" \
  "$release" $'    needs: [plan, assemble-signed]\n'"$guard"$'\n' $'    needs: [plan, assemble-signed]\n'
expect_fail "attest with a weaker guard" "release.yaml attest must carry" \
  "$release" $'    needs: [plan, assemble-signed]\n'"$guard" $'    needs: [plan, assemble-signed]\n    if: github.ref == \'refs/heads/main\''
expect_fail "release without the version in its guard" "release.yaml release must carry" \
  "$release" $'    needs: [plan, attest]\n'"$guard" $'    needs: [plan, attest]\n    if: github.ref == \'refs/heads/main\' && needs.plan.outputs.version != \'x\''
expect_fail "assemble-signed without plan in needs" "release.yaml assemble-signed (every new job) must need plan directly" \
  "$release" $'    needs: [plan, build, sign-macos, verify-macos]' $'    needs: [build, sign-macos, verify-macos]'
expect_fail "sign-macos without plan in needs" "release.yaml sign-macos (every new job) must need plan directly" \
  "$release" $'  sign-macos:\n    name: Sign And Notarize The macOS Files\n    needs: [plan, build]' $'  sign-macos:\n    name: Sign And Notarize The macOS Files\n    needs: [build]'
expect_fail "verify-macos not after sign-macos" "release.yaml verify-macos must need sign-macos directly" \
  "$release" $'    needs: [plan, build, sign-macos]' $'    needs: [plan, build]'
expect_fail "assemble-signed not after verify-macos" "release.yaml assemble-signed must need verify-macos directly" \
  "$release" $'    needs: [plan, build, sign-macos, verify-macos]' $'    needs: [plan, build, sign-macos]'
expect_fail "attest not after assemble-signed" "release.yaml attest must need assemble-signed directly" \
  "$release" $'    needs: [plan, assemble-signed]' $'    needs: [plan, build]'
expect_fail "release not after attest" "release.yaml release must need attest directly" \
  "$release" $'    needs: [plan, attest]' $'    needs: [plan, build]'
expect_fail "attest reading the unsigned set" "release.yaml attest must download exactly marshal-release-signed" \
  "$release" $'  attest:\n    name: Attest The Build Provenance\n    needs: [plan, assemble-signed]\n    if: github.ref == \'refs/heads/main\' && needs.plan.outputs.version != \'\'\n    runs-on: ubuntu-latest\n    timeout-minutes: 10\n    permissions:\n      id-token: write\n      attestations: write\n    steps:\n      - name: Download The Release Files\n        uses: actions/download-artifact@v8\n        with:\n          name: marshal-release-signed' $'  attest:\n    name: Attest The Build Provenance\n    needs: [plan, assemble-signed]\n    if: github.ref == \'refs/heads/main\' && needs.plan.outputs.version != \'\'\n    runs-on: ubuntu-latest\n    timeout-minutes: 10\n    permissions:\n      id-token: write\n      attestations: write\n    steps:\n      - name: Download The Release Files\n        uses: actions/download-artifact@v8\n        with:\n          name: marshal-release'
expect_fail "release reading the unsigned set" "release.yaml release must download exactly marshal-release-signed" \
  "$release" $'      - name: Download The Release Files\n        uses: actions/download-artifact@v8\n        with:\n          name: marshal-release-signed\n          path: release-files\n      - name: Release' $'      - name: Download The Release Files\n        uses: actions/download-artifact@v8\n        with:\n          name: ${{ needs.build.outputs.artifact }}\n          path: release-files\n      - name: Release'
expect_fail "attest with a further permission" "release.yaml attest must hold exactly" \
  "$release" $'      id-token: write\n      attestations: write\n' $'      id-token: write\n      attestations: write\n      contents: write\n'

echo "-- no way past a failed job --"
expect_fail "continue-on-error on sign-macos" "release.yaml sign-macos sets continue-on-error" \
  "$release" $'    timeout-minutes: 60\n    environment: release' $'    timeout-minutes: 60\n    continue-on-error: true\n    environment: release'
expect_fail "continue-on-error on release" "release.yaml release sets continue-on-error" \
  "$release" $'  release:\n    name: Release\n' $'  release:\n    name: Release\n    continue-on-error: true\n'
expect_fail "continue-on-error on a step" "sets continue-on-error" \
  "$release" $'      - name: Download The Notary Log\n' $'      - name: Download The Notary Log\n        continue-on-error: true\n'
expect_fail "always() in assemble-signed's condition" "has a job-level condition that can run it after a failure" \
  "$release" $'    needs: [plan, build, sign-macos, verify-macos]\n'"$guard" $'    needs: [plan, build, sign-macos, verify-macos]\n    if: always() && needs.plan.outputs.version != \'\''
expect_fail "!cancelled() in attest's condition" "has a job-level condition that can run it after a failure" \
  "$release" $'    needs: [plan, assemble-signed]\n'"$guard" $'    needs: [plan, assemble-signed]\n    if: ${{ !cancelled() }}'
expect_fail "failure() in release's condition" "has a job-level condition that can run it after a failure" \
  "$release" $'    needs: [plan, attest]\n'"$guard" $'    needs: [plan, attest]\n    if: failure()'
expect_fail "always() on a signing step" "is allowed only on the clean-up and notary-log upload steps" \
  "$release" $'      - name: Upload The Signed Files\n        uses: actions/upload-artifact@v7' $'      - name: Upload The Signed Files\n        if: always()\n        uses: actions/upload-artifact@v7'
expect_fail "failure() on the sign step" "is allowed only on the clean-up and notary-log upload steps" \
  "$release" $'      - name: Sign The Binary\n' $'      - name: Sign The Binary\n        if: failure()\n'
expect_fail "always() on a step of another job" "can run after a failure" \
  "$release" $'      - name: Download The Notary Log\n' $'      - name: Download The Notary Log\n        if: always()\n'
# a condition that skips a step also skips a check.
expect_fail "if: false on verify-macos's test of the signed package" "release.yaml verify-macos: step \"Test The Signed Package\" has a condition" \
  "$release" $'      - name: Test The Signed Package\n        timeout-minutes: 10\n' $'      - name: Test The Signed Package\n        if: false\n        timeout-minutes: 10\n'
expect_fail "a condition on a step of assemble-signed" "release.yaml assemble-signed: step \"Check The Hashes And Write SHA256SUMS\" has a condition" \
  "$release" $'      - name: Check The Hashes And Write SHA256SUMS\n        env:\n          VERSION: ${{ needs.plan.outputs.version }}\n          MACOS_SHA256' $'      - name: Check The Hashes And Write SHA256SUMS\n        if: github.ref == \'refs/heads/main\'\n        env:\n          VERSION: ${{ needs.plan.outputs.version }}\n          MACOS_SHA256'
expect_fail "a condition on sign-macos's Notarize step" "is allowed only on the clean-up and notary-log upload steps" \
  "$release" $'      - name: Notarize The Package\n' $'      - name: Notarize The Package\n        if: false\n'
expect_fail "a condition on sign-macos's Staple step" "is allowed only on the clean-up and notary-log upload steps" \
  "$release" $'      - name: Staple And Check The Package\n' $'      - name: Staple And Check The Package\n        if: success()\n'
expect_fail "a condition on sign-macos's Sign step that is not a status function" "is allowed only on the clean-up and notary-log upload steps" \
  "$release" $'      - name: Sign The Binary\n' $'      - name: Sign The Binary\n        if: needs.plan.outputs.version != \'\'\n'
# GitHub reads a status function's name in any case.
expect_fail "Always() in assemble-signed's condition" "has a job-level condition that can run it after a failure" \
  "$release" $'    needs: [plan, build, sign-macos, verify-macos]\n'"$guard" $'    needs: [plan, build, sign-macos, verify-macos]\n    if: Always() && needs.plan.outputs.version != \'\''
expect_fail "Always() on a step of another job" "can run after a failure" \
  "$release" $'      - name: Download The Notary Log\n' $'      - name: Download The Notary Log\n        if: Always()\n'
expect_fail "Always() on a signing step" "is allowed only on the clean-up and notary-log upload steps" \
  "$release" $'      - name: Upload The Signed Files\n        uses: actions/upload-artifact@v7' $'      - name: Upload The Signed Files\n        if: ${{ ALWAYS() }}\n        uses: actions/upload-artifact@v7'
expect_fail "a clean-up step that only runs after a success" "must have exactly one clean-up step, with \`if: always()\`" \
  "$release" $'      - name: Delete The Keychain And The Key Files\n        if: always()\n' $'      - name: Delete The Keychain And The Key Files\n'
expect_fail "a notary log upload that only runs after a success" "must upload notary-log-macos once, with \`if: always()\`" \
  "$release" $'      - name: Upload The Notary Log\n        if: always()\n' $'      - name: Upload The Notary Log\n'

echo "-- the measured tools --"
expect_fail "a tool the dry run does not measure" "does not measure /usr/bin/openssl" \
  "$release_build" 'bin_tools="security codesign pkgbuild openssl plutil shasum xcrun lsbom sudo"' 'bin_tools="security codesign pkgbuild plutil shasum xcrun lsbom sudo"'
expect_fail "a verify tool the dry run does not measure" "does not measure /usr/sbin/spctl" \
  "$release_build" 'sbin_tools="pkgutil spctl installer"' 'sbin_tools="pkgutil installer"'
expect_fail "a measuring step that does not fail" "does not fail when a tool is missing" \
  "$release_build" $'            echo "error: the signing jobs call tools that are missing or do not behave as they rely on:${missing}" >&2\n            exit 1' $'            echo "error: the signing jobs call tools that are missing or do not behave as they rely on:${missing}" >&2'
expect_fail "a measuring step that receives a signing value" "receives a signing value" \
  "$release_build" $'      - name: Check The Signing Tools\n        if: runner.os == \'macOS\'\n' $'      - name: Check The Signing Tools\n        if: runner.os == \'macOS\'\n        env:\n          MACOS_P12_PASSWORD: ${{ secrets.MACOS_P12_PASSWORD }}\n'
expect_fail "a build whose assemble job does not call the script" "release-build.yaml assemble does not call assemble-release.sh" \
  "$release_build" 'bash .github/scripts/assemble-release.sh' 'bash .github/scripts/other.sh'
expect_fail "a measuring step on every leg" "must run on the macOS leg only" \
  "$release_build" $'      - name: Check The Signing Tools\n        if: runner.os == \'macOS\'' $'      - name: Check The Signing Tools\n        if: runner.os != \'Linux\''
expect_fail "a measuring step that does not print MEASURED" "has no step that prints the MEASURED: signing tools line" \
  "$release_build" 'echo "MEASURED: signing tools:${found}"' 'echo "signing tools:${found}"'
expect_fail "a tool test-signed-pkg.sh adds without being measured" "does not measure /usr/bin/stat" \
  ".github/scripts/tests/test-signed-pkg.sh" '"$SYSTEM_BIN/lsbom" -p f "$1"' '"$SYSTEM_BIN/stat" -p f "$1"'

echo "-- a file the check cannot read --"
expect_fail "a workflow in a syntax the check does not support" "cannot be read" \
  "$release" $'permissions:\n  contents: read\n' $'permissions: {contents: read}\n'

if [[ $failures -ne 0 ]]; then
  echo "${failures} release shape case(s) failed." >&2
  exit 1
fi
echo "All release shape cases passed."
