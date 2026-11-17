#!/usr/bin/env bash
# runs the `run:` scripts of release.yaml's sign-macos job in order, against
# recording stand-ins of the macOS signing tools, because nothing before merge can
# run the job with real credentials. the scripts are extracted from the workflow,
# not copied, and each runs with only the environment its own step declares, with
# the secrets resolved to fake values, and with a PATH that holds no signing tool,
# so a call that skips the SYSTEM_BIN prefix fails. the stand-ins record every
# call, and check what only the tools can see: that the keychain password and
# every line of the .p8 were masked before they were used, and that the .p12 and
# the key file have mode 0600.
#
# covered, each as a case below:
#   - the refusals before any `security` call, and after, among them a certificate
#     that is another team's, expired, a second of its name, or not the identity's,
#     an app archive that is not one Marshal.app with one executable and no nested
#     code, and a designated requirement that is not form A or B;
#   - the flags of ditto, codesign, hdiutil, notarytool and stapler, and their order;
#   - hdiutil create's three attempts;
#   - the deletion of the keychain and the files after a pass, after a failure
#     injected at each stand-in call, and after a failure at the end of each step;
#   - that no secret value and no line of the .p8 appears in the job's output;
#   - that the notary log is where the upload step looks whenever a submission
#     ID exists.
# takes an optional workflow file; the default is release.yaml. runs on Linux and
# on macOS.
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
application_name="Developer ID Application: Test Maintainer (${team})"
keychain_password=6b1f0e9c2d4a8b7e3f5c1a0d9e8b7c6a5f4e3d2c1b0a9f8e
submission=8c4f2a1e-0b3d-4a6e-9f7c-1d2e3f4a5b6c
archive="marshal-${version}-macos-arm64-app.zip"
image="marshal-${version}-macos-arm64.dmg"
archive_content="app archive bytes"
archive_sha256="$(printf '%s\n' "$archive_content" | sha256sum | cut -d' ' -f1)"

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
secret_application="$(printf 'application p12 bytes' | base64 | tr -d '\n')"
password="p12-password-4f81c2"
key_id="KEYID-9d3b71e0"
issuer="ISSUER-5e7a2c44-b1d0"

# a PATH with the base utilities and nothing else. sleep returns at once, so the
# hdiutil retries do not wait.
mkdir -p "${work}/base"
for tool in bash env awk sed tr cat rm mkdir cp mv chmod ln mktemp find sort grep wc head tail cut date base64 stat dirname basename readlink tee sha256sum; do
  ln -s "$(command -v "$tool")" "${work}/base/${tool}"
