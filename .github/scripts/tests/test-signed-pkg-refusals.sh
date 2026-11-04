#!/usr/bin/env bash
# runs test-signed-pkg.sh against recording stand-ins of codesign, pkgutil, lsbom,
# xcrun, spctl, installer, sudo, plutil and shasum, and produces each refusal: a payload
# that differs, another identifier, a version or install location that differs, a
# bill of materials that differs, Scripts, two teams, an unsigned or ad-hoc
# binary, a missing hardened runtime or timestamp, a designated requirement that
# is not the default one, an assessment spctl does not accept, an unstapled
# package, a missing Installer signature, a notary log of another submission or
# that lacks the binary's CDHash, and unsigned files that SHA256SUMS does not list. a positive control passes first, so each refusal is shown to be
# the check named and not a failure the script always has, and every run is
# checked for what the stand-ins saw: nothing is installed after a refusal before
# the install, and the install is removed after a pass and after a failure.
# the stand-in packages and binaries are text files whose `#@ key=value` lines say
# what the stand-in tools report; a later line overrides an earlier one.
set -euo pipefail

script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/test-signed-pkg.sh"
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT

version=1.2.3
team=ABCDE12345
other_team=ZZZZZ99999
identifier=app.axross.marshal
cdhash=0123456789abcdef0123456789abcdef01234567
submission=8c4f2a1e-0b3d-4a6e-9f7c-1d2e3f4a5b6c
other_submission=0d9e8f7a-6b5c-4d3e-8a2b-1c0d9e8f7a6b
macos="marshal-${version}-macos-arm64"
macos_pkg="${macos}.pkg"
requirement="identifier \"${identifier}\" and anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] /* exists */ and certificate leaf[field.1.2.840.113635.100.6.1.13] /* exists */ and certificate leaf[subject.OU] = ${team}"

mkdir -p "${work}/bin" "${work}/sbin"
# one stand-in answers to every tool's name.
cat >"${work}/standin" <<'STANDIN'
#!/usr/bin/env bash
set -euo pipefail
tool="$(basename "$0")"
echo "${tool} $*" >>"$RECORD"

# meta <file> <key>: the last `#@ key=value` line of the file.
meta() {
  { sed '/^#@ payload-follows$/,$d' "$1" | grep "^#@ $2=" || true; } | tail -n 1 | sed "s/^#@ $2=//"
}
last_argument() {
  local argument
  for argument in "$@"; do :; done
  echo "$argument"
}

