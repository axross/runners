#!/usr/bin/env bash
# checks the signed macOS package and bare binary that release.yaml's sign-macos
# job produced, against the unsigned package the build made, on a runner that
# holds no signing identity. verify-macos runs it; it first runs for real at the
# first release, because a dry run cannot sign. test-signed-pkg-refusals.sh runs
# it against stand-ins and produces each refusal.
#
# it refuses unless all of these hold:
#   - the unsigned package and binary are the files the build's SHA256SUMS lists;
#   - the signed package's identifier, version and install location equal the
#     unsigned package's, and the identifier is app.axross.marshal;
#   - the two bills of materials list the same paths with the same mode, owner
#     and group, which leaves out the binary's size;
#   - neither package holds Scripts;
#   - the binary in the signed package is byte-identical to the signed bare binary;
#   - the bare binary verifies with codesign --verify --strict, is signed by a
#     Developer ID Application certificate of <team id> with the hardened runtime
#     and a secure timestamp, and its designated requirement is exactly the
#     default one for this identifier and team;
#   - the package's signature is a Developer ID Installer chain of <team id> to
#     Apple's root, stapler validates it, and spctl assesses it as notarized
#     Developer ID;
#   - the notary log is of the submission sign-macos made, by its job ID, and
#     lists the bare binary's CDHash.
# it then installs the signed package with installer, runs codesign --verify and
# --version on the installed file, and removes it. every tool is called through
# SYSTEM_BIN and SYSTEM_SBIN, whose defaults are the macOS paths. macOS ships bash
# 3.2, so this uses nothing newer. see docs/operations/cli.md.
set -euo pipefail

