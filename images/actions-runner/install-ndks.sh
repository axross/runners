#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -m)" != x86_64 ]]; then
  echo "Preinstalled Android NDKs are available on Linux x64 only."
  exit 0
fi

work_dir="$(mktemp -d)"
trap 'rm -rf -- "$work_dir"' EXIT
mkdir -p "$ANDROID_SDK_ROOT/ndk"

while read -r version release checksum; do
  archive="$work_dir/$release.zip"
  curl -fsSL "https://dl.google.com/android/repository/android-ndk-$release-linux.zip" -o "$archive"
  printf '%s  %s\n' "$checksum" "$archive" | sha256sum --check --strict
  unzip -q "$archive" -d "$work_dir"
  ndk="$work_dir/android-ndk-$release"
  actual_version="$(sed -n 's/^Pkg.Revision[[:space:]]*=[[:space:]]*//p' "$ndk/source.properties")"
  if [[ "$actual_version" != "$version" ]]; then
    echo "NDK revision mismatch: expected $version, found $actual_version" >&2
    exit 1
  fi
  IFS=. read -r major minor micro <<< "$version"
  # sdkmanager's legacy package discovery does not recognize side-by-side
  # NDKs from source.properties alone, although Gradle's NDK locator does.
  cat > "$ndk/package.xml" <<XML
<?xml version="1.0" encoding="UTF-8"?>
<common:repository xmlns:common="http://schemas.android.com/repository/android/common/02" xmlns:generic="http://schemas.android.com/repository/android/generic/02" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <localPackage path="ndk;$version" obsolete="false">
    <type-details xsi:type="generic:genericDetailsType" />
    <revision><major>$major</major><minor>$minor</minor><micro>$micro</micro></revision>
    <display-name>NDK (Side by side) $version</display-name>
  </localPackage>
</common:repository>
XML
  mv "$ndk" "$ANDROID_SDK_ROOT/ndk/$version"
  rm -- "$archive"
done <<'NDKS'
27.0.12077973 r27 2f17eb8bcbfdc40201c0b36e9a70826fcd2524ab7a2a235e2c71186c302da1dc
27.1.12297006 r27b 33e16af1a6bbabe12cad54b2117085c07eab7e4fa67cdd831805f0e94fd826c1
NDKS