case "$tool" in
  shasum)
    shift 2
    sha256sum "$@"
    ;;
  sudo)
    if [[ "${1:-}" == -n ]]; then
      shift
      if [[ "${1:-}" == true && -e "${RECORD}.password" ]]; then
        echo "sudo: a password is required" >&2
        exit 1
      fi
    fi
    "$@"
    ;;
  codesign)
    file="$(last_argument "$@")"
    case "$1" in
      --verify)
        if [[ "$(meta "$file" valid)" != 1 ]]; then
          echo "${file}: code object is not signed at all" >&2
          exit 1
        fi
        ;;
      --display)
        if [[ "$(meta "$file" display_fails)" == 1 ]]; then
          echo "${file}: cannot display" >&2
          exit 1
        fi
        if [[ "$2" == --requirements ]]; then
          echo "Executable=${file}" >&2
          echo "designated => $(meta "$file" requirement)"
        else
          {
            echo "Executable=${file}"
            echo "Identifier=$(meta "$file" identifier)"
            echo "Format=Mach-O thin (arm64)"
            if [[ "$(meta "$file" runtime)" == 1 ]]; then
              echo "CodeDirectory v=20500 size=1000 flags=0x10000(runtime) hashes=24+7 location=embedded"
            else
              echo "CodeDirectory v=20400 size=1000 flags=0x0(none) hashes=24+7 location=embedded"
            fi
            if [[ "$(meta "$file" adhoc)" == 1 ]]; then
              echo "Signature=adhoc"
            else
              echo "Signature size=9000"
              if [[ -n "$(meta "$file" authority)" ]]; then
                echo "Authority=$(meta "$file" authority)"
                echo "Authority=Developer ID Certification Authority"
                if [[ "$(meta "$file" rootless)" != 1 ]]; then
                  echo "Authority=Apple Root CA"
                fi
              fi
            fi
            if [[ "$(meta "$file" timestamp)" == 1 ]]; then
              echo "Timestamp=Nov 3, 2026 at 10:00:00"
            else
              echo "Signed Time=Nov 3, 2026 at 10:00:00"
            fi
            echo "TeamIdentifier=$(meta "$file" team)"
            echo "CDHash=$(meta "$file" cdhash)"
          } >&2
        fi
        ;;
    esac
    ;;
  pkgutil)
    case "$1" in
      --expand-full)
        package="$2"
        directory="$3"
        if [[ "$(meta "$package" expand_fails)" == 1 ]]; then
          echo "pkgutil: cannot expand" >&2
          exit 1
        fi
        mkdir -p "${directory}/Payload/usr/local/bin"
        echo "<pkg-info overwrite-permissions=\"true\" identifier=\"$(meta "$package" identifier)\" version=\"$(meta "$package" version)\" generator-version=\"stand-in\" install-location=\"$(meta "$package" location)\" auth=\"root\">" >"${directory}/PackageInfo"
        awk '/^#@ payload-follows$/ { exit } /^#@ bomreset=1$/ { n = 0; next } /^#@ bom=/ { lines[++n] = substr($0, 9) } END { for (i = 1; i <= n; i++) print lines[i] }' "$package" >"${directory}/Bom"
        if [[ "$(meta "$package" scripts)" == 1 ]]; then
          mkdir "${directory}/Scripts"
        fi
        sed '1,/^#@ payload-follows$/d' "$package" >"${directory}/Payload/usr/local/bin/marshal"
        if [[ "$(meta "$package" nopayload)" == 1 ]]; then
          rm "${directory}/Payload/usr/local/bin/marshal"
        fi
        ;;
      --check-signature)
        case "$(meta "$2" signature)" in
          installer)
            echo "Package \"$(basename "$2")\":"
            echo "   Status: signed by a developer certificate issued by Apple for distribution"
            echo "   Certificate Chain:"
            echo "    1. Developer ID Installer: Test Maintainer ($(meta "$2" team))"
            echo "    2. Developer ID Certification Authority"
            echo "    3. Apple Root CA"
            ;;
          untrusted)
            echo "Package \"$(basename "$2")\":"
            echo "   Status: signed by a certificate that is not trusted"
            echo "   Certificate Chain:"
            echo "    1. Developer ID Installer: Test Maintainer ($(meta "$2" team))"
            echo "    2. Developer ID Certification Authority"
            echo "    3. Apple Root CA"
            ;;
          rootless)
            echo "Package \"$(basename "$2")\":"
            echo "   Status: signed by a developer certificate issued by Apple for distribution"
            echo "   Certificate Chain:"
            echo "    1. Developer ID Installer: Test Maintainer ($(meta "$2" team))"
            echo "    2. Developer ID Certification Authority"
            ;;
          other)
            echo "Package \"$(basename "$2")\":"
            echo "   Status: signed by a developer certificate issued by Apple for distribution"
            echo "   Certificate Chain:"
            echo "    1. Mac Developer Installer: Test Maintainer ($(meta "$2" team))"
            echo "    2. Apple Root CA"
            ;;
          *)
            echo "Package \"$(basename "$2")\":"
            echo "   Status: no signature"
            exit 1
            ;;
        esac
        ;;
      --pkg-info)
        [[ -e "$RECEIPT" ]]
        ;;
      --forget)
        rm -f "$RECEIPT"
        ;;
    esac
    ;;
  plutil)
    # plutil -extract <key> raw -o - <file>
    key="$2"
    value="$(sed -n "s/.*\"${key}\" *: *\"\([^\"]*\)\".*/\1/p" "${@: -1}" | head -n 1)"
    [[ -n "$value" ]] || exit 1
    echo "$value"
    ;;
  lsbom)
    field="$2"
    column=1
    case "$field" in
      f) column=1 ;;
      m) column=2 ;;
      U) column=3 ;;
      G) column=4 ;;
    esac
    cut -d ';' -f "$column" "$3"
    ;;
  xcrun)
    # xcrun stapler validate <pkg>
    if [[ "$(meta "$3" stapled)" != 1 ]]; then
      echo "Processing: $3" >&2
      echo "$3 does not have a ticket stapled to it." >&2
      exit 65
    fi
    echo "The validate action worked!"
    ;;
  spctl)
    package="${@: -1}"
    case "$(meta "$package" assess)" in
      silent)
        echo "source=Notarized Developer ID" >&2
        ;;
      notarized)
        echo "${package}: accepted" >&2
        echo "source=Notarized Developer ID" >&2
        echo "origin=Developer ID Installer: Test Maintainer ($(meta "$package" team))" >&2
        ;;
      developer-id)
        echo "${package}: accepted" >&2
        echo "source=Developer ID" >&2
        ;;
      *)
        echo "${package}: rejected" >&2
        echo "source=Unnotarized Developer ID" >&2
        exit 3
        ;;
    esac
    ;;
  installer)
    if [[ "$(meta "$2" installer_fails)" == 1 ]]; then
      echo "installer: The install failed." >&2
      exit 1
    fi
    mkdir -p "$(dirname "$MARSHAL_INSTALL_PATH")"
    sed '1,/^#@ payload-follows$/d' "$2" >"$MARSHAL_INSTALL_PATH"
    case "$(meta "$2" installs)" in
      unsigned) echo '#@ valid=0' >>"$MARSHAL_INSTALL_PATH" ;;
      other) echo '# another file' >>"$MARSHAL_INSTALL_PATH" ;;
    esac
    chmod +x "$MARSHAL_INSTALL_PATH"
    : >"$RECEIPT"
    ;;
