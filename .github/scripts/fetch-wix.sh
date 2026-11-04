#!/usr/bin/env bash
# installs the WiX command line tool into <tool directory> from the one NuGet
# package pinned below, and only after its SHA-256 matches. dotnet reads a
# NuGet.Config that clears every source and lists a folder holding that checked
# package alone, so nothing else is fetched. refresh the version and the hash
# together, the hash computed from the real download; see
# docs/conventions/security.md. runs on Linux in its test and on Windows in the
# release build, where sha256sum, curl, and cygpath come with Git for Windows.
set -euo pipefail

version="7.0.1"
sha256="25209a1f73b20106621f11abe5e1a4e87c1d9f5c596795d246de7bb64446df17"
url="https://api.nuget.org/v3-flatcontainer/wix/${version}/wix.${version}.nupkg"

if [[ $# -ne 1 ]]; then
  echo "Usage: fetch-wix.sh <tool directory>" >&2
  exit 2
fi
tool_directory="$1"

refuse() {
  echo "error: $*" >&2
  exit 1
}

# dotnet on Windows reads a Windows path, which Git Bash does not print.
native_path() {
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -m "$1"
  else
    printf '%s\n' "$1"
  fi
}

command -v dotnet >/dev/null 2>&1 || refuse "dotnet is not installed"

work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT
packages="${work}/packages"
package="${packages}/wix.${version}.nupkg"
mkdir "$packages"

curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 --retry 3 --max-time 180 \
  --output "$package" "$url" || refuse "WiX ${version} could not be downloaded from ${url}"

actual="$(sha256sum "$package")"
actual="${actual%% *}"
if [[ "$actual" != "$sha256" ]]; then
  refuse "SHA-256 mismatch for WiX ${version} (expected ${sha256}, got ${actual}); refusing to install it"
fi

nuget_config="${work}/NuGet.Config"
cat >"$nuget_config" <<CONFIG
<?xml version="1.0" encoding="utf-8"?>
<configuration>
  <packageSources>
    <clear />
    <add key="checked-wix" value="$(native_path "$packages")" />
  </packageSources>
</configuration>
CONFIG

DOTNET_NOLOGO=1 DOTNET_CLI_TELEMETRY_OPTOUT=1 dotnet tool install wix \
  --version "$version" \
  --tool-path "$(native_path "$tool_directory")" \
  --configfile "$(native_path "$nuget_config")"
echo "WiX ${version} is installed in ${tool_directory}."