done
printf '#!/bin/sh\nexit 0\n' >"${work}/base/sleep"
chmod +x "${work}/base/sleep"

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
# file_mode <path>: the permission bits, with GNU or BSD stat.
file_mode() {
  stat -c %a "$1" 2>/dev/null || stat -f %Lp "$1"
}
mode_is_600() {
  [[ "$(file_mode "$1")" == 600 ]] || {
    echo "MODE $1 $(file_mode "$1")" >>"${RECORD}.problems"
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
            [[ "$(cat "$1")" == "$(scn p12_content 'unset')" ]] || exit 93
            [[ "$7" == "$(scn p12_password 'unset')" ]] || exit 92
            ;;
        esac
        ;;
      find-identity)
        # a listing; with `valid`, an identity marked invalid is left out.
        listing() {
          local n=0 i
          for ((i = 1; i <= $(scn application_count 1); i++)); do
            n=$((n + 1))
            if [[ "$1" == valid && "$(scn invalid none)" == application ]]; then continue; fi
            printf '  %d) %s "Developer ID Application: Test Maintainer (%s)"\n' "$n" "$(identity_hash A "$i")" "$(scn application_team ABCDE12345)"
          done
          printf '  %d) %s "Apple Development: someone (ZZZZZZZZZZ)"\n' "$((n + 1))" "$(identity_hash C 1)"
          echo "     $((n + 1)) identities found"
        }
        if [[ "$1" == -v ]]; then listing valid; else listing all; fi
        ;;
      find-certificate)
        # find-certificate -a -Z -c <name> -p <keychain>: the SHA-1 hash and the
        # PEM of each certificate whose name holds <name>. the scenario's
        # `certificates` is `one` (the identity's own certificate, the default),
        # `other` (a certificate of another hash) or `two` (the identity's and a
        # second one).
        [[ "$1 $2 $3 $5" == "-a -Z -c -p" ]] || exit 89
        certificate() {
          printf 'SHA-1 hash: %s\n-----BEGIN CERTIFICATE-----\nFAKE Application\n-----END CERTIFICATE-----\n' "$1"
        }
        case "$(scn certificates one)" in
          one) certificate "$(identity_hash A 1)" ;;
          other) certificate "$(identity_hash D 1)" ;;
          two)
            certificate "$(identity_hash A 1)"
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
        team_of="$(scn application_team ABCDE12345)"
        days="$(scn application_days 400)"
        if [[ "$*" == *-checkend* ]]; then
          seconds="${*: -1}"
          if ((seconds >= days * 86400)); then
            echo "Certificate will expire"
            exit 1
          fi
        else
          echo "subject=UID=${team_of},CN=Developer ID Application: Test Maintainer (${team_of}),OU=${team_of},O=Test Maintainer,C=US"
          echo "notAfter=Nov  3 12:00:00 2027 GMT"
        fi
        ;;
    esac
    ;;
  ditto)
    if [[ "$1" == -x ]]; then
      # ditto -x -k <archive> <directory>: lays out the app the scenario's
      # `layout` names.
      [[ "$2" == -k && -f "$3" ]] || exit 88
      contents="$4/Marshal.app/Contents"
      mkdir -p "${contents}/MacOS"
      printf 'marshal-app binary\n' >"${contents}/MacOS/marshal-app"
      printf '{"CFBundleIdentifier": "%s", "CFBundleShortVersionString": "%s"}\n' \
        "$(scn app_identifier app.axross.marshal)" "$(scn app_version "$VERSION_OF_APP")" >"${contents}/Info.plist"
      case "$(scn layout good)" in
        good) ;;
        extra-entry) printf 'readme\n' >"$4/README" ;;
        extra-executable) printf 'helper\n' >"${contents}/MacOS/fake-docker" ;;
        *) mkdir -p "${contents}/$(scn layout good)" ;;
      esac
    else
      # ditto <source> <destination>: a copy.
      [[ -d "$1" ]] || exit 88
      cp -R "$1" "$2"
    fi
    ;;
  plutil)
    # plutil -extract <key> raw -o - <file>
    key="$2"
    file="${*: -1}"
    value="$(sed -n "s/.*\"${key}\" *: *\"\([^\"]*\)\".*/\1/p" "$file" 2>/dev/null | head -n 1)"
    [[ -n "$value" ]] || exit 1
    echo "$value"
    ;;
  codesign)
    target="${*: -1}"
    if [[ "$1" == --force ]]; then
      if [[ -d "$target" ]]; then
        echo "# signed" >>"${target}/Contents/MacOS/marshal-app"
      else
        [[ -f "$target" ]] || exit 91
        echo "# signed" >>"$target"
      fi
    fi
    if [[ "$1" == --display ]]; then
      echo "Executable=${target}/Contents/MacOS/marshal-app" >&2
      case "$(scn requirement A)" in
        A) echo "designated => identifier \"app.axross.marshal\" and anchor apple generic and certificate leaf[subject.OU] = ABCDE12345 and certificate 1[field.1.2.840.113635.100.6.2.6] /* exists */ and certificate leaf[field.1.2.840.113635.100.6.1.13] /* exists */" ;;
        B) echo "designated => anchor apple generic and identifier \"app.axross.marshal\" and (certificate leaf[field.1.2.840.113635.100.6.1.9] /* exists */ or certificate 1[field.1.2.840.113635.100.6.2.6] /* exists */ and certificate leaf[field.1.2.840.113635.100.6.1.13] /* exists */ and certificate leaf[subject.OU] = \"ABCDE12345\")" ;;
        other-team) echo "designated => anchor apple generic and identifier \"app.axross.marshal\" and certificate 1[field.1.2.840.113635.100.6.2.6] /* exists */ and certificate leaf[field.1.2.840.113635.100.6.1.13] /* exists */ and certificate leaf[subject.OU] = ZZZZZ99999" ;;
        ad-hoc) echo "designated => cdhash H\"0123456789abcdef0123456789abcdef01234567\"" ;;
        none) ;;
      esac
    fi
    ;;
  hdiutil)
    # hdiutil create ... -srcfolder <stage> ... <image>: fails the scenario's
    # `hdiutil_failures` times first.
    failed=0
    if [[ -e "${RECORD}.hdiutil" ]]; then read -r failed <"${RECORD}.hdiutil"; fi
    if ((failed < $(scn hdiutil_failures 0))); then
      echo "$((failed + 1))" >"${RECORD}.hdiutil"
      echo "hdiutil: create failed - Resource busy" >&2
      exit 1
    fi
    stage=""
    previous=""
    for argument in "$@"; do
      [[ "$previous" == -srcfolder ]] && stage="$argument"
      previous="$argument"
    done
    grep -q '^# signed$' "${stage}/Marshal.app/Contents/MacOS/marshal-app" || exit 90
    [[ "$(readlink "${stage}/Applications")" == /Applications ]] || exit 87
    echo "a disk image" >"${*: -1}"
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
            *) echo "{\"id\": \"${SUBMISSION}\", \"message\": \"Processing complete\", \"name\": \"x.dmg\", \"path\": \"/x.dmg\", \"status\": \"${status}\"}" ;;
          esac
          exit "$(scn notary_exit 0)"
        fi
        if [[ "$2" == log ]]; then
          if [[ "$(scn log_fails 0)" == 1 ]]; then
            echo "Error: could not fetch the log" >&2
            exit 1
          fi
          echo "{\"jobId\": \"$3\", \"status\": \"$(scn notary_status Accepted)\"}" >"${*: -1}"
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
  s_password="$password"
  s_p8="-----BEGIN PRIVATE KEY-----