esac
STANDIN
chmod +x "${work}/standin"
for tool in codesign lsbom plutil shasum sudo xcrun; do
  ln -s "${work}/standin" "${work}/bin/${tool}"
done
for tool in installer pkgutil spctl; do
  ln -s "${work}/standin" "${work}/sbin/${tool}"
done

failures=0

# make_binary <file> <extra meta lines>: a stand-in bare binary that signed as the
# release signs it.
make_binary() {
  {
    printf '#!/usr/bin/env bash\n'
    # the expressions are the stand-in binary's own, so they stay literal.
    # shellcheck disable=SC2016
    printf 'if [[ "${1:-}" == --version ]]; then echo "marshal ${FAKE_VERSION:-%s}"; fi\n' "$version"
    printf '#@ valid=1\n#@ identifier=%s\n#@ team=%s\n' "$identifier" "$team"
    printf '#@ authority=Developer ID Application: Test Maintainer (%s)\n' "$team"
    printf '#@ runtime=1\n#@ timestamp=1\n#@ adhoc=0\n#@ cdhash=%s\n#@ requirement=%s\n' "$cdhash" "$requirement"
    printf '%s' "$2"
  } >"$1"
}

# write_sums <unsigned directory> <change>: the build's SHA256SUMS, as it lists
# the two macOS files and a Windows one; the change spoils one line, drops the
# package's, lists it twice, or removes the file.
write_sums() {
  local directory="$1" change="$2"
  (
    cd "$directory"
    printf 'windows binary\n' >"marshal-${version}-windows-x64.exe"
    sha256sum "$macos" "$macos_pkg" "marshal-${version}-windows-x64.exe" >SHA256SUMS
    rm "marshal-${version}-windows-x64.exe"
    case "$change" in
      "") ;;
      wrong-pkg) sed -i "s/^[0-9a-f]\{64\}  ${macos_pkg}\$/$(printf '0%.0s' {1..64})  ${macos_pkg}/" SHA256SUMS ;;
      wrong-binary) sed -i "s/^[0-9a-f]\{64\}  ${macos}\$/$(printf '0%.0s' {1..64})  ${macos}/" SHA256SUMS ;;
      no-pkg) grep -v " ${macos_pkg}\$" SHA256SUMS >sums && mv sums SHA256SUMS ;;
      twice) grep " ${macos_pkg}\$" SHA256SUMS >line && cat line >>SHA256SUMS && rm line ;;
      none) rm SHA256SUMS ;;
      *) echo "error: unknown change ${change}" >&2; exit 1 ;;
    esac
  )
}

