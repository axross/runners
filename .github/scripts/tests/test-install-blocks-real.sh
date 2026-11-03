#!/usr/bin/env bash
# runs the documented macOS install blocks of docs/operations/cli.md against the
# release dry run's real package, under the shell named by the first argument
# (zsh, or /bin/bash).
#
# runs on the macOS leg of the Install Blocks job in merge-checks.yaml. only gh is
# stubbed: standin-gh.sh serves the artifact's real SHA256SUMS, a tag on main, and
# a passing attestation. the check, migration, and uninstall blocks run with the
# real pkgutil, xattr, installer, and passwordless sudo. the one manual install is
# a stand-in script that differs from the package's binary. outcomes the CLI
# document states are printed on lines that start with MEASURED; a failed check
# exits non-zero. macOS ships bash 3.2, so this uses nothing newer.
#
# usage: test-install-blocks-real.sh <shell> <release directory> <release version>
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "Usage: test-install-blocks-real.sh <shell> <release directory> <release version>" >&2
  exit 2
fi
command -v jq >/dev/null || {
  echo "error: jq is required on PATH for the gh stand-in; install it (for example apt install jq or brew install jq)." >&2
  exit 2
}
shell_under_test="$1"
release_directory="$(cd "$2" && pwd)"
version="$3"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
document="$(cd "${here}/../../.." && pwd)/docs/operations/cli.md"
extract="${here}/extract-install-block.sh"
package_name="marshal-${version}-macos-arm64.pkg"
bare_name="marshal-${version}-macos-arm64"
installed="/usr/local/bin/marshal"
identifier="io.github.axross.runners.marshal"
manual="${HOME}/.local/bin/marshal"
logs_directory="${HOME}/Library/Logs/marshal"
commit_sha=0123456789abcdef0123456789abcdef01234567

measured() { echo "MEASURED: $*"; }
ok() { echo "ok   $*"; }
fail() {
  echo "FAILED: $*" >&2
  exit 1
}

# assert_equal <what> <actual> <expected>
assert_equal() {
  [[ "$2" == "$3" ]] || fail "$1: got '$2', expected '$3'"
  ok "$1"
}

sha256_of() {
  local line
  line="$(shasum -a 256 "$1")"
  echo "${line%% *}"
}

work="$(mktemp -d "${TMPDIR:-/tmp}/marshal-install-blocks-real.XXXXXX")"
stand_ins="${work}/bin"
mkdir -p "$stand_ins" "${work}/Downloads" "${work}/served"
cp "${here}/standin-gh.sh" "${stand_ins}/gh"
chmod +x "${stand_ins}/gh"
export STANDIN_LOG="${work}/calls.log"
: >"$STANDIN_LOG"
export STANDIN_RELEASE="$release_directory"
export STANDIN_TAG_SHA="$commit_sha"
tmp_directory="${work}/tmp"
mkdir -p "$tmp_directory"

# set after the clean-start guards pass, so a refusal on a host that has marshal
# installed never removes it.
cleanup_armed=0

cleanup() {
  if [[ $cleanup_armed -eq 1 ]]; then
    rm -f "$manual"
    sudo rm -f "$installed" || true
    sudo pkgutil --forget "$identifier" >/dev/null 2>&1 || true
    rm -f "${logs_directory}/install-blocks-test.marker"
  fi
  rm -rf -- "$work"
}
trap cleanup EXIT

# run_block <id> [<prefix>=<new assignment line>...]: runs the documented block
# with each named assignment line replaced, in the shell under test, and keeps
# its output and exit status in $output and $block_status.
run_block() {
  local id="$1" block replacement prefix line
  shift
  block="$("$extract" "$document" "$id")"
  for replacement in "$@"; do
    prefix="${replacement%%=*}="
    block="$(printf '%s\n' "$block" | awk -v prefix="  ${prefix}" -v line="  ${replacement}" '
      index($0, prefix) == 1 { print line; replaced++; next }
      { print }
      END { if (replaced != 1) exit 3 }')" || fail "the block ${id} does not hold exactly one line starting with '${prefix}'"
  done
  if printf '%s\n' "$block" | grep -q -E '^[[:space:]]*#'; then
    fail "the block ${id} has a comment line"
  fi
  block_status=0
  output="$(env PATH="${stand_ins}:${PATH}" TMPDIR="$tmp_directory" "$shell_under_test" -c "$block" 2>&1)" || block_status=$?
  while IFS= read -r line; do
    echo "     | ${line}"
  done <<<"$output"
}

