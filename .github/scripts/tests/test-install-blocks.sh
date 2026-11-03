#!/usr/bin/env bash
# runs the macOS install blocks of docs/operations/cli.md (the optional check,
# migration, uninstall, and bare-binary blocks) under the shell named by the
# argument, bash or zsh, with stand-ins first on PATH for gh, pkgutil, xattr, cp,
# sudo, installer, and marshal. each stand-in records its calls, so every refusal is
# produced and a case asserts what ran and what did not. nothing here touches the
# real /usr/local/bin or the real home directory: HOME and the uninstall block's
# installed= line point into a temporary directory.
#
# usage: test-install-blocks.sh [bash|zsh]. macOS ships bash 3.2, so this uses
# nothing newer.
set -euo pipefail

shell_under_test="${1:-bash}"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
document="$(cd "${here}/../../.." && pwd)/docs/operations/cli.md"
extract="${here}/extract-install-block.sh"
command -v "$shell_under_test" >/dev/null || {
  echo "error: ${shell_under_test} is not installed" >&2
  exit 2
}

command -v jq >/dev/null || {
  echo "error: jq is required on PATH for the gh stand-in; install it (for example apt install jq or brew install jq)." >&2
  exit 2
}

work="$(mktemp -d "${TMPDIR:-/tmp}/marshal-install-blocks.XXXXXX")"
trap 'rm -rf -- "$work"' EXIT
stand_ins="${work}/bin"
mkdir -p "$stand_ins"

version=1.2.3
commit_sha=0123456789abcdef0123456789abcdef01234567
failures=0
cases=0

# the stand-ins ---------------------------------------------------------------

cp "${here}/standin-gh.sh" "${stand_ins}/gh"

cat >"${stand_ins}/sudo" <<'STANDIN'
#!/usr/bin/env bash
echo "sudo $*" >>"$STANDIN_LOG"
exec "$@"
STANDIN

# cp copies the stand-in quarantine mark with the file, as macOS's cp copies the
# real attribute.
cat >"${stand_ins}/cp" <<'STANDIN'
#!/usr/bin/env bash
echo "cp $*" >>"$STANDIN_LOG"
/bin/cp "$@" || exit $?
if [[ $# -eq 2 && -e "$1.quarantine" ]]; then
  : >"$2.quarantine"
fi
STANDIN

cat >"${stand_ins}/xattr" <<'STANDIN'
#!/usr/bin/env bash
echo "xattr $*" >>"$STANDIN_LOG"
[[ "$2" == com.apple.quarantine && -e "$3.quarantine" ]] || {
  echo "xattr: No such xattr: $2" >&2
  exit 1
}
if [[ "$1" == -d ]]; then
  rm -f "$3.quarantine"
fi
STANDIN

cat >"${stand_ins}/pkgutil" <<'STANDIN'
#!/usr/bin/env bash
echo "pkgutil $*" >>"$STANDIN_LOG"
case "$1" in
  --expand-full)
    [[ -z "${STANDIN_EXPAND_FAIL:-}" ]] || exit 1
    mkdir -p "$3/Payload"
    if [[ "${STANDIN_PAYLOAD_VERSION}" != none ]]; then
      mkdir -p "$3/Payload/usr/local/bin"
      {
        echo '#!/bin/sh'
        echo "echo \"payload \$*\" >>\"$STANDIN_LOG\""
        echo "case \"\$1\" in --version) echo 'marshal ${STANDIN_PAYLOAD_VERSION}'; exit ${STANDIN_PAYLOAD_EXIT:-0};; esac"
      } >"$3/Payload/usr/local/bin/marshal"
      chmod 0644 "$3/Payload/usr/local/bin/marshal"
    fi
    ;;
  --pkg-info) [[ -e "$STANDIN_RECEIPT" ]] ;;
  --forget) rm -f "$STANDIN_RECEIPT" ;;
  *) exit 99 ;;
