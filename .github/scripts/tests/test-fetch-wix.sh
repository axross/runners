#!/usr/bin/env bash
# runs fetch-wix.sh with stand-ins for curl and dotnet, so it needs no network
# and no .NET. a package whose SHA-256 differs from the pin must be refused
# before dotnet runs. a positive control runs a copy of the script with the pin
# edited to the stand-in package's hash, so the refusal is shown to be the hash
# check and not a failure the script always has, and it checks that dotnet is
# told to read a NuGet.Config that clears every source and lists one folder
# holding that package.
set -euo pipefail

script="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/fetch-wix.sh"
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT

mkdir "${work}/bin"
# the stand-in curl writes $FAKE_PACKAGE_TEXT to the file named by --output.
cat >"${work}/bin/curl" <<'CURL'
#!/usr/bin/env bash
set -euo pipefail
output=""
while [[ $# -gt 0 ]]; do
  if [[ "$1" == "--output" ]]; then
    output="$2"
    shift
  fi
  shift
done
printf '%s\n' "$FAKE_PACKAGE_TEXT" >"$output"
CURL
# the stand-in dotnet records its arguments, and the NuGet.Config and package
# they name, before the script's temporary directory is removed.
cat >"${work}/bin/dotnet" <<'DOTNET'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >"${DOTNET_RECORD}.args"
while [[ $# -gt 0 ]]; do
  if [[ "$1" == "--configfile" ]]; then
    cp "$2" "${DOTNET_RECORD}.config"
    source_folder="$(sed -n 's/.*<add key="[^"]*" value="\([^"]*\)".*/\1/p' "$2")"
    cp "${source_folder}"/*.nupkg "${DOTNET_RECORD}.package"
  fi
  shift
done
DOTNET
chmod +x "${work}/bin/curl" "${work}/bin/dotnet"

failures=0
check() {
  local name="$1" condition="$2"
  if eval "$condition"; then
    echo "ok   ${name}"
  else
    echo "FAIL ${name}" >&2
    failures=$((failures + 1))
  fi
}

# a package that is not the pinned one.
status=0
FAKE_PACKAGE_TEXT="corrupt download" DOTNET_RECORD="${work}/refused" \
  PATH="${work}/bin:$PATH" bash "$script" "${work}/tools" >"${work}/refused.out" 2>&1 || status=$?
check "a package with another SHA-256 is refused" "[[ $status -eq 1 ]]"
check "the refusal names the mismatch" "grep -q 'SHA-256 mismatch for WiX' '${work}/refused.out'"
check "dotnet does not run on a refused package" "[[ ! -e '${work}/refused.args' ]]"
check "nothing is installed from a refused package" "[[ ! -e '${work}/tools' ]]"

# the pin edited to the stand-in package's hash.
package_text="stand-in wix package"
package_hash="$(printf '%s\n' "$package_text" | sha256sum)"
package_hash="${package_hash%% *}"
sed "s/^sha256=\".*\"\$/sha256=\"${package_hash}\"/" "$script" >"${work}/fetch-wix-edited.sh"
check "the edit changed the pin" "[[ \"\$(grep -c '^sha256=\"${package_hash}\"\$' '${work}/fetch-wix-edited.sh')\" == 1 ]]"
status=0
FAKE_PACKAGE_TEXT="$package_text" DOTNET_RECORD="${work}/accepted" \
  PATH="${work}/bin:$PATH" bash "${work}/fetch-wix-edited.sh" "${work}/tools" >"${work}/accepted.out" 2>&1 || status=$?
check "a package with the pinned SHA-256 is installed" "[[ $status -eq 0 ]]"
check "dotnet installs the pinned version into the tool directory" \
  "grep -q -F 'tool install wix --version 7.0.1 --tool-path ${work}/tools --configfile ' '${work}/accepted.args'"
check "the NuGet.Config clears every source" "grep -q '<clear />' '${work}/accepted.config'"
check "the NuGet.Config lists one source" "[[ \"\$(grep -c '<add ' '${work}/accepted.config')\" == 1 ]]"
check "dotnet read the package that was checked" \
  "[[ \"\$(cat '${work}/accepted.package')\" == '${package_text}' ]]"

# the edit above matches by shape, so the committed pin must keep that shape.
check "the committed pin is a SHA-256" "grep -q -E '^sha256=\"[0-9a-f]{64}\"\$' '${script}'"

if [[ $failures -ne 0 ]]; then
  echo "${failures} WiX fetch case(s) failed." >&2
  exit 1
fi
echo "All WiX fetch cases passed."