# make_package <file> <binary file> <extra meta lines>
make_package() {
  {
    printf '#@ identifier=%s\n#@ version=%s\n#@ location=/\n#@ scripts=0\n' "$identifier" "$version"
    printf '#@ signature=installer\n#@ team=%s\n#@ stapled=1\n#@ assess=notarized\n' "$team"
    printf '#@ bom=.;40755;root;wheel\n#@ bom=./usr/local/bin;40755;root;wheel\n#@ bom=./usr/local/bin/marshal;100755;root;wheel\n'
    printf '%s' "$3"
    printf '#@ payload-follows\n'
    cat "$2"
  } >"$1"
}

# run_case <name> <exit status> <text the output must contain> <installer ran: yes|no>
# builds the files from the variables below, runs the script, and checks the
# result and what the stand-ins saw. the variables are reset after each case.
bin_meta=""
unsigned_meta=""
signed_meta=""
payload_extra=""
log_text=""
team_argument="$team"
password_asked=0
receipt_before=0
fake_version=""
remove_notary=0
sums_change=""
submission_argument="$submission"
run_case() {
  local name="$1" want_status="$2" want_text="$3" want_install="$4" case_directory output status=0 problem=""
  case_directory="${work}/case-${name//[^a-zA-Z0-9]/-}"
  mkdir -p "${case_directory}/unsigned" "${case_directory}/signed" "${case_directory}/notary"
  make_binary "${case_directory}/unsigned/${macos}" ""
  make_package "${case_directory}/unsigned/${macos_pkg}" "${case_directory}/unsigned/${macos}" "${unsigned_meta}"
  write_sums "${case_directory}/unsigned" "$sums_change"
  make_binary "${case_directory}/signed/${macos}" "$bin_meta"
  cp "${case_directory}/signed/${macos}" "${case_directory}/signed/payload"
  printf '%s' "$payload_extra" >>"${case_directory}/signed/payload"
  make_package "${case_directory}/signed/${macos_pkg}" "${case_directory}/signed/payload" "$signed_meta"
  rm "${case_directory}/signed/payload"
  if [[ -z "$log_text" ]]; then
    printf '{"logFormatVersion":1,"jobId":"%s","status":"Accepted","ticketContents":[{"path":"x.pkg/Payload/usr/local/bin/marshal","digestAlgorithm":"SHA-256","cdhash":"%s","arch":"arm64"}]}\n' "$submission" "$cdhash" >"${case_directory}/notary/log.json"
  else
    printf '%s\n' "$log_text" >"${case_directory}/notary/log.json"
  fi
  : >"${case_directory}/record"
  if [[ $password_asked -eq 1 ]]; then : >"${case_directory}/record.password"; fi
  if [[ $receipt_before -eq 1 ]]; then : >"${case_directory}/receipt"; fi
  if [[ $remove_notary -eq 1 ]]; then rm "${case_directory}/notary/log.json"; fi
  output="$(cd "$case_directory" && RECORD="${case_directory}/record" RECEIPT="${case_directory}/receipt" FAKE_VERSION="$fake_version" \
    MARSHAL_INSTALL_PATH="${case_directory}/install/marshal" SYSTEM_BIN="${work}/bin" SYSTEM_SBIN="${work}/sbin" \
    bash "$script" "$version" unsigned signed notary/log.json "$team_argument" "$submission_argument" 2>&1)" || status=$?
  if [[ $status -ne $want_status ]]; then
    problem="exit ${status}, expected ${want_status}: ${output}"
  elif [[ "$output" != *"$want_text"* ]]; then
    problem="the output does not contain '${want_text}': ${output}"
  elif [[ "$want_install" == yes ]] && ! grep -q '^installer ' "${case_directory}/record"; then
    problem="the installer did not run"
  elif [[ "$want_install" == no ]] && grep -q '^installer ' "${case_directory}/record"; then
    problem="the installer ran although the case is refused before the install"
  elif [[ -e "${case_directory}/install/marshal" ]]; then
    problem="the installed file was left behind"
  elif [[ $receipt_before -eq 1 ]] && { [[ ! -e "${case_directory}/receipt" ]] || grep -q -e '--forget' "${case_directory}/record"; }; then
    problem="a receipt that was there before the run was forgotten"
  elif [[ $receipt_before -eq 0 && -e "${case_directory}/receipt" ]]; then
    problem="the receipt was left behind"
  fi
  if [[ -n "$problem" ]]; then
    echo "FAIL ${name}: ${problem}" >&2
    failures=$((failures + 1))
  else
    echo "ok   ${name}"
  fi
  bin_meta=""
  unsigned_meta=""
  signed_meta=""
  payload_extra=""
  log_text=""
  team_argument="$team"
  password_asked=0
  receipt_before=0
  fake_version=""
  remove_notary=0
  sums_change=""
  submission_argument="$submission"
}