esac
STANDIN
# no documented block may run installer, so the stand-in fails the block as well as
# recording the call that assert_installs_nothing looks for.
cat >"${stand_ins}/installer" <<'STANDIN'
#!/usr/bin/env bash
echo "installer $*" >>"$STANDIN_LOG"
exit 1
STANDIN
chmod +x "${stand_ins}"/*

# the harness -----------------------------------------------------------------

fail() {
  echo "FAIL ${case_name}: $*" >&2
  failures=$((failures + 1))
}

# begin_case <name>: a fresh home, release folder, log, and default scenario.
begin_case() {
  cases=$((cases + 1))
  case_name="$1"
  case_dir="${work}/case"
  rm -rf "$case_dir"
  mkdir -p "${case_dir}/home/.local/bin" "${case_dir}/release" "${case_dir}/downloads" "${case_dir}/tmp" "${case_dir}/usr-local-bin"
  export STANDIN_LOG="${case_dir}/calls.log"
  : >"$STANDIN_LOG"
  export STANDIN_RELEASE="${case_dir}/release"
  export STANDIN_RECEIPT="${case_dir}/receipt"
  export STANDIN_TAG_TYPE=commit STANDIN_TAG_SHA="$commit_sha" STANDIN_ANNOTATED_SHA="$commit_sha"
  export STANDIN_COMPARE=identical STANDIN_PAYLOAD_VERSION="$version" STANDIN_PAYLOAD_EXIT=0
  unset STANDIN_DOWNLOAD_FAIL STANDIN_TAG_FAIL STANDIN_ANNOTATED_FAIL STANDIN_COMPARE_FAIL STANDIN_ATTEST_FAIL STANDIN_EXPAND_FAIL
  output="" exit_status=0
}

sha256_of() {
  local line
  line="$(shasum -a 256 "$1")"
  echo "${line%% *}"
}

# make_marshal <path> <version> <stop output> <stop exit status>: a stand-in
# marshal that records its calls and answers --version and stop.
make_marshal() {
  mkdir -p "$(dirname "$1")"
  {
    echo '#!/bin/sh'
    echo "echo \"marshal $1 \$*\" >>\"$STANDIN_LOG\""
    echo "case \"\$1\" in"
    echo "  --version) echo 'marshal $2';;"
    echo "  stop) echo '$3' >&2; exit $4;;"
    echo "esac"
  } >"$1"
  chmod 0755 "$1"
}

# run_block <id> [<line prefix>=<new line>...]: runs the documented block under
# the shell with each named line replaced. a replaced line must exist exactly once.
run_block() {
  local id="$1" block replacement prefix line
  shift
  block="$("$extract" "$document" "$id")"
  for replacement in "$@"; do
    prefix="${replacement%%=*}="
    line="  ${replacement}"
    block="$(printf '%s\n' "$block" | awk -v prefix="  ${prefix}" -v line="$line" '
      index($0, prefix) == 1 { print line; replaced++; next }
      { print }
      END { if (replaced != 1) exit 3 }')" || {
      fail "the block ${id} does not hold exactly one line starting with '${prefix}'"
      return 0
    }
  done
  assert_no_comment_lines "$id" "$block"
  exit_status=0
  output="$(env PATH="${stand_ins}:${PATH}" HOME="${case_dir}/home" TMPDIR="${case_dir}/tmp" \
    "$shell_under_test" -c "$block" 2>&1)" || exit_status=$?
}

assert_no_comment_lines() {
  if printf '%s\n' "$2" | grep -q -E '^[[:space:]]*#'; then
    fail "the block $1 has a comment line, which an interactive zsh would run as a command"
  fi
}

assert_passed() {
  [[ $exit_status -eq 0 ]] || fail "exit ${exit_status}, expected 0: ${output}"
  [[ "$output" != *"refused:"* ]] || fail "the block refused: ${output}"
}

# assert_refused <text the refusal must contain>
assert_refused() {
  [[ $exit_status -ne 0 ]] || fail "exit 0, expected a refusal containing '$1': ${output}"
  [[ "$output" == *"refused: "*"$1"* ]] || fail "no 'refused:' line containing '$1': ${output}"
}

assert_called() {
  grep -q -F -e "$1" "$STANDIN_LOG" || fail "'$1' was not called; calls: $(tr '\n' ';' <"$STANDIN_LOG")"
}

assert_not_called() {
  if grep -q -F -e "$1" "$STANDIN_LOG"; then
    fail "'$1' was called; calls: $(tr '\n' ';' <"$STANDIN_LOG")"
  fi
}

assert_installs_nothing() {
  assert_not_called "sudo "
  assert_not_called "installer "
  assert_not_called "pkgutil --forget"
}

assert_exists() { [[ -e "$1" ]] || fail "$1 does not exist"; }
assert_absent() { [[ ! -e "$1" && ! -L "$1" ]] || fail "$1 exists"; }

assert_clean_temp() {
  if [[ -n "$(ls -A "${case_dir}/tmp")" ]]; then
    fail "the block left temporary files: $(ls -A "${case_dir}/tmp")"
  fi
}

finish_case() {
  echo "ok   ${case_name}"
}

# the check block -------------------------------------------------------------

package_name="marshal-${version}-macos-arm64.pkg"

# prepare_check [<downloaded file name>]: a package of known bytes in the
# downloads folder, its SHA256SUMS line, and a quarantine mark on it.
prepare_check() {
  package="${case_dir}/downloads/${1:-$package_name}"
  printf 'package bytes\n' >"$package"
  : >"${package}.quarantine"
  package_sha="$(sha256_of "$package")"
  printf '%s  %s\n' "$package_sha" "$package_name" >"${STANDIN_RELEASE}/SHA256SUMS"
  before="$(sha256_of "$package")"
}

run_check() {
  run_block macos-check "version=${1:-$version}" "file=\"${package}\""
}

assert_check_left_the_file() {
  [[ "$(sha256_of "$package")" == "$before" ]] || fail "the downloaded file changed"
  assert_exists "${package}.quarantine"
  assert_clean_temp
}

begin_case "check: a genuine package passes and installs nothing"
prepare_check
run_check
assert_passed
[[ "$output" == *"Passed:"* ]] || fail "no passed line: ${output}"
assert_called "gh release download v${version} --repo axross/runners --pattern SHA256SUMS --dir "
assert_not_called "--pattern ${package_name}"
assert_called "gh attestation verify ${package} --repo axross/runners --signer-workflow axross/runners/.github/workflows/release.yaml --source-ref refs/heads/main --source-digest ${commit_sha} --deny-self-hosted-runners"
assert_called "cp ${package} "
assert_called "pkgutil --expand-full ${case_dir}/tmp/"
assert_not_called "pkgutil --expand-full ${package} "
assert_called "xattr -d com.apple.quarantine ${case_dir}/tmp/"
assert_not_called "xattr -d com.apple.quarantine ${package}"
assert_called "payload --version"
assert_installs_nothing
assert_check_left_the_file
finish_case

begin_case "check: the file may be called anything and sit in a folder with spaces"
mkdir -p "${case_dir}/my downloads"
prepare_check
mv "$package" "${case_dir}/my downloads/marshal (1).pkg"
mv "${package}.quarantine" "${case_dir}/my downloads/marshal (1).pkg.quarantine"
package="${case_dir}/my downloads/marshal (1).pkg"
run_check
assert_passed
assert_installs_nothing
assert_check_left_the_file
finish_case

begin_case "check: an annotated tag is dereferenced"
prepare_check
export STANDIN_TAG_TYPE=tag STANDIN_TAG_SHA=ffffffffffffffffffffffffffffffffffffffff
run_check
assert_passed
assert_called "gh api repos/axross/runners/git/tags/ffffffffffffffffffffffffffffffffffffffff"
assert_called "compare/main...${commit_sha}"
assert_called "--source-digest ${commit_sha} "
finish_case

begin_case "check: SHA256SUMS that cannot be downloaded"
prepare_check
export STANDIN_DOWNLOAD_FAIL=1
run_check
assert_refused "SHA256SUMS could not be downloaded"
assert_not_called "gh attestation"
assert_installs_nothing
assert_check_left_the_file
finish_case

begin_case "check: no line for the file name the version gives"
prepare_check
printf '%s  marshal-9.9.9-macos-arm64.pkg\n' "$package_sha" >"${STANDIN_RELEASE}/SHA256SUMS"
run_check
assert_refused "no single line for ${package_name}"
assert_not_called "gh api"
assert_not_called "gh attestation"
assert_installs_nothing
assert_check_left_the_file
finish_case

begin_case "check: two lines for the file name"
prepare_check
printf '%s  %s\n%s  %s\n' "$package_sha" "$package_name" "$package_sha" "$package_name" >"${STANDIN_RELEASE}/SHA256SUMS"
run_check
assert_refused "no single line for ${package_name}"
assert_not_called "gh attestation"
finish_case

begin_case "check: a checksum that does not match"
prepare_check
printf '%s  %s\n' "0000000000000000000000000000000000000000000000000000000000000000" "$package_name" >"${STANDIN_RELEASE}/SHA256SUMS"
run_check
assert_refused "does not match the ${package_name} line of SHA256SUMS"
assert_not_called "gh api"
assert_not_called "gh attestation"
assert_not_called "pkgutil"
assert_installs_nothing
assert_check_left_the_file
finish_case

begin_case "check: a file that is not there"
prepare_check
rm -f "$package"
run_check
assert_refused "could not be read"
assert_not_called "gh attestation"
finish_case

begin_case "check: a tag that cannot be read"
prepare_check
export STANDIN_TAG_FAIL=1
run_check
assert_refused "the tag v${version} could not be read"
assert_not_called "gh attestation"
assert_installs_nothing
finish_case

begin_case "check: an annotated tag that cannot be read"
prepare_check
export STANDIN_TAG_TYPE=tag STANDIN_ANNOTATED_FAIL=1
run_check
assert_refused "the annotated tag v${version} could not be read"
assert_not_called "gh attestation"
finish_case

begin_case "check: a tag that is not a commit"
prepare_check
export STANDIN_TAG_TYPE=tree
run_check
assert_refused "v${version} does not resolve to a commit"
assert_not_called "gh attestation"
finish_case

begin_case "check: a commit that cannot be compared with main"
prepare_check
export STANDIN_COMPARE_FAIL=1
run_check
assert_refused "could not be compared with main"
assert_not_called "gh attestation"
finish_case

for comparison in ahead diverged; do
  begin_case "check: a commit whose comparison with main is ${comparison}"
  prepare_check
  export STANDIN_COMPARE="$comparison"
  run_check
  assert_refused "is not on main (${comparison})"
  assert_not_called "gh attestation"
  assert_not_called "pkgutil"
  assert_installs_nothing
  assert_check_left_the_file
  finish_case
done

begin_case "check: an attestation that does not verify"
prepare_check
export STANDIN_ATTEST_FAIL=1
run_check
assert_refused "the attestation does not verify"
assert_not_called "pkgutil"
assert_not_called "cp "
assert_installs_nothing
assert_check_left_the_file
finish_case

begin_case "check: a package that cannot be unpacked"
prepare_check
export STANDIN_EXPAND_FAIL=1
run_check
assert_refused "the package could not be unpacked"
assert_installs_nothing
assert_check_left_the_file
finish_case

begin_case "check: a package without the binary"
prepare_check
export STANDIN_PAYLOAD_VERSION=none
run_check
assert_refused "the package holds no usr/local/bin/marshal"
assert_installs_nothing
finish_case

begin_case "check: a binary that prints another version"
prepare_check
export STANDIN_PAYLOAD_VERSION=0.0.1
run_check
assert_refused "is not marshal ${version}: marshal 0.0.1"
assert_installs_nothing
assert_check_left_the_file
finish_case

begin_case "check: a binary that does not run"
prepare_check
export STANDIN_PAYLOAD_EXIT=3
run_check
assert_refused "does not run"
assert_installs_nothing
finish_case

begin_case "check: a refusal does not end the calling shell"
prepare_check
export STANDIN_ATTEST_FAIL=1
block="$("$extract" "$document" macos-check | sed -e "s|^  version=.*|  version=${version}|" -e "s|^  file=.*|  file=\"${package}\"|")"
exit_status=0
output="$(env PATH="${stand_ins}:${PATH}" HOME="${case_dir}/home" TMPDIR="${case_dir}/tmp" \
  "$shell_under_test" -c "${block}
echo still-running" 2>&1)" || exit_status=$?
[[ $exit_status -eq 0 && "$output" == *"still-running" && "$output" == *"refused: "* ]] || fail "the shell ended with the block: ${output}"
finish_case

# the migration block ---------------------------------------------------------

manual="home/.local/bin/marshal"

begin_case "migrate: there is no manual install"
run_block macos-migrate
assert_passed
[[ "$output" == *"nothing to migrate"* ]] || fail "no note: ${output}"
assert_not_called "marshal "
finish_case

begin_case "migrate: the manual install is stopped and removed"
make_marshal "${case_dir}/${manual}" 0.0.1 "No instance is running." 1
run_block macos-migrate
assert_passed
assert_called "marshal ${case_dir}/${manual} stop"
assert_absent "${case_dir}/${manual}"
finish_case

begin_case "migrate: an instance that does not stop"
make_marshal "${case_dir}/${manual}" 0.0.1 "the control channel did not answer" 1
run_block macos-migrate
assert_refused "the instance did not stop: the control channel did not answer"
assert_exists "${case_dir}/${manual}"
finish_case

# the uninstall block ---------------------------------------------------------

installed="${work}/case/usr-local-bin/marshal"

begin_case "uninstall: the file is removed and the receipt forgotten"
make_marshal "$installed" "$version" "" 0
: >"$STANDIN_RECEIPT"
run_block macos-uninstall "installed=${installed}"
assert_passed
assert_called "marshal ${installed} stop"
assert_called "sudo rm -f ${installed}"
assert_called "sudo pkgutil --forget io.github.axross.runners.marshal"
assert_absent "$installed"
assert_absent "$STANDIN_RECEIPT"
finish_case

begin_case "uninstall: no receipt is left to forget"
make_marshal "$installed" "$version" "No instance is running." 1
run_block macos-uninstall "installed=${installed}"
assert_passed
assert_absent "$installed"
assert_not_called "pkgutil --forget"
finish_case

begin_case "uninstall: an instance that does not stop"
make_marshal "$installed" "$version" "the control channel did not answer" 1
: >"$STANDIN_RECEIPT"
run_block macos-uninstall "installed=${installed}"
assert_refused "the instance did not stop: the control channel did not answer"
assert_exists "$installed"
assert_exists "$STANDIN_RECEIPT"
assert_not_called "sudo "
finish_case

# the bare-binary block -------------------------------------------------------

bare_version=0.1.0
bare_asset="marshal-${bare_version}-macos-arm64"
bare_target="home/.local/bin/marshal"

prepare_bare() {
  make_marshal "${STANDIN_RELEASE}/${bare_asset}" "$bare_version" "" 0
  chmod 0644 "${STANDIN_RELEASE}/${bare_asset}"
  printf '%s  %s\n' "$(sha256_of "${STANDIN_RELEASE}/${bare_asset}")" "$bare_asset" >"${STANDIN_RELEASE}/SHA256SUMS"
}

begin_case "bare binary: a genuine binary is installed"
prepare_bare
run_block macos-bare
assert_passed
[[ "$("${case_dir}/${bare_target}" --version)" == "marshal ${bare_version}" ]] || fail "the installed binary is not the downloaded one"
assert_called "gh attestation verify ${bare_asset} --repo axross/runners --signer-workflow axross/runners/.github/workflows/release.yaml --source-ref refs/heads/main --source-digest ${commit_sha} --deny-self-hosted-runners"
finish_case

begin_case "bare binary: a running instance is stopped before the binary is replaced"
prepare_bare
make_marshal "${case_dir}/${bare_target}" 0.0.1 "No instance is running." 1
run_block macos-bare
assert_passed
assert_called "marshal ${case_dir}/${bare_target} stop"
[[ "$("${case_dir}/${bare_target}" --version)" == "marshal ${bare_version}" ]] || fail "the binary was not replaced"
finish_case

begin_case "bare binary: a checksum that does not match"
prepare_bare
printf '%s  %s\n' "0000000000000000000000000000000000000000000000000000000000000000" "$bare_asset" >"${STANDIN_RELEASE}/SHA256SUMS"
run_block macos-bare
assert_refused "does not match SHA256SUMS"
assert_absent "${case_dir}/${bare_target}"
assert_not_called "gh attestation"
finish_case

begin_case "bare binary: a commit that is not on main"
prepare_bare
export STANDIN_COMPARE=diverged
run_block macos-bare
assert_refused "is not on main (diverged)"
assert_absent "${case_dir}/${bare_target}"
finish_case

begin_case "bare binary: an attestation that does not verify"
prepare_bare
export STANDIN_ATTEST_FAIL=1
run_block macos-bare
assert_refused "the attestation does not verify"
assert_absent "${case_dir}/${bare_target}"
finish_case

begin_case "bare binary: a binary that reports another version once installed"
prepare_bare
{
  echo '#!/bin/sh'
  echo "case \"\$0\" in */.local/bin/marshal) echo 'marshal 0.9.9';; *) echo 'marshal ${bare_version}';; esac"
} >"${STANDIN_RELEASE}/${bare_asset}"
printf '%s  %s\n' "$(sha256_of "${STANDIN_RELEASE}/${bare_asset}")" "$bare_asset" >"${STANDIN_RELEASE}/SHA256SUMS"
run_block macos-bare
assert_refused "the installed binary is not marshal ${bare_version}: marshal 0.9.9"
assert_exists "${case_dir}/${bare_target}"
finish_case