${p8_one}
${p8_two}
-----END PRIVATE KEY-----"
  s_key_id="$key_id"
  s_issuer="$issuer"
  s_team="$team"
  job_version="$version"
  job_app_sha256="$archive_sha256"
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
    secrets.MACOS_P12_PASSWORD) printf '%s' "$s_password" ;;
    secrets.MACOS_NOTARY_KEY_P8) printf '%s' "$s_p8" ;;
    secrets.MACOS_NOTARY_KEY_ID) printf '%s' "$s_key_id" ;;
    secrets.MACOS_NOTARY_ISSUER_ID) printf '%s' "$s_issuer" ;;
    vars.MACOS_TEAM_ID) printf '%s' "$s_team" ;;
    needs.plan.outputs.version) printf '%s' "$job_version" ;;
    needs.build.outputs.app_sha256) printf '%s' "$job_app_sha256" ;;
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
  mkdir -p "${case_dir}/ws/unsigned" "${case_dir}/tmp" "${case_dir}/bin"
  out="${case_dir}/out"
  record="${case_dir}/record"
  : >"$out"
  : >"$record"
  # the tools. a case may remove some.
  for tool in security codesign openssl plutil shasum xcrun ditto hdiutil; do
    ln -s "${work}/standin" "${case_dir}/bin/${tool}"
  done
  for tool in $missing_tools; do rm -f "${case_dir}/bin/${tool}"; done
  {
    printf 'p12_content=application p12 bytes\n'
    printf 'p12_password=%s\n' "$password"
    printf '%s\n' "$scenario_lines"
  } >"${case_dir}/scenario"

  # the files of marshal-macos-app.
  local unsigned="${case_dir}/ws/unsigned"
  printf '%s\n' "$archive_content" >"${unsigned}/${archive}"
  printf 'unsigned disk image\n' >"${unsigned}/${image}"
  case "$fixture_change" in
    "") ;;
    extra-file) printf 'helper\n' >"${unsigned}/fake-docker" ;;
    missing-dmg) rm "${unsigned}/${image}" ;;
    other-version) mv "${unsigned}/${archive}" "${unsigned}/marshal-9.9.9-macos-arm64-app.zip" ;;
    changed-archive) printf 'changed\n' >>"${unsigned}/${archive}" ;;
    symlink) rm "${unsigned}/${archive}" && ln -s "${unsigned}/${image}" "${unsigned}/${archive}" ;;
    *)
      echo "error: unknown fixture change ${fixture_change}" >&2
      exit 1
      ;;
  esac

  # the scripts, with a mutation if the case asks for one.
  cp -r "${work}/scripts" "${case_dir}/scripts"
  if [[ -n "$mutate_script" ]]; then
    sed -e "$mutate_expression" "${case_dir}/scripts/${mutate_script}" >"${case_dir}/mutated"
    mv "${case_dir}/mutated" "${case_dir}/scripts/${mutate_script}"
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
      "VERSION_OF_APP=${version}"
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
  for value in "$secret_application" "$password" "$p8_one" "$p8_two" "$key_id" "$issuer" "$keychain_password" "BEGIN PRIVATE KEY"; do
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
  local keychain="${case_dir}/tmp/marshal-signing.keychain-db" dir="${case_dir}/tmp/marshal-signing" stage="${case_dir}/tmp/marshal-dmg" expected sign_app build sign_image submit staple validate
  for expected in \
    "security create-keychain -p ${keychain_password} ${keychain}" \
    "security unlock-keychain -p ${keychain_password} ${keychain}" \
    "security import ${dir}/DeveloperIDG2CA.cer -k ${keychain} -t cert" \
    "security import ${dir}/application.p12 -k ${keychain} -f pkcs12 -P ${password} -T ${case_dir}/bin/codesign" \
    "security set-key-partition-list -S apple-tool:,apple: -k ${keychain_password} ${keychain}" \
    "security find-identity ${keychain}" \
    "security find-identity -v ${keychain}" \
    "security find-certificate -a -Z -c ${application_name} -p ${keychain}" \
    "ditto -x -k unsigned/${archive} app" \
    "codesign --force --sign ${application_identity} --keychain ${keychain} --options runtime --timestamp --identifier ${identifier} app/Marshal.app" \
    "codesign --verify --strict --verbose=2 app/Marshal.app" \
    "codesign --display --requirements - app/Marshal.app" \
    "ditto app/Marshal.app ${stage}/Marshal.app" \
    "hdiutil create -volname Marshal -srcfolder ${stage} -format UDZO -ov signed/${image}" \
    "codesign --force --sign ${application_identity} --keychain ${keychain} --timestamp --identifier ${identifier}.dmg signed/${image}" \
    "codesign --verify --strict --verbose=2 signed/${image}" \
    "xcrun notarytool submit signed/${image} --key ${dir}/AuthKey.p8 --key-id ${key_id} --issuer ${issuer} --wait --timeout 30m --output-format json" \
    "xcrun notarytool log ${submission} --key ${dir}/AuthKey.p8 --key-id ${key_id} --issuer ${issuer} ${case_dir}/tmp/marshal-notary/notary-log.json" \
    "xcrun stapler staple signed/${image}" \
    "xcrun stapler validate signed/${image}" \
    "xcrun --find notarytool" \
    "xcrun --find stapler"; do
    if ! grep -q -F -e "$expected" "$record"; then
      echo "the record has no call '${expected}'"
      return
    fi
  done
  if grep -q -E ' (pkgbuild|pkgutil|productsign) |Installer' "$record"; then
    echo "the job calls a package tool or names an Installer identity: $(grep -E ' (pkgbuild|pkgutil|productsign) |Installer' "$record" | head -n 1)"
    return
  fi
  sign_app="$(line_of "codesign --force --sign ${application_identity} --keychain ${keychain} --options runtime")"
  build="$(line_of " hdiutil create ")"
  sign_image="$(line_of "--identifier ${identifier}.dmg")"
  submit="$(line_of "notarytool submit")"
  staple="$(line_of "stapler staple")"
  validate="$(line_of "stapler validate")"
  if ! ((sign_app < build && build < sign_image && sign_image < submit && submit < staple && staple < validate)); then
    echo "the tools ran in the wrong order: sign the app ${sign_app}, build the image ${build}, sign the image ${sign_image}, submit ${submit}, staple ${staple}, validate ${validate}"
  fi
}