run_case "a genuine signed set" 0 "All signed package checks passed." yes

# a different arrangement of the same requirement is the same requirement.
requirement_saved="$requirement"
requirement="anchor apple generic and identifier \"${identifier}\" and (certificate leaf[subject.OU] = \"${team}\" and certificate leaf[field.1.2.840.113635.100.6.1.13] and certificate 1[field.1.2.840.113635.100.6.2.6])"
run_case "the same requirement in another order and punctuation" 0 "All signed package checks passed." yes
requirement="$requirement_saved"

payload_extra=$'\n# one byte more\n'
run_case "a payload that differs from the signed binary" 1 "the binary in the signed package is the signed bare binary" no

signed_meta=$'#@ identifier=io.github.example.marshal\n'
run_case "a package identifier other than the unsigned package's" 1 "the signed package's identifier equals the unsigned package's" no
unsigned_meta=$'#@ identifier=io.github.example.marshal\n'
signed_meta=$'#@ identifier=io.github.example.marshal\n'
run_case "an identifier that is not app.axross.marshal in both packages" 1 "the signed package's identifier: got 'io.github.example.marshal'" no
signed_meta=$'#@ version=1.2.4\n'
run_case "a version that differs" 1 "the signed package's version equals the unsigned package's" no
signed_meta=$'#@ location=/usr/local\n'
run_case "an install location that differs" 1 "the signed package's install-location equals the unsigned package's" no
signed_meta=$'#@ version=9.9.9\n'
unsigned_meta=$'#@ version=9.9.9\n'
run_case "a version that is the same in both packages but not the release's" 1 "the signed package's version is the release's" no
signed_meta=$'#@ location=/usr\n'
unsigned_meta=$'#@ location=/usr\n'
run_case "an install location that is the same in both packages but not /" 1 "the signed package's install location" no
signed_meta=$'#@ location=\n'
unsigned_meta=$'#@ location=\n'
run_case "an install location that neither package has" 1 "the unsigned package's PackageInfo has no install-location" no
unsigned_meta=$'#@ expand_fails=1\n'
run_case "an unsigned package pkgutil cannot expand" 1 "pkgutil cannot expand the unsigned package" no
signed_meta=$'#@ expand_fails=1\n'
run_case "a signed package pkgutil cannot expand" 1 "pkgutil cannot expand the signed package" no
signed_meta=$'#@ nopayload=1\n'
run_case "a signed package whose payload lacks the binary" 1 "the signed package's payload holds no usr/local/bin/marshal" no
remove_notary=1
run_case "a notary log that is missing" 1 "notary/log.json is not a file" no
signed_meta=$'#@ bom=./usr/local/bin/marshal-extra;100755;root;wheel\n'
run_case "an extra path in the bill of materials" 1 "the two bills of materials list the same" no
signed_meta=$'#@ bom=./usr/local/bin/marshal;100755;root;wheel\n'
run_case "a duplicated entry in the bill of materials" 1 "the two bills of materials list the same" no
signed_meta=$'#@ bomreset=1\n#@ bom=.;40755;root;wheel\n#@ bom=./usr/local/bin;40755;root;wheel\n#@ bom=./usr/local/bin/marshal;100644;root;wheel\n'
run_case "a binary with another mode in the bill of materials" 1 "the two bills of materials list the same" no
signed_meta=$'#@ bomreset=1\n#@ bom=.;40755;root;wheel\n#@ bom=./usr/local/bin;40755;root;wheel\n#@ bom=./usr/local/bin/marshal;100755;axross;staff\n'
run_case "a binary with another owner in the bill of materials" 1 "the two bills of materials list the same" no
signed_meta=$'#@ bomreset=1\n'
run_case "an empty bill of materials" 1 "the signed package's bill of materials is empty" no
signed_meta=$'#@ scripts=1\n'
run_case "Scripts in the signed package" 1 "the signed package holds no Scripts" no
unsigned_meta=$'#@ scripts=1\n'
run_case "Scripts in the unsigned package" 1 "the unsigned package holds no Scripts" no

