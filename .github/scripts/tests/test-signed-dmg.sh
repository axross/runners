#!/usr/bin/env bash
# checks the signed macOS disk image that release.yaml's sign-macos job produced,
# against the unsigned app archive the app job made, on a runner that holds no
# signing identity. verify-macos runs it, after it has checked the archive against
# app_sha256 and the disk image against sign-macos's hash; it first runs for real at
# the first release, because a dry run cannot sign. test-signed-dmg-refusals.sh runs
# it against stand-ins and produces each refusal.
#
# it refuses unless all of these hold:
#   - the disk image verifies with codesign --verify --strict, under the identifier
#     app.axross.marshal.dmg, by a Developer ID Application certificate of <team id>
#     with a secure timestamp, and not ad hoc;
#   - stapler validates it, and spctl assesses it for opening, by its primary
#     signature, as notarized Developer ID;
#   - the notary log is of the submission sign-macos made, by its job ID;
#   - mounted read-only, its volume is named Marshal and holds only Marshal.app and
#     a link named Applications to /Applications, apart from .DS_Store, .fseventsd
#     and .Trashes;
#   - the app's files are the unsigned archive's, by path and type, apart from
#     Contents/_CodeSignature/, and its Info.plist values are the archive's;
#   - the app verifies with codesign --verify --strict, under the identifier
#     app.axross.marshal, by a Developer ID Application certificate of the same team
#     with the hardened runtime and a secure timestamp, and its designated
#     requirement is the default one for that identifier and team in form A (plain
#     codesign's) or form B (Xcode's, TN3127), which it prints and, under GitHub
#     Actions, outputs as designated_requirement;
#   - spctl accepts the app for execution.
# every tool is called through SYSTEM_BIN and SYSTEM_SBIN, whose defaults are the
# macOS paths. macOS ships bash 3.2, so this uses nothing newer. see
# docs/operations/app.md.
set -euo pipefail