check_outputs() {
  local image_hash
  image_hash="$(sha256sum "${case_dir}/ws/signed/${image}" | cut -d' ' -f1)"
  for expected in "dmg_sha256=${image_hash}" "team_id=${team}" "submission_id=${submission}"; do
    grep -qxF "$expected" "${case_dir}/github_output" || {
      echo "the job outputs lack ${expected}"
      return
    }
  done
  grep -q '^# stapled$' "${case_dir}/ws/signed/${image}" || echo "the disk image hash was taken before stapling"
  grep -q '^# signed$' "${case_dir}/ws/signed/${image}" || echo "the disk image was not signed"
  [[ "$(find "${case_dir}/ws/signed" -mindepth 1 | wc -l | tr -d ' ')" -eq 1 ]] || echo "the signed directory holds other files than the disk image"
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

# check_two_retries: hdiutil create ran three times, two failures and a pass.
check_two_retries() {
  local calls
  calls="$(grep -c ' hdiutil create ' "$record" || true)"
  [[ "$calls" -eq 3 ]] || echo "hdiutil create ran ${calls} times, not 3"
}

check_not_signed() {
  ! grep -q -F -e "--identifier ${identifier}.dmg" "$record" || echo "the disk image was signed after hdiutil create failed"
}

# --- the cases ---------------------------------------------------------------

reset_case
run_case "a passing run" pass "Deleted the keychain, the certificate files and the key file." some check_flags
reset_case
run_case "a passing run's outputs, the signed disk image and the notary log" pass "Submission ${submission} was accepted." some check_outputs
reset_case
run_case "the notary log upload step reads the log's directory" pass "" some check_upload_path

echo "-- refusals before any security call --"
for name in MACOS_APPLICATION_P12_BASE64 MACOS_P12_PASSWORD MACOS_NOTARY_KEY_P8 MACOS_NOTARY_KEY_ID MACOS_NOTARY_ISSUER_ID MACOS_TEAM_ID; do
  reset_case
  case "$name" in
    MACOS_APPLICATION_P12_BASE64) s_application_p12="" ;;
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
for bad in "" "not-a-hash" "${archive_sha256}0" "$(tr 'a-f' 'A-F' <<<"$archive_sha256")"; do
  reset_case
  job_app_sha256="$bad"
  run_case "the build's app_sha256 '${bad:0:12}'" fail "is not a SHA-256" none
