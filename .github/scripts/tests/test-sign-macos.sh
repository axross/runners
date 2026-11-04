#!/usr/bin/env bash
# runs the `run:` scripts of release.yaml's sign-macos job in order, against
# recording stand-ins of the macOS signing tools, because nothing before merge can
# run the job with real credentials. the scripts are extracted from the workflow,
# not copied, and each runs with only the environment its own step declares, with
# the secrets resolved to fake values, and with a PATH that holds no signing tool,
# so a call that skips the SYSTEM_BIN and SYSTEM_SBIN prefixes fails. the stand-ins
# record every call, and check what only the tools can see: that the keychain
# password and every line of the .p8 were masked before they were used, and that
# the .p12 and the key file have mode 0600.
#
# covered, each as a case below:
#   - the refusals before any `security` call, and after, among them a certificate
#     that is another team's, expired, a second of its name, or not the identity's;
#   - the flags of codesign, pkgbuild, notarytool, stapler and pkgutil;
#   - the deletion of the keychain and the files after a pass, after a failure
#     injected at each stand-in call, and after a failure at the end of each step;
#   - that no secret value and no line of the .p8 appears in the job's output;
#   - that the notary log is where the upload step looks whenever a submission
#     ID exists.
# takes an optional workflow file, for the mutation checks; the default is
# release.yaml.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository="$(cd "${here}/../../.." && pwd)"
workflow="${1:-${repository}/.github/workflows/release.yaml}"
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT

command -v jq >/dev/null || {
  echo "error: jq is required; mise provides it" >&2
  exit 1
}

version=1.2.3
team=ABCDE12345
identifier=app.axross.marshal
application_identity=A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1
installer_name="Developer ID Installer: Test Maintainer (${team})"
installer_hash=B1B1B1B1B1B1B1B1B1B1B1B1B1B1B1B1B1B1B1B1
keychain_password=c0ffeec0ffeec0ffeec0ffeec0ffeec0ffeec0ffeec0ffee
submission=8c4f2a1e-0b3d-4a6e-9f7c-1d2e3f4a5b6c
windows="marshal-${version}-windows-x64.exe"
windows_msi="marshal-${version}-windows-x64.msi"
macos="marshal-${version}-macos-arm64"
macos_pkg="${macos}.pkg"

node "${here}/extract-run-scripts.mjs" "$workflow" sign-macos "${work}/scripts"
steps="${work}/scripts/steps.json"
# the run steps, read once: marker, script, condition and the env pairs of each.
step_markers=()
step_scripts=()
step_conditions=()
step_envs=()
while IFS=$'\t' read -r marker script condition pairs; do
  step_markers+=("$marker")
  step_scripts+=("$script")
  step_conditions+=("$condition")
  step_envs+=("$pairs")
done < <(jq -r '.[] | select(.kind == "run") | [.marker, .script, (.if // "-"), (.env | to_entries | map("\(.key)=\(.value)") | join("|"))] | @tsv' "$steps")

# the fake secrets. none may appear in the job's output.
p8_one="P8LINEONE-0f3a9c51d2"
p8_two="P8LINETWO-77be41aa90"
secret_application="$(printf 'application p12 bytes' | base64 -w0)"
secret_installer="$(printf 'installer p12 bytes' | base64 -w0)"
password="p12-password-4f81c2"
key_id="KEYID-9d3b71e0"
issuer="ISSUER-5e7a2c44-b1d0"

# a PATH with the base utilities and nothing else.
mkdir -p "${work}/base"
for tool in bash env awk sed tr cat rm mkdir cp mv chmod mktemp find sort grep wc head tail cut date base64 stat dirname basename tee sleep sha256sum; do
  ln -s "$(command -v "$tool")" "${work}/base/${tool}"
done

# one stand-in answers to every tool's name. what it reports comes from the
# `scenario` file of the case.
cat >"${work}/standin" <<'STANDIN'
#!/usr/bin/env bash
set -euo pipefail
tool="$(basename "$0")"

# scn <key> <default>: the last `key=value` line of the scenario.
scn() {
  local key value="" found
  while IFS='=' read -r key found; do
    if [[ "$key" == "$1" ]]; then value="$found"; fi
  done <"$SCENARIO"
  if [[ -z "$value" ]]; then echo "$2"; else echo "$value"; fi
}

count=0
if [[ -e "${RECORD}.count" ]]; then read -r count <"${RECORD}.count"; fi
count=$((count + 1))
echo "$count" >"${RECORD}.count"
echo "${count} ${STANDIN_STEP} ${tool} $*" >>"$RECORD"
if [[ "$(scn fail_at 0)" == "$count" ]]; then
  echo "stand-in: injected failure of call ${count}" >&2
  exit 1
fi