if [[ $# -ne 6 ]]; then
  echo "Usage: test-signed-dmg.sh <version> <unsigned app archive> <signed disk image> <notary log> <team id> <submission id>" >&2
  exit 2
fi
version="$1"
archive="$2"
dmg="$3"
notary_log="$4"
team="$5"
submission="$6"

: "${SYSTEM_BIN:=/usr/bin}"
: "${SYSTEM_SBIN:=/usr/sbin}"

identifier="app.axross.marshal"
dmg_identifier="app.axross.marshal.dmg"

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

# lowercase <text>
lowercase() {
  printf '%s' "$1" | tr '[:upper:]' '[:lower:]'
}

team_pattern='^[A-Z0-9]{10}$'
[[ "$team" =~ $team_pattern ]] || fail "'${team}' is not a 10-character team ID"
submission_pattern='^[0-9A-Fa-f-]{36}$'
[[ "$submission" =~ $submission_pattern ]] || fail "'${submission}' is not a notary submission ID"
[[ "$version" == *[!0-9A-Za-z.-]* || -z "$version" ]] && fail "'${version}' is not a release version"
for file in "$archive" "$dmg" "$notary_log"; do
  [[ -f "$file" ]] || fail "${file} is not a file"
done

work="$(mktemp -d "${TMPDIR:-/tmp}/marshal-signed-dmg.XXXXXX")"
mount="${work}/mount"
# set once the disk image is attached, so a refusal before then detaches nothing.
attached=0
cleanup() {
  if [[ $attached -eq 1 ]]; then
    "$SYSTEM_BIN/hdiutil" detach "$mount" -force >/dev/null 2>&1 || true
  fi
  rm -rf -- "$work"
}
trap cleanup EXIT

# signature_details <path>: what codesign displays of its signature.
signature_details() {
  "$SYSTEM_BIN/codesign" --display --verbose=4 "$1" 2>&1 || fail "codesign cannot display $1"
}

# check_developer_id <what> <details> <identifier>: the identifier, a Developer ID
# Application chain to Apple's root, a secure timestamp and no ad-hoc signature.
check_developer_id() {
  if grep -q -E '^Signature=adhoc' <<<"$2"; then
    fail "the $1 has an ad-hoc signature"
  fi
  assert_matches "the $1's identifier" "$2" "^Identifier=${3//./\\.}\$"
  assert_matches "the $1 is signed by a Developer ID Application certificate" "$2" '^Authority=Developer ID Application: '
  assert_matches "the $1's certificate chain reaches Apple's root" "$2" '^Authority=Apple Root CA$'
  assert_equal "the $1's team" "$(sed -n 's/^TeamIdentifier=//p' <<<"$2")" "$team"
  assert_matches "the $1 has a secure timestamp" "$2" '^Timestamp='
}

# normalized_requirement <text>: a designated requirement without its /* ... */
# comments, with one space between tokens and the team ID unquoted, which are the
# only differences between what codesign prints and the forms below.
normalized_requirement() {
  printf '%s\n' "$1" |
    sed -e 's|/\*[^*]*\*/||g' -e 's/[[:space:]][[:space:]]*/ /g' -e 's/^ //' -e 's/ $//' \
      -e 's/\(subject\.OU] = \)"\([A-Z0-9]*\)"/\1\2/'
}

# requirement_clauses <normalized requirement>: its clauses, one per line and
# sorted, so that their order does not matter and an extra or missing one does.
requirement_clauses() {
  printf '%s\n' "$1" |
    awk '{ n = split($0, parts, / and /); for (i = 1; i <= n; i++) print parts[i] }' |
    LC_ALL=C sort
}

# app_files <bundle>: every path in it with its type, sorted, apart from the
# signature's own folder.
app_files() {
  (cd "$1" && find . \( -path ./Contents/_CodeSignature -prune \) -o \( -type l -exec printf 'l %s\n' {} + \) -o \( -type d -exec printf 'd %s\n' {} + \) -o \( -type f -exec printf 'f %s\n' {} + \)) |
    LC_ALL=C sort
}

echo "== the disk image =="
"$SYSTEM_BIN/codesign" --verify --strict "$dmg" || fail "codesign --verify --strict rejects the disk image"
ok "codesign --verify --strict accepts the disk image"
dmg_details="$(signature_details "$dmg")"
check_developer_id "disk image" "$dmg_details" "$dmg_identifier"
"$SYSTEM_BIN/xcrun" stapler validate "$dmg" || fail "stapler validate rejects the disk image: it is not stapled"
ok "stapler validate accepts the disk image"
assessment_status=0
assessment="$("$SYSTEM_SBIN/spctl" --assess --type open --context context:primary-signature -vv "$dmg" 2>&1)" || assessment_status=$?
echo "MEASURED: spctl --assess --type open (exit ${assessment_status}): $(tr '\n' ' ' <<<"$assessment")"
[[ $assessment_status -eq 0 ]] || fail "spctl does not accept the disk image (exit ${assessment_status}): ${assessment}"
assert_matches "spctl accepts the disk image" "$assessment" ': accepted$'
assert_matches "spctl names the notary service as the disk image's source" "$assessment" '^source=Notarized Developer ID$'

echo "== the notary log =="
log_job="$("$SYSTEM_BIN/plutil" -extract jobId raw -o - "$notary_log" 2>/dev/null)" || fail "the notary log has no jobId"
assert_equal "the notary log is of the submission sign-macos made" "$(lowercase "$log_job")" "$(lowercase "$submission")"

echo "== the mounted disk image =="
mkdir "$mount" "${work}/unsigned"
attached=1
"$SYSTEM_BIN/hdiutil" attach -readonly -nobrowse -noautoopen -mountpoint "$mount" "$dmg" >/dev/null || fail "hdiutil cannot attach the disk image read-only"
"$SYSTEM_SBIN/diskutil" info -plist "$mount" >"${work}/volume.plist" || fail "diskutil cannot describe the mounted volume"
volume="$("$SYSTEM_BIN/plutil" -extract VolumeName raw -o - "${work}/volume.plist" 2>/dev/null)" || fail "diskutil names no volume"
assert_equal "the volume's name" "$volume" "Marshal"
entries="$(cd "$mount" && find . -mindepth 1 -maxdepth 1 ! -name .DS_Store ! -name .fseventsd ! -name .Trashes -print | LC_ALL=C sort | tr '\n' ' ')"
assert_equal "the volume holds only Marshal.app and Applications" "$entries" "./Applications ./Marshal.app "
[[ -L "${mount}/Applications" ]] || fail "Applications on the volume is not a link"
assert_equal "Applications links to /Applications" "$(readlink "${mount}/Applications")" "/Applications"
app="${mount}/Marshal.app"
[[ -d "$app" && ! -L "$app" ]] || fail "Marshal.app on the volume is not a folder"

"$SYSTEM_BIN/ditto" -x -k "$archive" "${work}/unsigned" || fail "ditto cannot unpack the unsigned app archive"
unsigned_app="${work}/unsigned/Marshal.app"
[[ -d "$unsigned_app" ]] || fail "the unsigned app archive holds no Marshal.app"
unsigned_files="$(app_files "$unsigned_app")"
[[ -n "$unsigned_files" ]] || fail "the unsigned app holds no files"
assert_equal "the signed app's files are the unsigned app's" "$(app_files "$app")" "$unsigned_files"
signed_plist="$("$SYSTEM_BIN/plutil" -convert json -o - "${app}/Contents/Info.plist")" || fail "plutil cannot read the signed app's Info.plist"
unsigned_plist="$("$SYSTEM_BIN/plutil" -convert json -o - "${unsigned_app}/Contents/Info.plist")" || fail "plutil cannot read the unsigned app's Info.plist"
assert_equal "the signed app's Info.plist values are the unsigned app's" "$signed_plist" "$unsigned_plist"

echo "== the app =="
"$SYSTEM_BIN/codesign" --verify --strict "$app" || fail "codesign --verify --strict rejects the app"
ok "codesign --verify --strict accepts the app"
app_details="$(signature_details "$app")"
check_developer_id "app" "$app_details" "$identifier"
assert_matches "the app has the hardened runtime" "$app_details" '^CodeDirectory .*flags=0x[0-9a-f]+\([a-z,]*runtime[a-z,]*\)'
requirement="$("$SYSTEM_BIN/codesign" --display --requirements - "$app" 2>&1 | sed -n 's/^designated => //p')"
[[ -n "$requirement" ]] || fail "codesign printed no designated requirement for the app"
# form A: the plain codesign default, as a set of clauses; form B: the requirement
# Xcode writes for Developer ID code, whole with its order and "or" (Apple TN3127).
form_a="anchor apple generic and identifier \"${identifier}\" and certificate 1[field.1.2.840.113635.100.6.2.6] and certificate leaf[field.1.2.840.113635.100.6.1.13] and certificate leaf[subject.OU] = ${team}"
form_b="anchor apple generic and identifier \"${identifier}\" and (certificate leaf[field.1.2.840.113635.100.6.1.9] or certificate 1[field.1.2.840.113635.100.6.2.6] and certificate leaf[field.1.2.840.113635.100.6.1.13] and certificate leaf[subject.OU] = ${team})"
normalized="$(normalized_requirement "$requirement")"
if [[ "$normalized" == "$(normalized_requirement "$form_b")" ]]; then
  form=B
elif [[ "$(requirement_clauses "$normalized")" == "$(requirement_clauses "$(normalized_requirement "$form_a")")" ]]; then
  form=A
else
  fail "the app's designated requirement is not the default one for ${identifier} and team ${team}, in form A (plain codesign) or form B (Xcode, TN3127): ${requirement}"
fi
ok "the app's designated requirement is the default one for ${identifier} and team ${team} (form ${form})"
echo "MEASURED: the app's designated requirement matches form ${form}"
echo "MEASURED: the app's designated requirement: ${requirement}"
if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  echo "designated_requirement=${requirement}" >>"$GITHUB_OUTPUT"
fi
assessment_status=0
assessment="$("$SYSTEM_SBIN/spctl" --assess --type execute -vv "$app" 2>&1)" || assessment_status=$?
echo "MEASURED: spctl --assess --type execute (exit ${assessment_status}): $(tr '\n' ' ' <<<"$assessment")"
[[ $assessment_status -eq 0 ]] || fail "spctl does not accept the app for execution (exit ${assessment_status}): ${assessment}"
assert_matches "spctl accepts the app" "$assessment" ': accepted$'

echo "All signed disk image checks passed for ${version}."
