#!/usr/bin/env bash
# installs, upgrades, and removes flat packages with the real `installer` and
# passwordless sudo, and reads the release package's contents.
#
# runs on the macOS build leg of release-build.yaml, after build-pkg.sh has built
# the release package. it builds throwaway packages at fixed test versions from
# two copies of the release binary that differ in trailing bytes, so every install
# is judged by the installed file's SHA-256. those copies are only packed and
# hashed, never run; the unmodified release binary alone prints its version. each
# outcome the CLI document states is printed on a line that starts with MEASURED.
# a failed check exits non-zero, so nothing is skipped silently. the removal at the
# end runs the uninstall block of the CLI document, extracted from it. macOS ships
# bash 3.2, so this uses nothing newer. see docs/operations/cli.md.
set -euo pipefail

if [[ $# -ne 5 ]]; then
  echo "Usage: test-pkg-installer.sh <binary> <release pkg> <release version> <installer version> <log directory>" >&2
  exit 2
fi
binary="$1"
release_pkg="$2"
release_version="$3"
installer_version="$4"
log_directory="$5"

identifier="app.axross.marshal"
installed="/usr/local/bin/marshal"
logs_directory="${HOME}/Library/Logs/marshal"
logs_marker="${logs_directory}/installer-test.marker"
build_pkg="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/build-pkg.sh"
extract_block="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/extract-install-block.sh"
cli_document="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)/docs/operations/cli.md"

measured() {
  echo "MEASURED: $*"
}

ok() {
  echo "ok   $*"
}

fail() {
  echo "FAILED: $*" >&2
  exit 1
}

# assert_equal <what> <actual> <expected>
assert_equal() {
  [[ "$2" == "$3" ]] || fail "$1: got '$2', expected '$3'"
  ok "$1"
}

assert_true() {
  local message="$1"
  shift
  "$@" || fail "$message"
  ok "$message"
}

sha256_of() {
  local line
  line="$(shasum -a 256 "$1")"
  echo "${line%% *}"
}

describe_owner_and_mode() {
  /usr/bin/stat -f '%Su:%Sg %Lp' "$1"
}

describe_usr_local_bin() {
  if [[ -d /usr/local/bin ]]; then
    describe_owner_and_mode /usr/local/bin
  else
    echo absent
  fi
}

receipt_version() {
  local info
  info="$(pkgutil --pkg-info "$identifier")"
  echo "$info" | awk '/^version:/ { print $2 }'
}

has_receipt() {
  pkgutil --pkg-info "$identifier" >/dev/null 2>&1
}

# install_pkg <pkg> <log name>: prints the installer's exit status; the installer's
# own log and the status are kept in the log directory.
install_pkg() {
  local status=0
  sudo installer -pkg "$1" -target / -dumplog 2>&1 | tee "${log_directory}/$2.log" >/dev/null || status=$?
  echo "$status" >"${log_directory}/$2.status"
  return "$status"
}

# installer_failed <log name> <message>: prints the tail of the installer's log to
# the job log, then fails.
installer_failed() {
  echo "---- the last lines of ${log_directory}/$1.log" >&2
  tail -n 30 "${log_directory}/$1.log" >&2 || true
  fail "$2"
}

# bom_entry <Bom> <path>: the path's octal mode, symbolic mode, user, and group
# as `lsbom` prints them, one tab-separated line.
bom_entry() {
  local bom="$1" path="$2" names modes symbols users groups user_ids group_ids
  names="$(lsbom -p f "$bom")"
  modes="$(lsbom -p m "$bom")"
  symbols="$(lsbom -p M "$bom")"
  users="$(lsbom -p U "$bom")"
  groups="$(lsbom -p G "$bom")"
  user_ids="$(lsbom -p u "$bom")"
  group_ids="$(lsbom -p g "$bom")"
  paste <(echo "$names") <(echo "$modes") <(echo "$symbols") <(echo "$users") <(echo "$groups") <(echo "$user_ids") <(echo "$group_ids") |
    awk -F '\t' -v path="$path" '$1 == path { print $2 "\t" $3 "\t" $4 "\t" $5 "\t" $6 "\t" $7 }'
}

work="$(mktemp -d "${TMPDIR:-/tmp}/marshal-pkg-test.XXXXXX")"
logs_directory_existed=0
[[ -d "$logs_directory" ]] && logs_directory_existed=1

# set after the clean-start guards pass, so a refusal on a host that has marshal
# installed never removes it.
cleanup_armed=0

cleanup() {
  if [[ $cleanup_armed -eq 1 ]]; then
    sudo rm -f "$installed" || true
    sudo pkgutil --forget "$identifier" >/dev/null 2>&1 || true
    rm -f "$logs_marker"
    if [[ $logs_directory_existed -eq 0 ]]; then
      rmdir "$logs_directory" 2>/dev/null || true
    fi
  fi
  rm -rf -- "$work"
}
trap cleanup EXIT

mkdir -p "$log_directory"
binary_hash="$(sha256_of "$binary")"

echo "== a clean start =="
sudo -n true || fail "sudo asks for a password"
ok "sudo needs no password"
[[ ! -e "$installed" && ! -L "$installed" ]] || fail "${installed} exists before the tests"
ok "${installed} is absent"
if has_receipt; then fail "a receipt for ${identifier} exists before the tests"; fi
ok "no receipt for ${identifier}"
cleanup_armed=1
before_directory="$(describe_usr_local_bin)"
mkdir -p "$logs_directory"
echo "kept by the uninstall" >"$logs_marker"

echo "== the release package =="
expanded="${work}/expanded-release"
pkgutil --expand-full "$release_pkg" "$expanded"
ok "pkgutil --expand-full expands the release package without root"
measured "the expanded package holds: $(cd "$expanded" && echo *)"
payload_binary="${expanded}/Payload/usr/local/bin/marshal"
assert_true "the expanded payload holds usr/local/bin/marshal" test -f "$payload_binary"
# find's -mindepth 1 leaves out the payload directory itself, so the listing is the
# binary and the three directories above it, and nothing else.
payload_entries="$(cd "${expanded}/Payload" && find . -mindepth 1 | LC_ALL=C sort | tr '\n' ' ')"
measured "the expanded payload lists: ${payload_entries}"
assert_equal "the expanded payload holds only usr/local/bin/marshal and its parent directories" \
  "$payload_entries" "./usr ./usr/local ./usr/local/bin ./usr/local/bin/marshal "
assert_equal "the expanded binary has the release binary SHA-256" "$(sha256_of "$payload_binary")" "$binary_hash"
scripts_found="$(find "$expanded" -iname 'Scripts*' | tr '\n' ' ')"
assert_equal "the expanded package holds no Scripts" "$scripts_found" ""
package_info="${expanded}/PackageInfo"
assert_true "PackageInfo names the identifier" grep -q "identifier=\"${identifier}\"" "$package_info"
assert_true "PackageInfo names version ${installer_version}" grep -q "version=\"${installer_version}\"" "$package_info"
assert_equal "PackageInfo declares no scripts" "$(grep -c '<scripts' "$package_info" || true)" "0"
measured "PackageInfo: $(tr -d '\n' <"$package_info")"

bom="${expanded}/Bom"
root_entry="$(bom_entry "$bom" .)"
directory_entry="$(bom_entry "$bom" ./usr/local/bin)"
file_entry="$(bom_entry "$bom" ./usr/local/bin/marshal)"
measured "the bill of materials, usr/local/bin (octal mode, symbolic mode, user, group, uid, gid): ${directory_entry}"
measured "the bill of materials, usr/local/bin/marshal (octal mode, symbolic mode, user, group, uid, gid): ${file_entry}"
measured "the bill of materials, the root entry (octal mode, symbolic mode, user, group, uid, gid): ${root_entry:-none}"
if [[ -n "$root_entry" ]]; then
  IFS=$'\t' read -r octal _ <<<"$root_entry"
  assert_equal "the bill of materials gives the root entry mode 0755" "${octal: -3}" "755"
fi
[[ -n "$directory_entry" ]] || fail "the bill of materials lists no usr/local/bin"
IFS=$'\t' read -r octal _ user group user_id group_id <<<"$directory_entry"
assert_equal "the bill of materials gives usr/local/bin mode 0755" "${octal: -3}" "755"
assert_true "the bill of materials gives usr/local/bin owner root:wheel" \
  test \( "$user" = root -o "$user_id" = 0 \) -a \( "$group" = wheel -o "$group_id" = 0 \)
IFS=$'\t' read -r octal _ user group user_id group_id <<<"$file_entry"
assert_equal "the bill of materials gives usr/local/bin/marshal mode 0755" "${octal: -3}" "755"
assert_true "the bill of materials gives usr/local/bin/marshal owner root:wheel" \
  test \( "$user" = root -o "$user_id" = 0 \) -a \( "$group" = wheel -o "$group_id" = 0 \)

table_of_contents="$(xar -f "$release_pkg" --dump-toc=-)"
archive_owners="$(echo "$table_of_contents" | grep -o '<user>[^<]*</user>' | sort -u | tr '\n' ' ' || true)"
measured "the package archive owners: ${archive_owners}"
# pkgbuild records the building account as each entry's owner, and every release package
# is built on a hosted runner, whose account is runner. so the owner is checked as that
# one account and left out of the name check, which would otherwise refuse it.
runner_owner="<user>runner</user>"
assert_equal "the package archive owner is only the hosted runner account runner (any other owner means the package was built by another account)" \
  "$archive_owners" "${runner_owner} "
table_of_contents="${table_of_contents//"$runner_owner"/}"
planted="${work}/planted-name"
for identifying in "${USER:-}" "$(hostname)" "$(hostname -s)" "${HOME}" "${GITHUB_WORKSPACE:-}" "${RUNNER_TEMP:-}"; do
  [[ ${#identifying} -ge 4 ]] || continue
  # whole words only: the hosted runner's account, runner, is inside runners, which
  # the identifier holds. the two plants show the match finds a name and only a name.
  printf 'owner="%s"\n' "$identifying" >"$planted"
  grep -l -i -F -w -e "$identifying" "$planted" >/dev/null || fail "the name check does not find '${identifying}' in a file that holds it"
  printf 'owner="%ss"\n' "$identifying" >"$planted"
  if grep -l -i -F -w -e "$identifying" "$planted" >/dev/null; then
    fail "the name check finds '${identifying}' inside a longer word"
  fi
  if grep -r -l -i -F -w --exclude-dir=Payload -e "$identifying" "$expanded" >/dev/null; then
    fail "the expanded package holds a machine, account, or workspace name"
  fi
  if echo "$table_of_contents" | grep -i -F -w -q -e "$identifying"; then
    fail "the package archive's table of contents holds a machine, account, or workspace name"
  fi
done
ok "no package metadata holds a machine, account, or workspace name"

echo "== the release package installs =="
install_pkg "$release_pkg" release-install || installer_failed release-install "installer exited non-zero for the release package (log: ${log_directory}/release-install.log)"
ok "installer exits 0 for the release package"
after_directory="$(describe_usr_local_bin)"
measured "/usr/local/bin before the install: ${before_directory}; after: ${after_directory}"
assert_equal "the installed file has the release binary SHA-256" "$(sha256_of "$installed")" "$binary_hash"
assert_equal "the installed file's owner and mode" "$(describe_owner_and_mode "$installed")" "root:wheel 755"
assert_equal "the installed binary prints its version" "$("$installed" --version)" "marshal ${release_version}"
assert_equal "pkgutil --pkg-info reports the mapped version" "$(receipt_version)" "$installer_version"
measured "pkgutil --file-info ${installed}: $(pkgutil --file-info "$installed" | tr '\n' ' ')"
login_path="$(env -i HOME="$HOME" /bin/zsh -l -c 'command -v marshal')"
assert_equal "a login zsh with a clean environment finds marshal by name" "$login_path" "$installed"
assert_equal "that marshal prints its version" "$(env -i HOME="$HOME" /bin/zsh -l -c 'marshal --version')" "marshal ${release_version}"

echo "== the uninstall block =="
uninstall_block="$(bash "$extract_block" "$cli_document" macos-uninstall)"
uninstall_status=0
uninstall_output="$(bash -c "$uninstall_block" 2>&1)" || uninstall_status=$?
echo "$uninstall_output"
[[ $uninstall_status -eq 0 ]] || fail "the uninstall block of the CLI document exited ${uninstall_status}"
ok "the uninstall block of the CLI document exits 0"
[[ ! -e "$installed" ]] || fail "${installed} remains after the uninstall"
ok "the uninstall removes the file"
if has_receipt; then fail "the receipt remains after the uninstall"; fi
ok "the uninstall removes the receipt"
assert_true "the uninstall leaves ~/Library/Logs/marshal" test -f "$logs_marker"
measured "/usr/local/bin after the uninstall: $(describe_usr_local_bin)"

echo "== fixtures =="
printf '\nfixture-a' | cat "$binary" - >"${work}/a"
printf '\nfixture-b' | cat "$binary" - >"${work}/b"
hash_a="$(sha256_of "${work}/a")"
hash_b="$(sha256_of "${work}/b")"
[[ "$hash_a" != "$hash_b" ]] || fail "the two fixtures are equal"
[[ "$hash_a" != "$binary_hash" && "$hash_b" != "$binary_hash" ]] || fail "a fixture equals the release binary"
ok "the two fixtures differ from each other and from the release binary"
bash "$build_pkg" "${work}/b" 1.0.0 "${work}/b-1.0.0.pkg"
bash "$build_pkg" "${work}/a" 2.0.0 "${work}/a-2.0.0.pkg"

echo "== install, upgrade, lower version over higher =="
install_pkg "${work}/b-1.0.0.pkg" fixture-b-1.0.0-install || installer_failed fixture-b-1.0.0-install "installer exited non-zero for version 1.0.0"
assert_equal "version 1.0.0 is installed" "$(sha256_of "$installed")" "$hash_b"
assert_equal "the receipt reports 1.0.0" "$(receipt_version)" "1.0.0"
install_pkg "${work}/a-2.0.0.pkg" fixture-a-2.0.0-install || installer_failed fixture-a-2.0.0-install "installer exited non-zero for version 2.0.0"
assert_equal "the higher version replaced the file" "$(sha256_of "$installed")" "$hash_a"
assert_equal "the receipt reports 2.0.0" "$(receipt_version)" "2.0.0"
status=0
install_pkg "${work}/b-1.0.0.pkg" fixture-b-1.0.0-downgrade || status=$?
outcome="unknown"
case "$(sha256_of "$installed")" in
  "$hash_b") outcome="the lower version's file replaced the higher version's" ;;
  "$hash_a") outcome="the higher version's file stayed" ;;
esac
measured "a lower package over a higher one: installer exit status ${status}; ${outcome}; the receipt reports $(receipt_version)"
[[ "$outcome" != unknown ]] || fail "the file after the lower package is neither fixture"
ok "the outcome of a lower package over a higher one is recorded"

echo "All package installer checks passed."