masked() {
  grep -qxF "::add-mask::$1" "$OUT" || {
    echo "UNMASKED $1" >>"${RECORD}.problems"
    echo "stand-in: a value was used before it was masked" >&2
    exit 97
  }
}
mode_is_600() {
  [[ "$(stat -c %a "$1")" == 600 ]] || {
    echo "MODE $1 $(stat -c %a "$1")" >>"${RECORD}.problems"
    echo "stand-in: $1 does not have mode 0600" >&2
    exit 96
  }
}

# identity_hash <letter> <number>: a 40-digit hex hash for the identity.
identity_hash() {
  local hash=""
  while [[ ${#hash} -lt 40 ]]; do hash="${hash}${1}${2}"; done
  echo "${hash:0:40}"
}

case "$tool" in
  shasum)
    shift 2
    sha256sum "$@"
    ;;
  security)
    command="$1"
    shift
    case "$command" in
      create-keychain | unlock-keychain)
        masked "$2"
        [[ "$command" == unlock-keychain ]] || : >"$3"
        ;;
      set-key-partition-list)
        masked "$4"
        ;;
      import)
        [[ -e "$1" ]] || exit 95
        [[ -e "$3" ]] || exit 94
        case "$1" in
          *.p12)
            mode_is_600 "$1"
            [[ "$(cat "$1")" == "$(scn p12_content_$(basename "$1" .p12) 'unset')" ]] || exit 93
            [[ "$7" == "$(scn p12_password 'unset')" ]] || exit 92
            ;;
        esac
        ;;
      find-identity)
        # a listing; with `valid`, an identity marked invalid is left out.
        listing() {
          local n=0 i invalid
          invalid="$(scn invalid none)"
          for ((i = 1; i <= $(scn application_count 1); i++)); do
            n=$((n + 1))
            if [[ "$1" == valid && "$invalid" == application ]]; then continue; fi
            printf '  %d) %s "Developer ID Application: Test Maintainer (%s)"\n' "$n" "$(identity_hash A "$i")" "$(scn application_team ABCDE12345)"
          done
          for ((i = 1; i <= $(scn installer_count 1); i++)); do
            n=$((n + 1))
            if [[ "$1" == valid && "$invalid" == installer ]]; then continue; fi
            printf '  %d) %s "Developer ID Installer: Test Maintainer (%s)"\n' "$n" "$(identity_hash B "$i")" "$(scn installer_team ABCDE12345)"
          done
          printf '  %d) %s "Apple Development: someone (ZZZZZZZZZZ)"\n' "$((n + 1))" "$(identity_hash C 1)"
          echo "     $((n + 1)) identities found"
        }
        if [[ "$1" == -v ]]; then listing valid; else listing all; fi
        ;;
      find-certificate)
        # find-certificate -a -Z -c <name> -p <keychain>: the SHA-1 hash and the
        # PEM of each certificate whose name holds <name>. the scenario's
        # `<kind>_certificates` is `one` (the identity's own certificate, the
        # default), `other` (a certificate of another hash) or `two` (the identity's
        # and a second one).
        [[ "$1 $2 $3 $5" == "-a -Z -c -p" ]] || exit 89
        kind=Application
        letter=A
        if [[ "$4" == *Installer* ]]; then kind=Installer letter=B; fi
        certificate() {
          printf 'SHA-1 hash: %s\n-----BEGIN CERTIFICATE-----\nFAKE %s\n-----END CERTIFICATE-----\n' "$1" "$kind"
        }
        case "$(scn "$(echo "$kind" | tr 'A-Z' 'a-z')_certificates" one)" in
          one) certificate "$(identity_hash "$letter" 1)" ;;
          other) certificate "$(identity_hash D 1)" ;;
          two)
            certificate "$(identity_hash "$letter" 1)"
            certificate "$(identity_hash D 1)"
            ;;
        esac
        ;;
      delete-keychain)
        if [[ "$(scn delete_fails 0)" == 1 ]]; then exit 1; fi
        rm -f "$1"
        ;;
      *) ;;
    esac
    ;;
  openssl)
    case "$1" in
      rand) echo "$KEYCHAIN_PASSWORD" ;;
      x509)
        kind=Application
        grep -q 'FAKE Installer' "$3" && kind=Installer
        kind_lower="$(echo "$kind" | tr 'A-Z' 'a-z')"
        team_of="$(scn "${kind_lower}_team" ABCDE12345)"
        days="$(scn "${kind_lower}_days" 400)"
        if [[ "$*" == *-checkend* ]]; then
          seconds="${*: -1}"
          if ((seconds >= days * 86400)); then
            echo "Certificate will expire"
            exit 1
          fi
        else
          echo "subject=UID=${team_of},CN=Developer ID ${kind}: Test Maintainer (${team_of}),OU=${team_of},O=Test Maintainer,C=US"
          echo "notAfter=Nov  3 12:00:00 2027 GMT"
        fi
        ;;
    esac
    ;;
  plutil)
    # plutil -extract <key> raw -o - <file>
    key="$2"
    file="${@: -1}"
    value="$(sed -n "s/.*\"${key}\" *: *\"\([^\"]*\)\".*/\1/p" "$file" 2>/dev/null | head -n 1)"
    [[ -n "$value" ]] || exit 1
    echo "$value"
    ;;
  codesign)
    file="${@: -1}"
    if [[ "$1" == --force ]]; then
      [[ -f "$file" ]] || exit 91
      echo "# signed" >>"$file"
    fi
    if [[ "$1" == --display ]]; then
      echo "designated => identifier \"app.axross.marshal\" and anchor apple generic"
    fi
    ;;
  pkgbuild)
    root=""
    previous=""
    for argument in "$@"; do
      [[ "$previous" == --root ]] && root="$argument"
      previous="$argument"
    done
    for path in "$root" "$root/usr" "$root/usr/local" "$root/usr/local/bin" "$root/usr/local/bin/marshal"; do
      [[ "$(stat -c %a "$path")" == 755 ]] || {
        echo "MODE $path $(stat -c %a "$path")" >>"${RECORD}.problems"
        exit 96
      }
    done
    grep -q '^# signed$' "$root/usr/local/bin/marshal" || exit 90
    echo "a package" >"${@: -1}"
    ;;
  pkgutil)
    echo 'Package "x.pkg":'
    if [[ "$(scn package_signature good)" == good ]]; then
      echo "   Status: signed by a developer certificate issued by Apple for distribution"
      echo "   Certificate Chain:"
      echo "    1. Developer ID Installer: Test Maintainer ($(scn installer_team ABCDE12345))"
    else
      echo "   Status: no signature"
    fi
    ;;
  xcrun)
    case "$1" in
      --find)
        [[ "$(scn xcrun_missing none)" != "$2" ]] || exit 1
        echo "/stand-in/xcrun/$2"
        ;;
      notarytool)
        key=""
        previous=""
        for argument in "$@"; do
          [[ "$previous" == --key ]] && key="$argument"
          previous="$argument"
        done
        mode_is_600 "$key"
        while IFS= read -r line; do
          [[ -z "$line" ]] || masked "$line"
        done <"$key"
        if [[ "$2" == submit ]]; then
          status="$(scn notary_status Accepted)"
          case "$status" in
            none) echo "Error: could not reach the notary service" >&2 ;;
            garbage) echo "not json at all" ;;
            noid) echo '{"message": "no id here", "status": "Accepted"}' ;;
            nostatus) echo "{\"id\": \"${SUBMISSION}\", \"message\": \"processing\"}" ;;
            *) echo "{\"id\": \"${SUBMISSION}\", \"message\": \"Processing complete\", \"name\": \"x.pkg\", \"path\": \"/x.pkg\", \"status\": \"${status}\"}" ;;
          esac
          exit "$(scn notary_exit 0)"
        fi
        if [[ "$2" == log ]]; then
          if [[ "$(scn log_fails 0)" == 1 ]]; then
            echo "Error: could not fetch the log" >&2
            exit 1
          fi
          echo "{\"jobId\": \"$3\", \"status\": \"$(scn notary_status Accepted)\"}" >"${@: -1}"
        fi
        ;;
      stapler)
        [[ "$2" == validate ]] || echo "# stapled" >>"$3"
        ;;
    esac
    ;;