bin_meta=$'#@ valid=0\n#@ authority=\n'
run_case "an unsigned binary" 1 "codesign --verify --strict rejects the signed binary" no
bin_meta=$'#@ adhoc=1\n#@ authority=\n#@ runtime=0\n#@ timestamp=0\n'
run_case "an ad-hoc binary" 1 "the signed binary has an ad-hoc signature" no
bin_meta=$'#@ authority=Apple Development: Test Maintainer (ABCDE12345)\n'
run_case "a binary signed by another kind of certificate" 1 "the binary is signed by a Developer ID Application certificate" no
bin_meta=$'#@ display_fails=1\n'
run_case "a binary codesign cannot display" 1 "codesign cannot display the signed binary" no
bin_meta=$'#@ rootless=1\n'
run_case "a binary whose chain stops short of Apple's root" 1 "the binary's certificate chain reaches Apple's root" no
bin_meta=$'#@ runtime=0\n'
run_case "a binary without the hardened runtime" 1 "the binary has the hardened runtime" no
bin_meta=$'#@ timestamp=0\n'
run_case "a binary without a secure timestamp" 1 "the binary has a secure timestamp" no
bin_meta=$'#@ identifier=marshal-1.2.3-macos-arm64\n'
run_case "a binary signed under its file name" 1 "the binary's identifier" no
bin_meta=$'#@ team=ZZZZZ99999\n'
run_case "a binary of another team" 1 "the binary's team: got 'ZZZZZ99999'" no
signed_meta=$'#@ team=ZZZZZ99999\n'
run_case "two teams: the package's differs from the binary's" 1 "the binary names team 'ABCDE12345' but the package names team 'ZZZZZ99999'" no
team_argument="$other_team"
run_case "a team other than the one sign-macos output" 1 "the binary's team: got 'ABCDE12345', expected 'ZZZZZ99999'" no
team_argument="not-a-team"
run_case "a team ID that is not a team ID" 1 "'not-a-team' is not a 10-character team ID" no