done
for change in extra-file missing-dmg other-version; do
  reset_case
  fixture_change="$change"
  run_case "an artifact with ${change}" fail "the downloaded files are not exactly" none
done
reset_case
fixture_change=symlink
run_case "an app archive that is a symlink" fail "is not a regular file" none
reset_case
fixture_change=changed-archive
run_case "an app archive that differs from the app job's hash" fail "not the ${archive_sha256} that the app job output" none
for tool in security codesign openssl plutil shasum xcrun ditto hdiutil; do
  reset_case
  missing_tools="$tool"
  run_case "a missing ${tool}" fail "${tool} is missing" none
done
for tool in notarytool stapler; do
  reset_case
  scenario_lines="xcrun_missing=${tool}"
  run_case "xcrun cannot find ${tool}" fail "xcrun cannot find ${tool}" none
done

reset_case
mutate_script="02-keychain.sh"
mutate_expression='s/^g2_sha256="f/g2_sha256="0/'
run_case "an embedded intermediate whose SHA-256 differs from the pin (the pin changed)" fail "not the pinned" none
reset_case
mutate_script="02-keychain.sh"
mutate_expression='s/^g2_base64="MIIEPj/g2_base64="MIIEPk/'
run_case "an embedded intermediate whose SHA-256 differs from the pin (the certificate changed)" fail "not the pinned" none