esac
STANDIN
chmod +x "${work}/standin"

failures=0

# --- one run of the job ------------------------------------------------------

# the variables a case sets before run_job.
reset_case() {
  s_application_p12="$secret_application"
  s_installer_p12="$secret_installer"
  s_password="$password"
  s_p8="-----BEGIN PRIVATE KEY-----
${p8_one}
${p8_two}
-----END PRIVATE KEY-----"
  s_key_id="$key_id"
  s_issuer="$issuer"
  s_team="$team"
  job_version="$version"
  scenario_lines=""
  missing_tools=""
  fail_after_step=""
  mutate_script=""
  mutate_expression=""
  fixture_change=""
}

# resolve <expression>: the value of a `${{ ... }}` expression of a step's env.
resolve() {
  local expression="$1" inner
  inner="${expression#\$\{\{ }"
  inner="${inner% \}\}}"
  case "$inner" in
    secrets.MACOS_APPLICATION_P12_BASE64) printf '%s' "$s_application_p12" ;;
    secrets.MACOS_INSTALLER_P12_BASE64) printf '%s' "$s_installer_p12" ;;
    secrets.MACOS_P12_PASSWORD) printf '%s' "$s_password" ;;
    secrets.MACOS_NOTARY_KEY_P8) printf '%s' "$s_p8" ;;
    secrets.MACOS_NOTARY_KEY_ID) printf '%s' "$s_key_id" ;;
    secrets.MACOS_NOTARY_ISSUER_ID) printf '%s' "$s_issuer" ;;
    vars.MACOS_TEAM_ID) printf '%s' "$s_team" ;;
    needs.plan.outputs.version) printf '%s' "$job_version" ;;
    *)
      echo "error: the test does not know the expression ${expression}" >&2
      exit 1
      ;;
  esac
}

