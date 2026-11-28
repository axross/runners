#!/usr/bin/env bash
# runs test-signed-dmg.sh against recording stand-ins of codesign, xcrun, spctl,
# plutil, hdiutil, diskutil and ditto, and produces each refusal: an unsigned,
# ad-hoc, misnamed, untimestamped, other team's or rootless disk image, one that is
# not stapled or that spctl rejects or does not call notarized, a notary log of
# another submission, another volume name, an extra file on the volume, an
# Applications that is not a link to /Applications, an app whose files or
# Info.plist differ from the unsigned archive's, an unsigned, ad-hoc, misnamed,
# other team's app or one without the hardened runtime, a designated requirement
# that is not the default one (another team, an extra clause, an alternative),
# an app spctl rejects, and a malformed team ID. a positive control passes first,
# so each refusal is shown to be the check named and not a failure the script
# always has; form B of the requirement and a volume with .DS_Store and .Trashes
# pass too. every run is checked for what the stand-ins saw: the disk image is
# detached whenever it was attached, and never attached after a refusal before it.
# the stand-in disk image is a text file whose `#@ key=value` lines say what the
# stand-in tools report, of it and, under `app_`, of the app on it; a later line
# overrides an earlier one. the stand-in archive is a tar file, which the stand-in
# ditto unpacks, so this runs on Linux as well as on macOS.
set -euo pipefail

script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/test-signed-dmg.sh"
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT

version=1.2.3
team=ABCDE12345
other_team=ZZZZZ99999
identifier=app.axross.marshal
submission=8c4f2a1e-0b3d-4a6e-9f7c-1d2e3f4a5b6c
other_submission=0d9e8f7a-6b5c-4d3e-8a2b-1c0d9e8f7a6b
# form A as codesign prints it, comments included, and form B as Xcode writes it.
requirement="anchor apple generic and identifier \"${identifier}\" and certificate 1[field.1.2.840.113635.100.6.2.6] /* exists */ and certificate leaf[field.1.2.840.113635.100.6.1.13] /* exists */ and certificate leaf[subject.OU] = ${team}"
requirement_b="anchor apple generic and identifier \"${identifier}\" and (certificate leaf[field.1.2.840.113635.100.6.1.9] /* exists */ or certificate 1[field.1.2.840.113635.100.6.2.6] /* exists */ and certificate leaf[field.1.2.840.113635.100.6.1.13] /* exists */ and certificate leaf[subject.OU] = ${team})"

mkdir -p "${work}/bin" "${work}/sbin"
# one stand-in answers to every tool's name.
cat >"${work}/standin" <<'STANDIN'
#!/usr/bin/env bash
set -euo pipefail
tool="$(basename "$0")"
echo "${tool} $*" >>"$RECORD"

last_argument() {
  local argument
  for argument in "$@"; do :; done
  echo "$argument"
}

# meta <path> <key>: the last `#@ key=value` line of the disk image for a file,
# or of its `app_` lines for a folder, which is the app on the mounted volume.
meta() {
  local file="$1" key="$2"
  if [[ -d "$file" ]]; then
    file="$(cat "${RECORD}.dmg")"
    key="app_${key}"
  fi
  { grep "^#@ ${key}=" "$file" || true; } | tail -n 1 | sed "s/^#@ ${key}=//"
}