bin_meta=$'#@ requirement=\n'
run_case "a binary with no designated requirement" 1 "codesign printed no designated requirement" no
bin_meta=$'#@ requirement=identifier "app.axross.other" and anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] and certificate leaf[field.1.2.840.113635.100.6.1.13] and certificate leaf[subject.OU] = ABCDE12345\n'
run_case "a designated requirement for another identifier" 1 "the designated requirement is the default one" no
bin_meta=$'#@ requirement=identifier "app.axross.marshal" and anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] and certificate leaf[field.1.2.840.113635.100.6.1.13] and certificate leaf[subject.OU] = ZZZZZ99999\n'
run_case "a designated requirement for another team" 1 "the designated requirement is the default one" no
bin_meta=$'#@ requirement=identifier "app.axross.marshal" and anchor apple generic and certificate leaf[subject.OU] = ABCDE12345\n'
run_case "a designated requirement missing the Developer ID markers" 1 "the designated requirement is the default one" no
bin_meta=$'#@ requirement=identifier "app.axross.marshal" and anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] and certificate leaf[field.1.2.840.113635.100.6.1.13] and certificate leaf[subject.OU] = ABCDE12345 and info[x] = y\n'
run_case "a designated requirement with an extra clause" 1 "the designated requirement is the default one" no
bin_meta=$'#@ requirement=identifier "app.axross.marshal" and anchor apple generic and (certificate leaf[subject.OU] = ABCDE12345 or certificate 1[field.1.2.840.113635.100.6.2.6] and certificate leaf[field.1.2.840.113635.100.6.1.13])\n'
run_case "a designated requirement with an alternative" 1 "the designated requirement has an alternative" no

signed_meta=$'#@ signature=none\n'
run_case "a package with no signature" 1 "pkgutil --check-signature rejects the signed package" no
signed_meta=$'#@ signature=other\n'
run_case "a package signed by another kind of certificate" 1 "the package's chain starts at a Developer ID Installer certificate" no
signed_meta=$'#@ signature=untrusted\n'
run_case "a package whose signature is not trusted" 1 "the package is signed by a developer certificate issued by Apple" no
signed_meta=$'#@ signature=rootless\n'
run_case "a package whose chain stops short of Apple's root" 1 "the package's chain reaches Apple's root" no
signed_meta=$'#@ stapled=0\n'
run_case "an unstapled package" 1 "stapler validate rejects the package" no
signed_meta=$'#@ assess=rejected\n'
run_case "an assessment spctl rejects" 1 "spctl does not accept the package" no
signed_meta=$'#@ assess=developer-id\n'
run_case "an assessment that is not the notarized source" 1 "spctl names the notary service as the source" no
signed_meta=$'#@ assess=silent\n'
run_case "an assessment that does not say accepted" 1 "spctl accepts the package" no
bin_meta=$'#@ cdhash=\n'
log_text="{\"jobId\":\"${submission}\",\"ticketContents\":[{\"cdhash\":\"${cdhash}\"}]}"
run_case "a binary with no CDHash" 1 "codesign printed no CDHash for the signed binary" no
log_text="{\"jobId\":\"${submission}\",\"ticketContents\":[{\"cdhash\":\"ffffffffffffffffffffffffffffffffffffffff\"}]}"
run_case "a notary log that lacks the binary's CDHash" 1 "the notary log does not list the binary's CDHash" no
log_text="{\"jobId\":\"${submission}\",\"ticketContents\":[{\"cdhash\":\"$(tr 'a-f' 'A-F' <<<"$cdhash")\"}]}"
run_case "a notary log that spells the CDHash in capitals" 0 "the notary log lists the binary's CDHash" yes
log_text="{\"jobId\":\"$(tr 'a-f' 'A-F' <<<"$submission")\",\"ticketContents\":[{\"cdhash\":\"${cdhash}\"}]}"
run_case "a notary log that spells the job ID in capitals" 0 "the notary log is of the submission sign-macos made" yes
log_text="{\"jobId\":\"${other_submission}\",\"ticketContents\":[{\"cdhash\":\"${cdhash}\"}]}"
run_case "a notary log of another submission" 1 "the notary log is of the submission sign-macos made: got '${other_submission}', expected '${submission}'" no
log_text="{\"ticketContents\":[{\"cdhash\":\"${cdhash}\"}]}"
run_case "a notary log with no job ID" 1 "the notary log has no jobId" no
submission_argument="$other_submission"
run_case "a submission ID other than the notary log's" 1 "the notary log is of the submission sign-macos made: got '${submission}', expected '${other_submission}'" no
submission_argument=""
run_case "no submission ID (sign-macos output none)" 1 "'' is not a notary submission ID" no
submission_argument="not-an-id"
run_case "a submission ID that is not a submission ID" 1 "'not-an-id' is not a notary submission ID" no