# run_job <case name>: lays out the files the download step would, runs every
# run step with the environment its step declares, and leaves the case's
# directory (case_dir) holding the output (out), the record of the stand-in
# calls, and what the job ended with (job_status).
run_job() {
  case_dir="${work}/case-${1//[^a-zA-Z0-9]/-}"
  rm -rf "$case_dir"
  mkdir -p "${case_dir}/ws/unsigned" "${case_dir}/tmp" "${case_dir}/bin" "${case_dir}/sbin"
  out="${case_dir}/out"
  record="${case_dir}/record"
  : >"$out"
  : >"$record"
  # the tools. a case may remove some.
  for tool in security codesign pkgbuild openssl plutil shasum xcrun; do
    ln -s "${work}/standin" "${case_dir}/bin/${tool}"
  done
  ln -s "${work}/standin" "${case_dir}/sbin/pkgutil"
  for tool in $missing_tools; do rm -f "${case_dir}/bin/${tool}" "${case_dir}/sbin/${tool}"; done
  {
    printf 'application_count=1\ninstaller_count=1\n'
    printf 'p12_content_application=application p12 bytes\np12_content_installer=installer p12 bytes\n'
    printf 'p12_password=%s\n' "$password"
    printf '%s\n' "$scenario_lines"
  } >"${case_dir}/scenario"

  # the files of marshal-release.
  local unsigned="${case_dir}/ws/unsigned"
  printf 'windows binary\n' >"${unsigned}/${windows}"
  printf 'windows installer\n' >"${unsigned}/${windows_msi}"
  printf 'macos binary\n' >"${unsigned}/${macos}"
  printf 'macos installer\n' >"${unsigned}/${macos_pkg}"
  (cd "$unsigned" && sha256sum "$macos" "$macos_pkg" "$windows" "$windows_msi" >SHA256SUMS)
  case "$fixture_change" in
    "") ;;
    extra-file) printf 'helper\n' >"${unsigned}/fake-docker" ;;
    missing-msi) rm "${unsigned}/${windows_msi}" ;;
    other-version) mv "${unsigned}/${windows}" "${unsigned}/marshal-9.9.9-windows-x64.exe" ;;
    changed-binary) printf 'changed\n' >>"${unsigned}/${macos}" ;;
    changed-pkg) printf 'changed\n' >>"${unsigned}/${macos_pkg}" ;;
    no-binary-line) grep -v " ${macos}\$" "${unsigned}/SHA256SUMS" >"${case_dir}/sums" && cp "${case_dir}/sums" "${unsigned}/SHA256SUMS" ;;
    twice-listed) grep " ${macos_pkg}\$" "${unsigned}/SHA256SUMS" >"${case_dir}/line" && cat "${case_dir}/line" >>"${unsigned}/SHA256SUMS" ;;
    symlink) rm "${unsigned}/${macos}" && ln -s "${unsigned}/${windows}" "${unsigned}/${macos}" ;;
    *) echo "error: unknown fixture change ${fixture_change}" >&2; exit 1 ;;
  esac

  # the scripts, with a mutation if the case asks for one.
  cp -r "${work}/scripts" "${case_dir}/scripts"
  if [[ -n "$mutate_script" ]]; then
    sed -i "$mutate_expression" "${case_dir}/scripts/${mutate_script}"
  fi

  : >"${case_dir}/github_env"
  : >"${case_dir}/github_output"
  job_status=pass
  local index condition marker script step_failed env_args line pair key value resolved
  for index in "${!step_markers[@]}"; do
    marker="${step_markers[$index]}"
    script="${case_dir}/scripts/${step_scripts[$index]}"
    condition="${step_conditions[$index]}"
    if [[ "$job_status" == fail && "$condition" != "always()" ]]; then
      continue
    fi
    env_args=(
      "PATH=${work}/base" "HOME=${case_dir}/home" "RUNNER_TEMP=${case_dir}/tmp"
      "GITHUB_ENV=${case_dir}/github_env" "GITHUB_OUTPUT=${case_dir}/github_output"
      "SYSTEM_BIN=${case_dir}/bin" "SYSTEM_SBIN=${case_dir}/sbin"
      "SCENARIO=${case_dir}/scenario" "RECORD=${record}" "OUT=${out}"
      "STANDIN_STEP=${marker}" "KEYCHAIN_PASSWORD=${keychain_password}" "SUBMISSION=${submission}"
    )
    while IFS= read -r line; do
      [[ -n "$line" ]] && env_args+=("$line")
    done <"${case_dir}/github_env"
    # the pairs are KEY=${{ expression }}, separated by |.
    while IFS= read -r pair; do
      [[ -n "$pair" ]] || continue
      key="${pair%%=*}"
      value="${pair#*=}"
      # the trailing x keeps a value that ends in a newline whole.
      resolved="$(
        resolve "$value"
        printf x
      )"
      resolved="${resolved%x}"
      env_args+=("${key}=${resolved}")
    done < <(tr '|' '\n' <<<"${step_envs[$index]}")
    echo "== ${marker} ==" >>"$out"
    step_failed=0
    (cd "${case_dir}/ws" && env -i "${env_args[@]}" "${work}/base/bash" --noprofile --norc -eo pipefail "$script") >>"$out" 2>&1 || step_failed=1
    if [[ "$fail_after_step" == "$marker" ]]; then
      echo "stand-in: the step ${marker} is made to fail" >>"$out"
      step_failed=1
    fi
    [[ $step_failed -eq 0 ]] || job_status=fail
  done
}