case "$tool" in
  codesign)
    target="$(last_argument "$@")"
    case "$1" in
      --verify)
        if [[ "$(meta "$target" valid)" != 1 ]]; then
          echo "${target}: code object is not signed at all" >&2
          exit 1
        fi
        ;;
      --display)
        if [[ "$2" == --requirements ]]; then
          echo "Executable=${target}" >&2
          echo "designated => $(meta "$target" requirement)"
        else
          {
            echo "Executable=${target}"
            echo "Identifier=$(meta "$target" identifier)"
            if [[ "$(meta "$target" runtime)" == 1 ]]; then
              echo "CodeDirectory v=20500 size=1000 flags=0x10000(runtime) hashes=24+7 location=embedded"
            else
              echo "CodeDirectory v=20400 size=1000 flags=0x0(none) hashes=24+7 location=embedded"
            fi
            if [[ "$(meta "$target" adhoc)" == 1 ]]; then
              echo "Signature=adhoc"
            else
              echo "Signature size=9000"
              echo "Authority=$(meta "$target" authority)"
              echo "Authority=Developer ID Certification Authority"
              if [[ "$(meta "$target" rootless)" != 1 ]]; then
                echo "Authority=Apple Root CA"
              fi
            fi
            if [[ "$(meta "$target" timestamp)" == 1 ]]; then
              echo "Timestamp=Nov 3, 2026 at 10:00:00"
            else
              echo "Signed Time=Nov 3, 2026 at 10:00:00"
            fi
            echo "TeamIdentifier=$(meta "$target" team)"
          } >&2
        fi
        ;;
    esac
    ;;
  xcrun)
    # xcrun stapler validate <disk image>
    if [[ "$(meta "$3" stapled)" != 1 ]]; then
      echo "$3 does not have a ticket stapled to it." >&2
      exit 65
    fi
    echo "The validate action worked!"
    ;;
  spctl)
    target="$(last_argument "$@")"
    case "$(meta "$target" assess)" in
      notarized)
        echo "${target}: accepted" >&2
        echo "source=Notarized Developer ID" >&2
        ;;
      developer-id)
        echo "${target}: accepted" >&2
        echo "source=Developer ID" >&2
        ;;
      *)
        echo "${target}: rejected" >&2
        echo "source=Unnotarized Developer ID" >&2
        exit 3
        ;;
    esac
    ;;
  plutil)
    if [[ "$1" == -convert ]]; then
      cat "$(last_argument "$@")"
    else
      # plutil -extract <key> raw -o - <file>
      value="$(sed -n "s/.*\"$2\" *: *\"\([^\"]*\)\".*/\1/p" "$(last_argument "$@")" | head -n 1)"
      [[ -n "$value" ]] || exit 1
      echo "$value"
    fi
    ;;
  hdiutil)
    case "$1" in
      attach)
        image="$(last_argument "$@")"
        mountpoint=""
        previous=""
        for argument in "$@"; do
          [[ "$previous" == -mountpoint ]] && mountpoint="$argument"
          previous="$argument"
        done
        [[ " $* " == *" -readonly "* ]] || { echo "hdiutil: attach is not read-only" >&2; exit 1; }
        echo "$image" >"${RECORD}.dmg"
        cp -R -P "$(meta "$image" content)/." "$mountpoint"
        ;;
      detach) ;;
    esac
    ;;
  diskutil)
    printf '{"VolumeName": "%s"}\n' "$(meta "$(cat "${RECORD}.dmg")" volume)"
    ;;
  ditto)
    # ditto -x -k <archive> <folder>
    tar -xf "$3" -C "$4"
    ;;
esac
STANDIN
chmod +x "${work}/standin"
for tool in codesign xcrun plutil hdiutil ditto; do
  ln -s "${work}/standin" "${work}/bin/${tool}"
done
for tool in spctl diskutil; do
  ln -s "${work}/standin" "${work}/sbin/${tool}"
done

# make_app <folder>: a stand-in Marshal.app as the app job builds it.
make_app() {
  mkdir -p "$1/Contents/MacOS" "$1/Contents/Resources"
  printf '{"CFBundleIdentifier": "%s", "CFBundleVersion": "%s"}\n' "$identifier" "$version" >"$1/Contents/Info.plist"
  printf 'binary\n' >"$1/Contents/MacOS/marshal-app"
  printf 'icon\n' >"$1/Contents/Resources/icon.icns"
}

failures=0