begin_case "bare binary: an older binary under a newer name leaves the install path alone"
prepare_bare
make_marshal "${STANDIN_RELEASE}/${bare_asset}" 0.0.9 "" 0
chmod 0644 "${STANDIN_RELEASE}/${bare_asset}"
printf '%s  %s\n' "$(sha256_of "${STANDIN_RELEASE}/${bare_asset}")" "$bare_asset" >"${STANDIN_RELEASE}/SHA256SUMS"
make_marshal "${case_dir}/${bare_target}" 0.0.1 "No instance is running." 1
before="$(sha256_of "${case_dir}/${bare_target}")"
run_block macos-bare
assert_refused "is not marshal ${bare_version}: marshal 0.0.9"
[[ "$(sha256_of "${case_dir}/${bare_target}")" == "$before" ]] || fail "the install path changed"
assert_not_called "marshal ${case_dir}/${bare_target} stop"
finish_case

begin_case "bare binary: an instance that does not stop leaves the install path alone"
prepare_bare
make_marshal "${case_dir}/${bare_target}" 0.0.1 "the control channel did not answer" 1
before="$(sha256_of "${case_dir}/${bare_target}")"
run_block macos-bare
assert_refused "the instance did not stop: the control channel did not answer"
[[ "$(sha256_of "${case_dir}/${bare_target}")" == "$before" ]] || fail "the install path changed"
finish_case

echo
if [[ $failures -ne 0 ]]; then
  echo "${failures} failure(s) in ${cases} cases under ${shell_under_test}." >&2
  exit 1
fi
echo "All ${cases} install block cases passed under ${shell_under_test}."