# job_output: the job's output without the ::add-mask:: commands, which the runner
# consumes and does not print.
job_output() {
  grep -v '^::add-mask::' "$out" >"${case_dir}/visible" || true
}

# --- the checks every run gets -----------------------------------------------

check_clean_and_quiet() {
  local problem="" leftovers value
  job_output
  leftovers="$(find "${case_dir}/tmp" -mindepth 1 \( -name '*.keychain*' -o -name '*.p12' -o -name '*.p8' -o -name 'marshal-signing*' \) 2>/dev/null | tr '\n' ' ')"
  if [[ -n "$leftovers" ]]; then
    problem="the clean-up left ${leftovers}"
  fi
  for value in "$secret_application" "$secret_installer" "$password" "$p8_one" "$p8_two" "$key_id" "$issuer" "$keychain_password" "BEGIN PRIVATE KEY"; do
    if grep -q -F -e "$value" "${case_dir}/visible"; then
      problem="${problem:+${problem}; }the output holds a secret: ${value:0:12}..."
    fi
  done
  if [[ -s "${record}.problems" ]]; then
    problem="${problem:+${problem}; }the stand-ins saw: $(tr '\n' ' ' <"${record}.problems")"
  fi
  echo "$problem"
}

failed_case() {
  echo "FAIL $1: $2" >&2
  echo "---- output" >&2
  tail -n 25 "$out" >&2
  failures=$((failures + 1))
}

# run_case <name> <pass|fail> <text the output must hold> <security: none|some> [<extra check function>]
# sets up the case with the variables of reset_case, which the caller changed first.
run_case() {
  local name="$1" want="$2" text="$3" security="$4" extra="${5:-}" problem
  run_job "$name"
  problem="$(check_clean_and_quiet)"
  if [[ "$job_status" != "$want" ]]; then
    problem="the job ended with ${job_status}, expected ${want}"
  elif [[ -n "$text" ]] && ! grep -q -F -e "$text" "${case_dir}/visible"; then
    problem="the output does not hold '${text}'"
  elif [[ "$security" == none ]] && grep -q ' security ' "$record"; then
    problem="security was called before the refusal: $(grep ' security ' "$record" | head -n 1)"
  elif [[ -n "$extra" && -z "$problem" ]]; then
    problem="$("$extra")"
  fi
  if [[ -n "$problem" ]]; then
    failed_case "$name" "$problem"
  else
    echo "ok   ${name}"
  fi
}

# --- extra checks ------------------------------------------------------------

# line_of <pattern>: the number of the first record line that holds the fixed text.
line_of() {
  grep -n -F -e "$1" "$record" | head -n 1 | cut -d: -f1
}

