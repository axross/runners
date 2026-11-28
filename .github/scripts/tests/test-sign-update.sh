#!/usr/bin/env bash
# runs the staged sign-update scripts through run-sign-update.sh against a
# stand-in cargo-tauri and a stand-in unshare, which record how they were called.
# covered, each as a case below: a signature for the build's MSI, made by the
# hash-checked signer under unshare with the two secrets in its environment and
# nowhere in the output; and a refusal, before the signer runs, of a changed MSI,
# an MSI the build's SHA256SUMS does not list once, a changed signer, and an
# empty key or password. a two-file list signs both files, and corruption of
# either refuses the whole list before any signer runs. merge-checks.yaml's
# update-feed job runs the same scripts with the real signer and a throwaway key.
# the step that lifts AppArmor's user namespace limit runs ahead of the signer, and
# its sudo is a stand-in.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
runner="${here}/run-sign-update.sh"
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT

version=1.2.3
msi="marshal-${version}-windows-x64.msi"
key="KEY-6c1f0e93a7"
password="PASSWORD-2b9d44e0"

# the stand-in signer writes a signature next to the file it is given and
# records its arguments and the two variables. it learns where to record from
# the stand-in unshare, so a signer called without unshare fails.
cat >"${work}/cargo-tauri" <<'SIGNER'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"${STANDIN_RECORD}.signer"
printf 'key=%s password=%s\n' "${TAURI_SIGNING_PRIVATE_KEY-}" "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD-}" >>"${STANDIN_RECORD}.secrets"
printf 'c2lnbmF0dXJl\n' >"${!#}.sig"
SIGNER
chmod +x "${work}/cargo-tauri"
line="$(sha256sum "${work}/cargo-tauri")"
tool_sha256="${line%% *}"

# SYSTEM_BIN for the scripts: the real awk and sha256sum, an unshare that
# records its options and runs the rest, and a sudo that records its arguments
# and runs nothing. the record path reaches it through a
# file, since each step runs with only its own environment.
bin="${work}/bin"
mkdir "$bin"
ln -s "$(command -v awk)" "${bin}/awk"
ln -s "$(command -v sha256sum)" "${bin}/sha256sum"
cat >"${bin}/unshare" <<UNSHARE
#!/usr/bin/env bash
set -euo pipefail
record="\$(<"${work}/record-path")"
options=()
while [[ "\${1:-}" == --* ]]; do
  options+=("\$1")
  shift
done
printf '%s\n' "\${options[*]}" >>"\${record}.unshare"
STANDIN_RECORD="\$record" exec "\$@"
UNSHARE
chmod +x "${bin}/unshare"
cat >"${bin}/sudo" <<SUDO
#!/usr/bin/env bash
set -euo pipefail
printf '%s\\n' "\$*" >>"\$(<"${work}/record-path").sudo"
SUDO
chmod +x "${bin}/sudo"

failures=0
fail() {
  echo "FAIL $*" >&2
  failures=$((failures + 1))
}