# run_case <name> <exit status> <text the output must contain> <attached: yes|no>
# builds the files from the variables below, runs the script and checks the result
# and what the stand-ins saw. the variables are reset after each case.
dmg_meta=""
app_meta=""
volume_change=""
log_job="$submission"
team_argument="$team"
run_case() {
  local name="$1" want_status="$2" want_text="$3" want_attached="$4" directory output status=0 problem=""
  directory="${work}/case-${name//[^a-zA-Z0-9]/-}"
  mkdir -p "${directory}/unsigned" "${directory}/volume"
  make_app "${directory}/unsigned/Marshal.app"
  tar -cf "${directory}/app.tar" -C "${directory}/unsigned" Marshal.app
  make_app "${directory}/volume/Marshal.app"
  mkdir "${directory}/volume/Marshal.app/Contents/_CodeSignature"
  printf 'resources\n' >"${directory}/volume/Marshal.app/Contents/_CodeSignature/CodeResources"
  ln -s /Applications "${directory}/volume/Applications"
  case "$volume_change" in
    "") ;;
    extra) printf 'x\n' >"${directory}/volume/README.txt" ;;
    hidden) printf 'x\n' >"${directory}/volume/.DS_Store" && mkdir "${directory}/volume/.Trashes" ;;
    folder) rm "${directory}/volume/Applications" && mkdir "${directory}/volume/Applications" ;;
    elsewhere) rm "${directory}/volume/Applications" && ln -s /tmp "${directory}/volume/Applications" ;;
    app-file) printf 'x\n' >"${directory}/volume/Marshal.app/Contents/Resources/extra" ;;
    plist) printf '{"CFBundleIdentifier": "%s", "CFBundleVersion": "9.9.9"}\n' "$identifier" >"${directory}/volume/Marshal.app/Contents/Info.plist" ;;
    *) echo "error: unknown change ${volume_change}" >&2; exit 1 ;;
  esac
  {
    printf '#@ content=%s\n#@ volume=Marshal\n' "${directory}/volume"
    printf '#@ valid=1\n#@ identifier=%s.dmg\n#@ team=%s\n#@ adhoc=0\n#@ timestamp=1\n' "$identifier" "$team"
    printf '#@ authority=Developer ID Application: Test Maintainer (%s)\n#@ stapled=1\n#@ assess=notarized\n' "$team"
    printf '#@ app_valid=1\n#@ app_identifier=%s\n#@ app_team=%s\n#@ app_adhoc=0\n#@ app_timestamp=1\n#@ app_runtime=1\n' "$identifier" "$team"
    printf '#@ app_authority=Developer ID Application: Test Maintainer (%s)\n#@ app_assess=notarized\n' "$team"
    printf '#@ app_requirement=%s\n' "$requirement"
    printf '%s' "$dmg_meta" "$app_meta"
  } >"${directory}/signed.dmg"
  printf '{"jobId": "%s", "status": "Accepted"}\n' "$log_job" >"${directory}/notary-log.json"
  output="$(RECORD="${directory}/record" SYSTEM_BIN="${work}/bin" SYSTEM_SBIN="${work}/sbin" GITHUB_OUTPUT="${directory}/output" \
    bash "$script" "$version" "${directory}/app.tar" "${directory}/signed.dmg" "${directory}/notary-log.json" "$team_argument" "$submission" 2>&1)" || status=$?
  touch "${directory}/record"
  [[ "$status" -eq "$want_status" ]] || problem="exit status ${status}, expected ${want_status}"
  [[ -z "$problem" && "$output" != *"$want_text"* ]] && problem="the output does not contain '${want_text}'"
  if [[ -z "$problem" ]]; then
    if grep -q '^hdiutil attach' "${directory}/record"; then
      [[ "$want_attached" == yes ]] || problem="the disk image was attached after a refusal before the attach"
      grep -q '^hdiutil detach' "${directory}/record" || problem="the disk image was attached but not detached"
    elif [[ "$want_attached" == yes ]]; then
      problem="the disk image was never attached"
    fi
  fi
  if [[ -z "$problem" && "$want_status" -eq 0 ]] && ! grep -q '^designated_requirement=anchor apple generic' "${directory}/output"; then
    problem="the designated requirement was not output"
  fi
  if [[ -n "$problem" ]]; then
    echo "FAIL $name: ${problem}"
    while IFS= read -r line; do echo "    ${line}"; done <<<"$output"
    failures=$((failures + 1))
  else
    echo "ok   $name"
  fi
  dmg_meta=""
  app_meta=""
  volume_change=""
  log_job="$submission"
  team_argument="$team"
}