check_flags() {
  local keychain="${case_dir}/tmp/marshal-signing.keychain-db" dir="${case_dir}/tmp/marshal-signing" expected sign package submit staple validate
  for expected in \
    "security create-keychain -p ${keychain_password} ${keychain}" \
    "security unlock-keychain -p ${keychain_password} ${keychain}" \
    "security import ${dir}/DeveloperIDG2CA.cer -k ${keychain} -t cert" \
    "security import ${dir}/application.p12 -k ${keychain} -f pkcs12 -P ${password} -T ${case_dir}/bin/codesign -T ${case_dir}/bin/pkgbuild" \
    "security import ${dir}/installer.p12 -k ${keychain} -f pkcs12 -P ${password} -T ${case_dir}/bin/codesign -T ${case_dir}/bin/pkgbuild" \
    "security set-key-partition-list -S apple-tool:,apple: -k ${keychain_password} ${keychain}" \
    "security find-identity ${keychain}" \
    "security find-identity -v ${keychain}" \
    "security find-certificate -a -Z -c Developer ID Application: Test Maintainer (${team}) -p ${keychain}" \
    "security find-certificate -a -Z -c ${installer_name} -p ${keychain}" \
    "codesign --force --sign ${application_identity} --keychain ${keychain} --options runtime --timestamp --identifier ${identifier} signed/${macos}" \
    "codesign --verify --strict --verbose=2 signed/${macos}" \
    " --install-location / --ownership recommended --sign ${installer_name} --keychain ${keychain} signed/${macos_pkg}" \
    "xcrun notarytool submit signed/${macos_pkg} --key ${dir}/AuthKey.p8 --key-id ${key_id} --issuer ${issuer} --wait --timeout 30m --output-format json" \
    "xcrun notarytool log ${submission} --key ${dir}/AuthKey.p8 --key-id ${key_id} --issuer ${issuer} ${case_dir}/tmp/marshal-notary/notary-log.json" \
    "xcrun stapler staple signed/${macos_pkg}" \
    "xcrun stapler validate signed/${macos_pkg}" \
    "pkgutil --check-signature signed/${macos_pkg}" \
    "xcrun --find notarytool" \
    "xcrun --find stapler"; do
    if ! grep -q -F -e "$expected" "$record"; then
      echo "the record has no call '${expected}'"
      return
    fi
  done
  if ! grep -q -E " pkgbuild --root ${dir}/stage\.[A-Za-z0-9]+ --identifier ${identifier} --version ${version} --install-location / --ownership recommended --sign " "$record"; then
    echo "pkgbuild did not get build-pkg.sh's arguments"
    return
  fi
  sign="$(line_of "codesign --force")"
  package="$(line_of " pkgbuild ")"
  submit="$(line_of "notarytool submit")"
  staple="$(line_of "stapler staple")"
  validate="$(line_of "stapler validate")"
  if ! ((sign < package && package < submit && submit < staple && staple < validate)); then
    echo "the tools ran in the wrong order: sign ${sign}, package ${package}, submit ${submit}, staple ${staple}, validate ${validate}"
  fi
}

check_outputs() {
  local binary_hash package_hash
  binary_hash="$(sha256sum "${case_dir}/ws/signed/${macos}" | cut -d' ' -f1)"
  package_hash="$(sha256sum "${case_dir}/ws/signed/${macos_pkg}" | cut -d' ' -f1)"
  for expected in "binary_sha256=${binary_hash}" "package_sha256=${package_hash}" "team_id=${team}" "submission_id=${submission}"; do
    grep -qxF "$expected" "${case_dir}/github_output" || {
      echo "the job outputs lack ${expected}"
      return
    }
  done
  grep -q '^# stapled$' "${case_dir}/ws/signed/${macos_pkg}" || echo "the package hash was taken before stapling"
  [[ "$(find "${case_dir}/ws/signed" -mindepth 1 | wc -l)" -eq 2 ]] || echo "the signed directory holds other files than the binary and the package"
  grep -q '^# signed$' "${case_dir}/ws/signed/${macos}" || echo "the binary was not signed"
  [[ -s "${case_dir}/tmp/marshal-notary/notary-log.json" ]] || echo "the notary log is not where the upload step reads it"
}

# the upload step reads the directory the script writes the log to.
check_upload_path() {
  local upload path
  upload="$(jq -c '.[] | select(.kind == "uses" and .with.name == "notary-log-macos")' "$steps")"
  path="$(jq -r '.with.path' <<<"$upload")"
  # shellcheck disable=SC2016 # the expression is compared as text.
  if [[ "$path" != '${{ runner.temp }}/marshal-notary' ]]; then
    echo "the notary log upload reads ${path}, not the directory the script writes to"
  fi
  if [[ "$(jq -r '.if' <<<"$upload")" != "always()" || "$(jq -r '.with["if-no-files-found"]' <<<"$upload")" != ignore ]]; then
    echo "the notary log upload does not run after a failure, or fails when there is no log"
  fi
}

check_log_kept() {
  [[ -s "${case_dir}/tmp/marshal-notary/notary-log.json" ]] || echo "no notary log is kept although a submission ID exists"
}

check_no_log() {
  [[ ! -e "${case_dir}/tmp/marshal-notary/notary-log.json" ]] || echo "a notary log exists although there is no submission ID"
}

check_warning() {
  grep -q '^::warning::' "$out" || echo "no warning about an expiry within 90 days"
}

check_no_warning() {
  ! grep -q '^::warning::the Developer ID' "$out" || echo "a warning for a certificate that expires in more than 90 days"
}