nothing_installed() {
  [[ ! -e "$installed" && ! -L "$installed" ]] || fail "$1: ${installed} exists"
  if pkgutil --pkg-info "$identifier" >/dev/null 2>&1; then fail "$1: a receipt for ${identifier} exists"; fi
  ok "$1: nothing is installed"
}

make_manual() {
  mkdir -p "$(dirname "$manual")"
  cat >"$manual" <<'MANUAL'
#!/bin/sh
case "$1" in
  --version) echo 'marshal 0.0.1-manual';;
  stop) echo 'No instance is running.' >&2; exit 1;;
esac
MANUAL
  chmod 0755 "$manual"
}

# install_package <package> <log>: installs with the real installer and keeps its output.
install_package() {
  sudo installer -pkg "$1" -target / 2>&1 | tee "$2" >/dev/null || fail "installer exited non-zero: $(cat "$2")"
}

sudo -n true || fail "sudo asks for a password"
[[ ! -e "$installed" && ! -L "$installed" ]] || fail "${installed} exists before the tests"
[[ ! -e "$manual" && ! -L "$manual" ]] || fail "${manual} exists before the tests"
nothing_installed "before the tests"
cleanup_armed=1
package="${release_directory}/${package_name}"
bare="${release_directory}/${bare_name}"
package_hash="$(sha256_of "$package")"
bare_hash="$(sha256_of "$bare")"
listed="$(awk -v name="$package_name" '$2 == name { print $1 }' "${release_directory}/SHA256SUMS")"
assert_equal "the artifact SHA256SUMS lists the package with its SHA-256" "$listed" "$package_hash"

echo "== the check block passes a genuine package given as a renamed copy with a download mark =="
renamed="${work}/Downloads/marshal ${version} (1).pkg"
cp "$package" "$renamed"
xattr -w com.apple.quarantine "0083;65a1b2c3;Safari;" "$renamed"
mark_before="$(xattr -p com.apple.quarantine "$renamed")"
assert_equal "the renamed copy carries the quarantine attribute" "$mark_before" "0083;65a1b2c3;Safari;"
stamp_before="$(/usr/bin/stat -f '%m' "$renamed")"
run_block macos-check "version=${version}" "file=\"${renamed}\""
[[ $block_status -eq 0 ]] || fail "the check block did not pass"
[[ "$output" == *"Passed:"* ]] || fail "the check block printed no passed line"
assert_equal "the check block left the file unchanged" "$(sha256_of "$renamed")" "$package_hash"
assert_equal "the check block left the quarantine attribute unchanged" "$(xattr -p com.apple.quarantine "$renamed")" "$mark_before"
assert_equal "the check block left the file time unchanged" "$(/usr/bin/stat -f '%m' "$renamed")" "$stamp_before"
nothing_installed "after the check block"
grep -q -F -e "gh attestation verify ${renamed} --repo axross/runners --signer-workflow axross/runners/.github/workflows/release.yaml --source-ref refs/heads/main --source-digest ${commit_sha} --deny-self-hosted-runners" "$STANDIN_LOG" \
  || fail "the attestation was not verified on the downloaded file with the pins"
ok "the attestation was verified on the downloaded file with the pins"