run_case "a disk image signed as the release signs it passes" 0 "All signed disk image checks passed" yes
app_meta="#@ app_requirement=${requirement_b}"$'\n'
run_case "form B of the designated requirement passes" 0 "(form B)" yes
volume_change=hidden
run_case ".DS_Store and .Trashes on the volume pass" 0 "All signed disk image checks passed" yes

dmg_meta=$'#@ valid=0\n'
run_case "an unsigned disk image" 1 "rejects the disk image" no
dmg_meta=$'#@ adhoc=1\n'
run_case "an ad-hoc disk image" 1 "the disk image has an ad-hoc signature" no
dmg_meta="#@ identifier=${identifier}"$'\n'
run_case "a disk image under the app's identifier" 1 "the disk image's identifier" no
dmg_meta=$'#@ timestamp=0\n'
run_case "a disk image without a secure timestamp" 1 "the disk image has a secure timestamp" no
dmg_meta="#@ team=${other_team}"$'\n'
run_case "a disk image of another team" 1 "the disk image's team" no
dmg_meta=$'#@ rootless=1\n'
run_case "a disk image whose chain does not reach Apple's root" 1 "the disk image's certificate chain reaches Apple's root" no
dmg_meta=$'#@ authority=Apple Development: Test Maintainer\n'
run_case "a disk image not signed by Developer ID Application" 1 "Developer ID Application certificate" no
dmg_meta=$'#@ stapled=0\n'
run_case "a disk image that is not stapled" 1 "it is not stapled" no
dmg_meta=$'#@ assess=rejected\n'
run_case "a disk image spctl rejects" 1 "spctl does not accept the disk image" no
dmg_meta=$'#@ assess=developer-id\n'
run_case "a disk image spctl does not call notarized" 1 "spctl names the notary service" no
log_job="$other_submission"
run_case "a notary log of another submission" 1 "the notary log is of the submission" no
dmg_meta=$'#@ volume=Marshal 1\n'
run_case "another volume name" 1 "the volume's name" yes
volume_change=extra
run_case "an extra file on the volume" 1 "the volume holds only Marshal.app and Applications" yes
volume_change=folder
run_case "an Applications that is a folder" 1 "Applications on the volume is not a link" yes
volume_change=elsewhere
run_case "an Applications that links elsewhere" 1 "Applications links to /Applications" yes
volume_change=app-file
run_case "an app with a file the archive lacks" 1 "the signed app's files are the unsigned app's" yes
volume_change=plist
run_case "an app whose Info.plist differs" 1 "Info.plist values are the unsigned app's" yes
app_meta=$'#@ app_valid=0\n'
run_case "an unsigned app" 1 "codesign --verify --strict rejects the app" yes
app_meta=$'#@ app_adhoc=1\n'
run_case "an ad-hoc app" 1 "the app has an ad-hoc signature" yes
app_meta="#@ app_identifier=${identifier}.dmg"$'\n'
run_case "an app under another identifier" 1 "the app's identifier" yes
app_meta="#@ app_team=${other_team}"$'\n'
run_case "an app of another team" 1 "the app's team" yes
app_meta=$'#@ app_runtime=0\n'
run_case "an app without the hardened runtime" 1 "the app has the hardened runtime" yes
app_meta="#@ app_requirement=${requirement//${team}/${other_team}}"$'\n'
run_case "a designated requirement of another team" 1 "is not the default one" yes
app_meta="#@ app_requirement=${requirement} and info[CFBundleVersion] = \"1\""$'\n'
run_case "a designated requirement with an extra clause" 1 "is not the default one" yes
app_meta="#@ app_requirement=${requirement} or anchor apple"$'\n'
run_case "a designated requirement with an alternative" 1 "is not the default one" yes
app_meta=$'#@ app_assess=rejected\n'
run_case "an app spctl rejects" 1 "spctl does not accept the app for execution" yes
team_argument="abc"
run_case "a team ID that is not one" 1 "is not a 10-character team ID" no

if [[ $failures -gt 0 ]]; then
  echo "${failures} case(s) failed" >&2
  exit 1
fi
echo "All signed disk image refusal cases passed."