if [[ $# -ne 6 ]]; then
  echo "Usage: test-signed-pkg.sh <version> <unsigned directory> <signed directory> <notary log> <team id> <submission id>" >&2
  exit 2
fi
version="$1"
unsigned_directory="$2"
signed_directory="$3"
notary_log="$4"
team="$5"
submission="$6"

: "${SYSTEM_BIN:=/usr/bin}"
: "${SYSTEM_SBIN:=/usr/sbin}"

identifier="app.axross.marshal"
# only the refusals test overrides the install path, to keep its stand-in installer
# away from the real /usr/local/bin.
installed="${MARSHAL_INSTALL_PATH:-/usr/local/bin/marshal}"
macos="marshal-${version}-macos-arm64"
macos_pkg="${macos}.pkg"
unsigned_binary="${unsigned_directory}/${macos}"
unsigned_pkg="${unsigned_directory}/${macos_pkg}"
unsigned_sums="${unsigned_directory}/SHA256SUMS"
signed_pkg="${signed_directory}/${macos_pkg}"
signed_binary="${signed_directory}/${macos}"

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

# assert_matches <what> <text> <extended regular expression>: the text has a line that matches.
assert_matches() {
  grep -q -E "$3" <<<"$2" || fail "$1: no line of the output matches ${3}:
$2"
  ok "$1"
}

sha256_of() {
  local line
  line="$("$SYSTEM_BIN/shasum" -a 256 "$1")"
  echo "${line%% *}"
}

team_pattern='^[A-Z0-9]{10}$'
[[ "$team" =~ $team_pattern ]] || fail "'${team}' is not a 10-character team ID"
submission_pattern='^[0-9A-Fa-f-]{36}$'
[[ "$submission" =~ $submission_pattern ]] || fail "'${submission}' is not a notary submission ID"
for file in "$unsigned_binary" "$unsigned_pkg" "$unsigned_sums" "$signed_pkg" "$signed_binary" "$notary_log"; do
  [[ -f "$file" ]] || fail "${file} is not a file"
done

work="$(mktemp -d "${TMPDIR:-/tmp}/marshal-signed-pkg.XXXXXX")"
# set once the signed package is about to be installed, so a refusal on a host
# that has marshal installed never removes it.
cleanup_armed=0
cleanup() {
  if [[ $cleanup_armed -eq 1 ]]; then
    "$SYSTEM_BIN/sudo" rm -f "$installed" || true
    "$SYSTEM_BIN/sudo" "$SYSTEM_SBIN/pkgutil" --forget "$identifier" >/dev/null 2>&1 || true
  fi
  rm -rf -- "$work"
}
trap cleanup EXIT

# package_attribute <PackageInfo> <attribute>: the attribute of the pkg-info element.
package_attribute() {
  tr '\n' ' ' <"$1" | grep -o '<pkg-info[^>]*>' | grep -o " $2=\"[^\"]*\"" | sed -e 's/^[^=]*="//' -e 's/"$//'
}

# listed_hash <name>: the hash the build's SHA256SUMS lists for the name, which
# exactly one line must.
listed_hash() {
  local lines
  lines="$(awk -v name="$1" '$2 == name { print $1 }' "$unsigned_sums")"
  if [[ -z "$lines" || "$lines" == *$'\n'* ]]; then
    fail "SHA256SUMS does not list $1 exactly once"
  fi
  echo "$lines"
}

# lowercase <text>
lowercase() {
  printf '%s' "$1" | tr '[:upper:]' '[:lower:]'
}

# bill_of_materials <Bom>: path, mode, user and group of every entry, sorted.
bill_of_materials() {
  paste <("$SYSTEM_BIN/lsbom" -p f "$1") <("$SYSTEM_BIN/lsbom" -p m "$1") <("$SYSTEM_BIN/lsbom" -p U "$1") <("$SYSTEM_BIN/lsbom" -p G "$1") | LC_ALL=C sort
}

# requirement_clauses <text>: the clauses of a designated requirement, one per
# line and sorted, without comments, parentheses or quotes, so that the order and
# the punctuation macOS prints do not matter, and an extra or missing clause does.
requirement_clauses() {
  printf '%s\n' "$1" |
    sed -e 's|/\* [a-z]* \*/||g' -e 's/[()"]//g' |
    awk '{ n = split($0, parts, / and /); for (i = 1; i <= n; i++) { gsub(/^ +| +$/, "", parts[i]); if (parts[i] != "") print parts[i] } }' |
    LC_ALL=C sort
}

echo "== the unsigned files =="
for name in "$macos" "$macos_pkg"; do
  want="$(listed_hash "$name")"
  assert_equal "the unsigned ${name} is the file SHA256SUMS lists" "$(sha256_of "${unsigned_directory}/${name}")" "$want"
done

echo "== the packages =="
"$SYSTEM_SBIN/pkgutil" --expand-full "$unsigned_pkg" "${work}/unsigned" || fail "pkgutil cannot expand the unsigned package"
"$SYSTEM_SBIN/pkgutil" --expand-full "$signed_pkg" "${work}/signed" || fail "pkgutil cannot expand the signed package"
for attribute in identifier version install-location; do
  unsigned_value="$(package_attribute "${work}/unsigned/PackageInfo" "$attribute")"
  signed_value="$(package_attribute "${work}/signed/PackageInfo" "$attribute")"
  [[ -n "$unsigned_value" ]] || fail "the unsigned package's PackageInfo has no ${attribute}"
  assert_equal "the signed package's ${attribute} equals the unsigned package's" "$signed_value" "$unsigned_value"
done
assert_equal "the signed package's identifier" "$(package_attribute "${work}/signed/PackageInfo" identifier)" "$identifier"
assert_equal "the signed package's install location" "$(package_attribute "${work}/signed/PackageInfo" install-location)" "/"
assert_equal "the signed package's version is the release's" "$(package_attribute "${work}/signed/PackageInfo" version)" "$version"
for which in unsigned signed; do
  scripts_found="$(find "${work}/${which}" -iname 'Scripts*' | tr '\n' ' ')"
  assert_equal "the ${which} package holds no Scripts" "$scripts_found" ""
done
unsigned_bom="$(bill_of_materials "${work}/unsigned/Bom")"
signed_bom="$(bill_of_materials "${work}/signed/Bom")"
[[ -n "$signed_bom" ]] || fail "the signed package's bill of materials is empty"
assert_equal "the two bills of materials list the same paths, modes, owners and groups" "$signed_bom" "$unsigned_bom"
payload_binary="${work}/signed/Payload/usr/local/bin/marshal"
[[ -f "$payload_binary" ]] || fail "the signed package's payload holds no usr/local/bin/marshal"
assert_equal "the binary in the signed package is the signed bare binary" "$(sha256_of "$payload_binary")" "$(sha256_of "$signed_binary")"

echo "== the bare binary =="
"$SYSTEM_BIN/codesign" --verify --strict "$signed_binary" || fail "codesign --verify --strict rejects the signed binary"
ok "codesign --verify --strict accepts the signed binary"
details="$("$SYSTEM_BIN/codesign" --display --verbose=4 "$signed_binary" 2>&1)" || fail "codesign cannot display the signed binary"
if grep -q -E '^Signature=adhoc' <<<"$details"; then
  fail "the signed binary has an ad-hoc signature"
fi
assert_matches "the binary's identifier" "$details" "^Identifier=${identifier}\$"
assert_matches "the binary is signed by a Developer ID Application certificate" "$details" '^Authority=Developer ID Application: '
assert_matches "the binary's certificate chain reaches Apple's root" "$details" '^Authority=Apple Root CA$'
teams="$(sed -n 's/^TeamIdentifier=//p' <<<"$details")"
assert_equal "the binary's team" "$teams" "$team"
assert_matches "the binary has the hardened runtime" "$details" '^CodeDirectory .*flags=0x[0-9a-f]+\([a-z,]*runtime[a-z,]*\)'
assert_matches "the binary has a secure timestamp" "$details" '^Timestamp='
requirement="$("$SYSTEM_BIN/codesign" --display --requirements - "$signed_binary" 2>&1 | sed -n 's/^designated => //p')"
[[ -n "$requirement" ]] || fail "codesign printed no designated requirement"
if grep -q ' or ' <<<"$requirement"; then
  fail "the designated requirement has an alternative: ${requirement}"
fi
expected_requirement="$(requirement_clauses "anchor apple generic and identifier \"${identifier}\" and certificate 1[field.1.2.840.113635.100.6.2.6] and certificate leaf[field.1.2.840.113635.100.6.1.13] and certificate leaf[subject.OU] = ${team}")"
assert_equal "the designated requirement is the default one for ${identifier} and team ${team}" "$(requirement_clauses "$requirement")" "$expected_requirement"
echo "MEASURED: the signed binary's designated requirement: ${requirement}"

echo "== the signed package =="
signature="$("$SYSTEM_SBIN/pkgutil" --check-signature "$signed_pkg" 2>&1)" || fail "pkgutil --check-signature rejects the signed package:
${signature}"
echo "$signature"
assert_matches "the package is signed by a developer certificate issued by Apple" "$signature" 'Status: signed by a developer certificate issued by Apple for distribution'
assert_matches "the package's chain starts at a Developer ID Installer certificate" "$signature" '^ *1\. Developer ID Installer: '
assert_matches "the package's chain reaches Apple's root" "$signature" 'Apple Root CA'
package_teams="$(sed -n 's/^ *1\. Developer ID Installer: .*(\([A-Z0-9]*\))$/\1/p' <<<"$signature")"
[[ "$package_teams" == "$teams" ]] || fail "the binary names team '${teams}' but the package names team '${package_teams}'"
assert_equal "the package's team" "$package_teams" "$team"
"$SYSTEM_BIN/xcrun" stapler validate "$signed_pkg" || fail "stapler validate rejects the package: it is not stapled"
ok "stapler validate accepts the package"
assessment_status=0
assessment="$("$SYSTEM_SBIN/spctl" --assess --type install -vv "$signed_pkg" 2>&1)" || assessment_status=$?
echo "MEASURED: spctl --assess --type install -vv (exit ${assessment_status}): $(tr '\n' ' ' <<<"$assessment")"
[[ $assessment_status -eq 0 ]] || fail "spctl does not accept the package (exit ${assessment_status}): ${assessment}"
assert_matches "spctl accepts the package" "$assessment" ': accepted$'
assert_matches "spctl names the notary service as the source" "$assessment" '^source=Notarized Developer ID$'

echo "== the notary log =="
log_job="$("$SYSTEM_BIN/plutil" -extract jobId raw -o - "$notary_log" 2>/dev/null)" || fail "the notary log has no jobId"
assert_equal "the notary log is of the submission sign-macos made" "$(lowercase "$log_job")" "$(lowercase "$submission")"
cdhash="$(sed -n 's/^CDHash=//p' <<<"$details")"
cdhash_pattern='^[0-9a-f]{40}$'
[[ "$cdhash" =~ $cdhash_pattern ]] || fail "codesign printed no CDHash for the signed binary"
grep -q -i -F -e "$cdhash" "$notary_log" || fail "the notary log does not list the binary's CDHash ${cdhash}"
ok "the notary log lists the binary's CDHash ${cdhash}"

echo "== installing the signed package =="
"$SYSTEM_BIN/sudo" -n true || fail "sudo asks for a password"
[[ ! -e "$installed" && ! -L "$installed" ]] || fail "${installed} exists before the install"
if "$SYSTEM_SBIN/pkgutil" --pkg-info "$identifier" >/dev/null 2>&1; then
  fail "a receipt for ${identifier} exists before the install"
fi
cleanup_armed=1
"$SYSTEM_BIN/sudo" "$SYSTEM_SBIN/installer" -pkg "$signed_pkg" -target / || fail "installer rejects the signed package"
ok "installer installs the signed package"
"$SYSTEM_BIN/codesign" --verify --strict "$installed" || fail "codesign --verify --strict rejects the installed file"
ok "codesign --verify --strict accepts the installed file"
assert_equal "the installed file is the signed bare binary" "$(sha256_of "$installed")" "$(sha256_of "$signed_binary")"
assert_equal "the installed binary prints its version" "$("$installed" --version)" "marshal ${version}"

echo "All signed package checks passed."