echo "== the check block refuses a genuine package under another version's name =="
printf '%s  marshal-9.9.9-macos-arm64.pkg\n' "$package_hash" >"${work}/served/SHA256SUMS"
export STANDIN_RELEASE="${work}/served"
run_block macos-check "version=9.9.9" "file=\"${renamed}\""
export STANDIN_RELEASE="$release_directory"
[[ $block_status -ne 0 ]] || fail "the check block passed a package under another version's name"
[[ "$output" == *"refused: "*"is not marshal 9.9.9: marshal ${version}"* ]] || fail "the refusal is not at the version step: ${output}"
ok "the check block refuses at the version step"
assert_equal "the refused check left the file unchanged" "$(sha256_of "$renamed")" "$package_hash"
assert_equal "the refused check left the quarantine attribute unchanged" "$(xattr -p com.apple.quarantine "$renamed")" "$mark_before"
nothing_installed "after the refused check"

echo "== migration from a manual install, then install, then uninstall =="
make_manual
assert_equal "the seeded manual copy is not the bare binary" "$([[ "$(sha256_of "$manual")" == "$bare_hash" ]] && echo same || echo different)" "different"
run_block macos-migrate
[[ $block_status -eq 0 ]] || fail "the migration block did not pass"
[[ ! -e "$manual" ]] || fail "the migration block left ${manual}"
ok "the migration block removed the manual install"

install_package "$package" "${work}/install.log"
ok "installer installs the package"
assert_equal "the installed file is the bare binary" "$(sha256_of "$installed")" "$bare_hash"
assert_equal "marshal --version at the install path" "$("$installed" --version)" "marshal ${version}"
assert_equal "a login shell finds marshal by name" "$(env -i HOME="$HOME" "$shell_under_test" -l -c 'command -v marshal')" "$installed"

mkdir -p "$logs_directory"
echo "kept by the uninstall" >"${logs_directory}/install-blocks-test.marker"
run_block macos-uninstall
[[ $block_status -eq 0 ]] || fail "the uninstall block did not pass"
nothing_installed "after the uninstall block"
[[ -f "${logs_directory}/install-blocks-test.marker" ]] || fail "the uninstall block removed the logs"
ok "the uninstall block left ~/Library/Logs/marshal"

echo "== installing over a manual copy without the migration block =="
make_manual
manual_hash="$(sha256_of "$manual")"
install_package "$package" "${work}/install-over-manual.log"
# which_binary <path>: names the binary at the path by its SHA-256.
which_binary() {
  local hash
  [[ -n "$1" && -e "$1" ]] || { echo "nothing"; return 0; }
  hash="$(sha256_of "$1")"
  if [[ "$hash" == "$manual_hash" ]]; then
    echo "the manual copy"
  elif [[ "$hash" == "$bare_hash" ]]; then
    echo "the installer's binary"
  else
    echo "another file"
  fi
}
at_install_path="$(which_binary "$installed")"
manual_remains=no
[[ -e "$manual" ]] && manual_remains=yes
shell_path="${HOME}/.local/bin:/usr/local/bin:/usr/bin:/bin"
resolved="$(env -i HOME="$HOME" PATH="$shell_path" "$shell_under_test" -c 'command -v marshal')"
resolved_is="$(which_binary "$resolved")"
found="$(env -i HOME="$HOME" PATH="$shell_path" "$shell_under_test" -c 'command -v marshal; marshal --version' | tr '\n' ' ')"
measured "installing the pkg beside a manual copy at ~/.local/bin/marshal, without the migration block: ${installed} is ${at_install_path}; the manual copy remains: ${manual_remains}; with ~/.local/bin ahead of /usr/local/bin on PATH, command -v marshal resolves to ${resolved_is}, and command -v marshal and marshal --version print: ${found}"
run_block macos-uninstall
measured "the uninstall block after that install: exit status ${block_status}; ${installed} afterwards: $([[ -e "$installed" ]] && echo present || echo absent); the manual copy afterwards: $([[ -e "$manual" ]] && echo present || echo absent)"
[[ $block_status -eq 0 ]] || fail "the uninstall block did not pass after the install over a manual copy"

echo "All macOS install block checks passed under ${shell_under_test}."
