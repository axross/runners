#!/usr/bin/env bash
# runs the `run:` scripts of release.yaml's sign-update job in order, outside
# GitHub Actions, the way the job runs them: extracted from the workflow, not
# copied, each with only the environment its own step declares, in a working
# directory laid out as the job's download steps lay it out. merge-checks.yaml's
# update-feed job runs it with the real cargo-tauri and a throwaway key, and
# test-sign-update.sh with a stand-in signer.
#
#   run-sign-update.sh <work directory> <msi> <SHA256SUMS> <cargo-tauri>
#
# the step values come from the environment: SIGN_VERSION, SIGN_TAURI_CLI_SHA256,
# SIGN_KEY and SIGN_PASSWORD stand for the job's version, the tool job's hash and
# the two secrets, and a `${{ }}` this script does not know fails it. SIGN_WORKFLOW
# names another workflow file, and SIGN_SYSTEM_BIN, when set, is handed to the
# scripts as SYSTEM_BIN, so a test can put stand-ins in place of system tools.
# the signature ends up in <work directory>/job/signature, and the script prints
# its path.
set -euo pipefail

if [[ $# -ne 4 ]]; then
  echo "Usage: run-sign-update.sh <work directory> <msi> <SHA256SUMS> <cargo-tauri>" >&2
  exit 2
fi
work="$1"
msi="$2"
sums="$3"
tool="$4"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
workflow="${SIGN_WORKFLOW:-${here}/../../workflows/release.yaml}"

refuse() {
  echo "error: $*" >&2
  exit 1
}

job="${work}/job"
runner_temp="${work}/runner-temp"
scripts="${work}/scripts"
mkdir -p "${job}/unsigned" "${runner_temp}/update-tools"
cp -- "$msi" "${job}/unsigned/"
cp -- "$sums" "${job}/unsigned/SHA256SUMS"
cp -- "$tool" "${runner_temp}/update-tools/cargo-tauri"

node "${here}/extract-run-scripts.mjs" "$workflow" sign-update "$scripts" >/dev/null
steps="${scripts}/steps.json"

# the download steps must put their files where this script put them, and the
# upload step must take the signature from where the scripts leave it.
paths="$(jq -r '.[] | select(.kind == "uses") | "\(.uses | split("@")[0]) \(.with.name) \(.with.path)"' "$steps")"
want="actions/download-artifact \${{ needs.build.outputs.artifact }} unsigned
actions/download-artifact update-tools \${{ runner.temp }}/update-tools
actions/upload-artifact update-signature signature"
[[ "$paths" == "$want" ]] || refuse "sign-update's artifact steps are not the ones this script lays out:
${paths}"

# value <expression>: what the job would put in place of `${{ <expression> }}`.
value() {
  case "$1" in
    "needs.plan.outputs.version") echo "${SIGN_VERSION:?}" ;;
    "needs.update-tool.outputs.tauri_cli_sha256") echo "${SIGN_TAURI_CLI_SHA256:?}" ;;
    "secrets.UPDATE_FEED_SIGNING_KEY") echo "${SIGN_KEY-}" ;;
    "secrets.UPDATE_FEED_SIGNING_KEY_PASSWORD") echo "${SIGN_PASSWORD-}" ;;
    *) refuse "sign-update reads \${{ $1 }}, which this script does not know" ;;
  esac
}

while IFS=$'\t' read -r script names; do
  environment=("PATH=/usr/bin:/bin" "HOME=${work}" "RUNNER_TEMP=${runner_temp}")
  if [[ -n "${SIGN_SYSTEM_BIN:-}" ]]; then
    environment+=("SYSTEM_BIN=${SIGN_SYSTEM_BIN}")
  fi
  for name in $names; do
    raw="$(jq -r --arg script "$script" --arg name "$name" '.[] | select(.script == $script) | .env[$name]' "$steps")"
    # shellcheck disable=SC2016 # the ${{ the branches seek is literal.
    if [[ "$raw" =~ ^\$\{\{\ ([^}]*[^}\ ])\ \}\}$ ]]; then
      environment+=("${name}=$(value "${BASH_REMATCH[1]}")")
    elif [[ "$raw" == *'${{'* ]]; then
      refuse "the env ${name} of ${script} is more than one \${{ }}: ${raw}"
    else
      environment+=("${name}=${raw}")
    fi
  done
  echo "--- sign-update: ${script}"
  (cd "$job" && env -i "${environment[@]}" bash --noprofile --norc -eo pipefail "${scripts}/${script}")
done < <(jq -r '.[] | select(.kind == "run") | [.script, (.env | keys | join(" "))] | @tsv' "$steps")

signatures=("${job}"/signature/*)
[[ ${#signatures[@]} -eq 1 && -f "${signatures[0]}" ]] \
  || refuse "the scripts did not leave exactly one file in signature/"
echo "${signatures[0]}"