# --- the cases ---------------------------------------------------------------

reset_case
run_case "a passing run" pass "Deleted the keychain, the certificate files and the key file." some check_flags
reset_case
run_case "a passing run's outputs, the signed files and the notary log" pass "Submission ${submission} was accepted." some check_outputs
reset_case
run_case "the notary log upload step reads the log's directory" pass "" some check_upload_path

echo "-- refusals before any security call --"
for name in MACOS_APPLICATION_P12_BASE64 MACOS_INSTALLER_P12_BASE64 MACOS_P12_PASSWORD MACOS_NOTARY_KEY_P8 MACOS_NOTARY_KEY_ID MACOS_NOTARY_ISSUER_ID MACOS_TEAM_ID; do
  reset_case
  case "$name" in
    MACOS_APPLICATION_P12_BASE64) s_application_p12="" ;;
    MACOS_INSTALLER_P12_BASE64) s_installer_p12="" ;;
    MACOS_P12_PASSWORD) s_password="" ;;
    MACOS_NOTARY_KEY_P8) s_p8="" ;;
    MACOS_NOTARY_KEY_ID) s_key_id="" ;;
    MACOS_NOTARY_ISSUER_ID) s_issuer="" ;;
    MACOS_TEAM_ID) s_team="" ;;
  esac
  run_case "an empty ${name}" fail "these secrets or variables are empty: ${name}" none
done
reset_case
s_team="abc"
run_case "a team ID that is not a team ID" fail "MACOS_TEAM_ID is not a 10-character team ID" none
for bad in "1.2" "1.2.3-rc.1" "01.2.3" "v1.2.3" "" "1.2.3 "; do
  reset_case
  job_version="$bad"
  run_case "the version '${bad}'" fail "is not X.Y.Z" none
done
for change in extra-file missing-msi other-version; do
  reset_case
  fixture_change="$change"
  run_case "an artifact with ${change}" fail "the downloaded files are not exactly" none
done
reset_case
fixture_change=symlink
run_case "a macOS binary that is a symlink" fail "is not a regular file" none
reset_case
fixture_change=changed-binary
run_case "a macOS binary that differs from its SHA256SUMS line" fail "${macos} has SHA-256" none
reset_case
fixture_change=changed-pkg
run_case "a macOS package that differs from its SHA256SUMS line" fail "${macos_pkg} has SHA-256" none
reset_case
fixture_change=no-binary-line
run_case "a SHA256SUMS without the binary's line" fail "does not list ${macos} exactly once" none
reset_case
fixture_change=twice-listed
run_case "a SHA256SUMS that lists the package twice" fail "does not list ${macos_pkg} exactly once" none
for tool in security codesign pkgbuild openssl plutil shasum xcrun pkgutil; do
  reset_case
  missing_tools="$tool"
  run_case "a missing ${tool}" fail "${tool} is missing" none
done
for tool in notarytool stapler; do
  reset_case
  scenario_lines="xcrun_missing=${tool}"
  run_case "xcrun cannot find ${tool}" fail "xcrun cannot find ${tool}" none
done

echo "-- refusals after the keychain exists --"
reset_case
mutate_script="02-keychain.sh"
mutate_expression='s/^g2_sha256="f/g2_sha256="0/'
run_case "an embedded intermediate whose SHA-256 differs from the pin (the pin changed)" fail "not the pinned" some
reset_case
mutate_script="02-keychain.sh"
mutate_expression='s/^g2_base64="MIIEPj/g2_base64="MIIEPk/'
run_case "an embedded intermediate whose SHA-256 differs from the pin (the certificate changed)" fail "not the pinned" some
reset_case
s_application_p12=$'\n'
run_case "an Application secret that decodes to nothing" fail "application.p12 is empty after decoding its secret" some
reset_case
s_installer_p12=$'\n'
run_case "an Installer secret that decodes to nothing" fail "installer.p12 is empty after decoding its secret" some
for scenario in "application_count=0:holds 0 Developer ID Application identities" "application_count=2:holds 2 Developer ID Application identities" "installer_count=0:holds 0 Developer ID Installer identities" "installer_count=2:holds 2 Developer ID Installer identities"; do
  reset_case
  scenario_lines="${scenario%%:*}"
  run_case "${scenario%%:*}" fail "${scenario#*:}" some