sums_change=wrong-pkg
run_case "an unsigned package that SHA256SUMS does not list with its hash" 1 "the unsigned ${macos_pkg} is the file SHA256SUMS lists" no
sums_change=wrong-binary
run_case "an unsigned binary that SHA256SUMS does not list with its hash" 1 "the unsigned ${macos} is the file SHA256SUMS lists" no
sums_change=no-pkg
run_case "a SHA256SUMS that does not list the unsigned package" 1 "SHA256SUMS does not list ${macos_pkg} exactly once" no
sums_change=twice
run_case "a SHA256SUMS that lists the unsigned package twice" 1 "SHA256SUMS does not list ${macos_pkg} exactly once" no
sums_change=none
run_case "a missing SHA256SUMS" 1 "unsigned/SHA256SUMS is not a file" no

signed_meta=$'#@ installer_fails=1\n'
run_case "an installer that fails" 1 "installer rejects the signed package" yes
password_asked=1
run_case "a sudo that asks for a password" 1 "sudo asks for a password" no
receipt_before=1
run_case "a receipt that exists before the install" 1 "a receipt for app.axross.marshal exists before the install" no
signed_meta=$'#@ installs=unsigned\n'
run_case "an installed file codesign rejects" 1 "codesign --verify --strict rejects the installed file" yes
signed_meta=$'#@ installs=other\n'
run_case "an installed file that is not the signed binary" 1 "the installed file is the signed bare binary" yes
fake_version="9.9.9"
run_case "an installed binary that prints another version" 1 "the installed binary prints its version" yes

# an existing install is never removed.
existing_directory="${work}/case-existing"
mkdir -p "${existing_directory}/unsigned" "${existing_directory}/signed" "${existing_directory}/notary" "${existing_directory}/install"
make_binary "${existing_directory}/unsigned/${macos}" ""
make_package "${existing_directory}/unsigned/${macos_pkg}" "${existing_directory}/unsigned/${macos}" ""
make_binary "${existing_directory}/signed/${macos}" ""
make_package "${existing_directory}/signed/${macos_pkg}" "${existing_directory}/signed/${macos}" ""
printf '{"jobId":"%s","cdhash":"%s"}\n' "$submission" "$cdhash" >"${existing_directory}/notary/log.json"
write_sums "${existing_directory}/unsigned" ""
printf 'a manual install\n' >"${existing_directory}/install/marshal"
: >"${existing_directory}/record"
status=0
output="$(cd "$existing_directory" && RECORD="${existing_directory}/record" RECEIPT="${existing_directory}/receipt" \
  MARSHAL_INSTALL_PATH="${existing_directory}/install/marshal" SYSTEM_BIN="${work}/bin" SYSTEM_SBIN="${work}/sbin" \
  bash "$script" "$version" unsigned signed notary/log.json "$team" "$submission" 2>&1)" || status=$?
if [[ $status -ne 1 || "$output" != *"exists before the install"* || "$(cat "${existing_directory}/install/marshal")" != "a manual install" ]] ||
  grep -q '^installer ' "${existing_directory}/record"; then
  echo "FAIL an existing install: exit ${status}: ${output}" >&2
  failures=$((failures + 1))
else
  echo "ok   an existing install is refused and left alone"
fi

status=0
output="$(bash "$script" "$version" 2>&1)" || status=$?
if [[ $status -ne 2 || "$output" != *"Usage:"* ]]; then
  echo "FAIL too few arguments: exit ${status}: ${output}" >&2
  failures=$((failures + 1))
else
  echo "ok   too few arguments"
fi

if [[ $failures -ne 0 ]]; then
  echo "${failures} signed package case(s) failed." >&2
  exit 1
fi
echo "All signed package refusal cases passed."