# run_case <name> <SHA256SUMS file> [VAR=value...] -- <file>...: runs the scripts
# in a directory of the case's own; sets status, output and record.
run_case() {
  local name="$1" sums="$2"
  shift 2
  local environment=()
  while [[ "${1:-}" != -- ]]; do
    [[ $# -gt 0 ]] || { fail "${name}: missing -- before files"; return; }
    environment+=("$1")
    shift
  done
  shift
  record="${work}/${name}/record"
  mkdir -p "${work}/${name}"
  echo "$record" >"${work}/record-path"
  status=0
  output="$(env SIGN_VERSION="$version" SIGN_TAURI_CLI_SHA256="$tool_sha256" \
    SIGN_KEY="$key" SIGN_PASSWORD="$password" SIGN_SYSTEM_BIN="$bin" "${environment[@]}" \
    bash "$runner" "${work}/${name}/run" "$sums" "${work}/cargo-tauri" "$@" 2>&1)" || status=$?
}

# expect_refusal <name> <text>: the case failed naming <text>, and never signed.
expect_refusal() {
  if [[ $status -eq 0 ]]; then
    fail "$1: the scripts passed: ${output}"
  elif [[ "$output" != *"$2"* ]]; then
    fail "$1: the output does not contain '$2': ${output}"
  elif [[ -e "${record}.signer" ]]; then
    fail "$1: the signer ran"
  else
    echo "ok   $1"
  fi
}

printf 'windows installer\n' >"${work}/${msi}"
(cd "$work" && sha256sum "$msi" >"${work}/SHA256SUMS")

run_case pass "${work}/SHA256SUMS" -- "${work}/${msi}"
if [[ $status -ne 0 ]]; then
  fail "a signature for the build's MSI: exit ${status}: ${output}"
else
  signature="${output##*$'\n'}"
  if [[ "$signature" != */signature/"${msi}.sig" || ! -f "$signature" ]]; then
    fail "the signature is not signature/${msi}.sig: ${signature}"
  elif [[ "$(<"${record}.signer")" != "signer sign --app-version ${version} signing/${msi}" ]]; then
    fail "the signer was called as: $(<"${record}.signer")"
  elif [[ "$(<"${record}.unshare")" != "--map-root-user --net" ]]; then
    fail "the signer did not run under unshare --map-root-user --net: $(cat "${record}.unshare" 2>&1)"
  elif [[ "$(<"${record}.secrets")" != "key=${key} password=${password}" ]]; then
    fail "the signer did not get the two secrets: $(<"${record}.secrets")"
  elif [[ "$output" == *"$key"* || "$output" == *"$password"* ]]; then
    fail "a secret appears in the job's output"
  else
    echo "ok   a signature for the build's MSI, by the signer under unshare, with the secrets in its environment only"
  fi
fi

# the user namespace step calls sudo exactly when this host has the limit set.
knob=/proc/sys/kernel/apparmor_restrict_unprivileged_userns
sysctl_call="sysctl -w kernel.apparmor_restrict_unprivileged_userns=0"
if [[ -f "$knob" && "$(<"$knob")" != 0 ]]; then
  want="$sysctl_call"
else
  want=""
fi
if [[ "$(cat "${work}/pass/record.sudo" 2>/dev/null)" != "$want" ]]; then
  fail "the user namespace step called sudo as '$(cat "${work}/pass/record.sudo" 2>&1)', not '${want}'"
else
  echo "ok   the user namespace step calls sudo only where the limit is set"
fi

extra=stand-in-update.archive
printf 'another update file\n' >"${work}/${extra}"
(cd "$work" && sha256sum "$msi" "$extra" >"${work}/two-file-sums")
node - "${here}/../../staged/release-feed.yaml" "${work}/two-files.yaml" "$extra" <<'WORKFLOW'
const fs = require("node:fs");
const [source, target, extra] = process.argv.slice(2);
const workflow = fs.readFileSync(source, "utf8");
const from = '"marshal-${VERSION}-windows-x64.msi" > checked-files';
if (workflow.split(from).length !== 2) {
  throw new Error("the workflow does not declare its checked-file list exactly once");
}
fs.writeFileSync(target, workflow.replace(from,
  '"marshal-${VERSION}-windows-x64.msi" \\\n            "' + extra + '" > checked-files'));
WORKFLOW

run_case two-files "${work}/two-file-sums" SIGN_WORKFLOW="${work}/two-files.yaml" -- "${work}/${msi}" "${work}/${extra}"
if [[ $status -ne 0 ]]; then
  fail "two checked files: exit ${status}: ${output}"
else
  expected_calls="$(printf 'signer sign --app-version %s signing/%s\n' "$version" "$msi" "$version" "$extra")"
  expected_paths="$(printf '%s/run/job/signature/%s.sig\n' "${work}/two-files" "$msi" "${work}/two-files" "$extra")"
  if [[ "$(<"${record}.signer")" != "$expected_calls" ]]; then
    fail "two checked files: the signer calls were: $(<"${record}.signer")"
  elif [[ "$(<"${record}.unshare")" != $'--map-root-user --net\n--map-root-user --net' ]]; then
    fail "two checked files: both signer calls must run under unshare"
  elif [[ "$(<"${record}.secrets")" != "$(printf 'key=%s password=%s\n' "$key" "$password" "$key" "$password")" ]]; then
    fail "two checked files: both signer calls must receive the secrets"
  elif [[ "$(printf '%s\n' "$output" | tail -n 2)" != "$expected_paths" ]]; then
    fail "two checked files: the runner did not return both signature paths: ${output}"
  elif [[ "$output" == *"$key"* || "$output" == *"$password"* ]]; then
    fail "two checked files: a secret appears in the output"
  else
    for file in "$msi" "$extra"; do
      signature="${work}/two-files/run/job/signature/${file}.sig"
      if [[ ! -f "$signature" || "$(<"$signature")" != c2lnbmF0dXJl ]]; then
        fail "two checked files: no expected signature for ${file}"
      fi
    done
    echo "ok   both checked files signed under unshare, with secrets only in the environment"
  fi
fi

cp "${work}/${msi}" "${work}/changed.msi"
printf 'changed\n' >>"${work}/changed.msi"
mkdir -p "${work}/changed"
mv "${work}/changed.msi" "${work}/changed/${msi}"
run_case changed-msi "${work}/SHA256SUMS" -- "${work}/changed/${msi}"
expect_refusal "a changed MSI" "${msi} has SHA-256"

run_case two-files-changed-msi "${work}/two-file-sums" SIGN_WORKFLOW="${work}/two-files.yaml" -- "${work}/changed/${msi}" "${work}/${extra}"
expect_refusal "a changed first file of two, before any signer runs" "${msi} has SHA-256"
cp "${work}/${extra}" "${work}/changed/${extra}"
printf 'changed\n' >>"${work}/changed/${extra}"
run_case two-files-changed-extra "${work}/two-file-sums" SIGN_WORKFLOW="${work}/two-files.yaml" -- "${work}/${msi}" "${work}/changed/${extra}"
expect_refusal "a changed second file of two, before any signer runs" "${extra} has SHA-256"

printf '%s  %s\n' "$(cut -d' ' -f1 "${work}/SHA256SUMS")" other.msi >"${work}/unlisted-sums"
run_case unlisted "${work}/unlisted-sums" -- "${work}/${msi}"
expect_refusal "an MSI the build's SHA256SUMS does not list" "does not list ${msi} exactly once"

cat "${work}/SHA256SUMS" "${work}/SHA256SUMS" >"${work}/twice-sums"
run_case twice "${work}/twice-sums" -- "${work}/${msi}"
expect_refusal "an MSI the build's SHA256SUMS lists twice" "does not list ${msi} exactly once"

run_case changed-tool "${work}/SHA256SUMS" SIGN_TAURI_CLI_SHA256="$(printf '0%.0s' {1..64})" -- "${work}/${msi}"
expect_refusal "a changed signer" "cargo-tauri has SHA-256"

run_case empty-key "${work}/SHA256SUMS" SIGN_KEY= -- "${work}/${msi}"
expect_refusal "an empty key" "must both be set"

run_case empty-password "${work}/SHA256SUMS" SIGN_PASSWORD= -- "${work}/${msi}"
expect_refusal "an empty password" "must both be set"

if [[ $failures -ne 0 ]]; then
  echo "${failures} sign-update case(s) failed." >&2
  exit 1
fi
echo "All sign-update cases passed."