done
reset_case
scenario_lines="invalid=application"
run_case "an Application identity that is not valid in the keychain" fail "is not valid in this keychain" some
reset_case
scenario_lines="invalid=installer"
run_case "an Installer identity that is not valid in the keychain" fail "is not valid in this keychain" some
reset_case
scenario_lines="application_team=ZZZZZ99999"
run_case "an Application certificate of another team" fail "Developer ID Application certificate names team 'ZZZZZ99999'" some
reset_case
scenario_lines="installer_team=ZZZZZ99999"
run_case "an Installer certificate of another team" fail "Developer ID Installer certificate names team 'ZZZZZ99999'" some
reset_case
scenario_lines="installer_certificates=two"
run_case "a second Installer certificate of the same name" fail "holds 2 certificates named 'Developer ID Installer: Test Maintainer (${team})', not exactly one" some
reset_case
scenario_lines="application_certificates=two"
run_case "a second Application certificate of the same name" fail "holds 2 certificates named 'Developer ID Application: Test Maintainer (${team})', not exactly one" some
reset_case
scenario_lines="installer_certificates=other"
run_case "an Installer certificate that does not have the identity's hash" fail "does not have the SHA-1 hash ${installer_hash} of the Developer ID Installer identity" some
reset_case
scenario_lines="application_certificates=other"
run_case "an Application certificate that does not have the identity's hash" fail "does not have the SHA-1 hash ${application_identity} of the Developer ID Application identity" some
reset_case
scenario_lines="application_days=-5"
run_case "an expired Application certificate" fail "Application certificate expired on" some
reset_case
scenario_lines="installer_days=-5"
run_case "an expired Installer certificate" fail "Installer certificate expired on" some
reset_case
scenario_lines="application_days=30"
run_case "an Application certificate that expires within 90 days" pass "expires on Nov  3 12:00:00 2027 GMT, within 90 days" some check_warning
reset_case
scenario_lines="installer_days=89"
run_case "an Installer certificate that expires in 89 days" pass "within 90 days" some check_warning
reset_case
scenario_lines="installer_days=91"
run_case "an Installer certificate that expires in 91 days" pass "" some check_no_warning

echo "-- notarization --"
reset_case
scenario_lines=$'notary_status=Invalid\nnotary_exit=1'
run_case "an Invalid result" fail "status 'Invalid', not Accepted" some check_log_kept
reset_case
scenario_lines=$'notary_status=Invalid\nnotary_exit=0'
run_case "an Invalid result that exits 0" fail "status 'Invalid', not Accepted" some check_log_kept
reset_case
scenario_lines=$'notary_status=nostatus\nnotary_exit=0'
run_case "a result that exits 0 without a status" fail "status '', not Accepted" some check_log_kept
reset_case
scenario_lines=$'notary_status=none\nnotary_exit=1'
run_case "a submission that printed nothing" fail "printed no submission ID" some check_no_log
reset_case
scenario_lines=$'notary_status=garbage\nnotary_exit=0'
run_case "a submission that printed no JSON" fail "printed no submission ID" some check_no_log
reset_case
scenario_lines=$'notary_status=noid\nnotary_exit=0'
run_case "a result with a status and no submission ID" fail "printed no submission ID" some check_no_log
reset_case
scenario_lines=$'notary_status=Accepted\nnotary_exit=1'
run_case "an Accepted result with a non-zero exit" pass "although the status is Accepted" some check_log_kept
reset_case
scenario_lines="log_fails=1"
run_case "a notary log that cannot be fetched" fail "could not be fetched" some
reset_case
scenario_lines="package_signature=bad"
run_case "a package that is not signed by the Installer certificate" fail "is not signed by a Developer ID Installer certificate of team" some

echo "-- clean-up --"
# the number of calls a passing run makes.
reset_case
run_job "counting"
calls="$(cat "${record}.count")"
echo "a passing run makes ${calls} stand-in calls"
for ((call = 1; call <= calls; call++)); do
  reset_case
  scenario_lines="fail_at=${call}"
  run_job "failure at call ${call}"
  problem="$(check_clean_and_quiet)"
  injected="$(sed -n "${call}p" "$record" | cut -d' ' -f2)"
  # a failed 90-day check is a warning, not a refusal.
  if [[ "$injected" != cleanup && "$job_status" != fail && "$(sed -n "${call}p" "$record")" != *"-checkend 7776000" ]]; then
    problem="the job passed although call ${call} (${injected}) failed"
  fi
  if [[ -n "$problem" ]]; then
    failed_case "a failure injected at call ${call} ($(sed -n "${call}p" "$record" | cut -d' ' -f2-3))" "$problem"
  fi
done
echo "ok   a failure injected at each of the ${calls} calls leaves nothing behind"
for step in check keychain identities sign package notarize staple hash; do
  reset_case
  fail_after_step="$step"
  run_case "a failure at the end of the ${step} step" fail "the step ${step} is made to fail" some
done
reset_case
scenario_lines="delete_fails=1"
run_case "a keychain that security cannot delete" pass "security could not delete the keychain" some

if [[ $failures -ne 0 ]]; then
  echo "${failures} sign-macos case(s) failed." >&2
  exit 1
fi
echo "All sign-macos cases passed."