echo "-- refusals after the keychain exists --"
reset_case
s_application_p12=$'\n'
run_case "an Application secret that decodes to nothing" fail "application.p12 is empty after decoding its secret" some
for scenario in "application_count=0:holds 0 Developer ID Application identities" "application_count=2:holds 2 Developer ID Application identities"; do
  reset_case
  scenario_lines="${scenario%%:*}"
  run_case "${scenario%%:*}" fail "${scenario#*:}" some
done
reset_case
scenario_lines="invalid=application"
run_case "an Application identity that is not valid in the keychain" fail "is not valid in this keychain" some
reset_case
scenario_lines="application_team=ZZZZZ99999"
run_case "an Application certificate of another team" fail "Developer ID Application certificate names team 'ZZZZZ99999'" some
reset_case
scenario_lines="certificates=two"
run_case "a second Application certificate of the same name" fail "holds 2 certificates named '${application_name}', not exactly one" some
reset_case
scenario_lines="certificates=other"
run_case "an Application certificate that does not have the identity's hash" fail "does not have the SHA-1 hash ${application_identity} of the Developer ID Application identity" some
reset_case
scenario_lines="application_days=-5"
run_case "an expired Application certificate" fail "Application certificate expired on" some
reset_case
scenario_lines="application_days=30"
run_case "an Application certificate that expires within 90 days" pass "expires on Nov  3 12:00:00 2027 GMT, within 90 days" some check_warning
reset_case
scenario_lines="application_days=89"
run_case "an Application certificate that expires in 89 days" pass "within 90 days" some check_warning
reset_case
scenario_lines="application_days=91"
run_case "an Application certificate that expires in 91 days" pass "" some check_no_warning

echo "-- the app and its signature --"
reset_case
scenario_lines="layout=extra-entry"
run_case "an archive with more than Marshal.app" fail "not only Marshal.app" some
reset_case
scenario_lines="layout=extra-executable"
run_case "an app with a second executable" fail "Contents/MacOS holds" some
for nested in Frameworks PlugIns Helpers XPCServices Library; do
  reset_case
  scenario_lines="layout=${nested}"
  run_case "an app with Contents/${nested}" fail "the app holds Contents/${nested}, nested code" some
done
reset_case
scenario_lines="app_identifier=app.axross.other"
run_case "an app of another identifier" fail "the app's identifier is 'app.axross.other'" some
reset_case
scenario_lines="app_version=1.2.4"
run_case "an app of another version" fail "the app's version is '1.2.4', not ${version}" some
reset_case
scenario_lines="requirement=B"
run_case "a designated requirement in form B" pass "its designated requirement is form B" some
reset_case
run_case "a designated requirement in form A, its clauses in another order" pass "its designated requirement is form A" some
for requirement in other-team ad-hoc; do
  reset_case
  scenario_lines="requirement=${requirement}"
  run_case "a designated requirement that is ${requirement}" fail "is not the default one for ${identifier} and team ${team}" some
done
reset_case
scenario_lines="requirement=none"
run_case "no designated requirement" fail "codesign printed no designated requirement" some

echo "-- the disk image --"
reset_case
scenario_lines="hdiutil_failures=2"
run_case "hdiutil create that fails twice, then works" pass "trying again in 10 seconds" some check_two_retries
reset_case
scenario_lines="hdiutil_failures=3"
run_case "hdiutil create that fails three times" fail "hdiutil create failed 3 times" some check_not_signed

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
  call_line="$(sed -n "${call}p" "$record")"
  # a failed 90-day check is a warning, not a refusal, and hdiutil create is
  # tried again.
  if [[ "$injected" != cleanup && "$job_status" != fail && "$call_line" != *"-checkend 7776000" && "$call_line" != *" hdiutil create "* ]]; then
    problem="the job passed although call ${call} (${injected}) failed"
  fi
  if [[ -n "$problem" ]]; then
    failed_case "a failure injected at call ${call} ($(cut -d' ' -f2-4 <<<"$call_line"))" "$problem"
  fi
done
echo "ok   a failure injected at each of the ${calls} calls leaves nothing behind"
for step in check keychain identities unpack sign dmg notarize staple hash; do
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
