#!/usr/bin/env bash
set -euo pipefail

image="${1:?Usage: bash images/actions-runner/tests/smoke-test.sh <image>}"
test "$(docker image inspect --format '{{json (index .Config "Volumes")}}' "$image")" = null
test "$(docker image inspect --format '{{json (index .Config "Entrypoint")}}' "$image")" = null
docker run --rm --network none -i "$image" bash -s <<'SMOKE'
set -euo pipefail
test "$(id -un)" = runner
test -x /home/runner/run.sh
test -w /opt/hostedtoolcache
ccache --version > /dev/null
test "$ANDROID_HOME" = /home/runner/.android/sdk
test "$ANDROID_SDK_ROOT" = "$ANDROID_HOME"
test -w "$ANDROID_SDK_ROOT"
test ! -e /tmp/install-ndks.sh
test -z "${ANDROID_NDK_HOME:-}"
test -w "$HOME/.android"
printf 'sdk download cache can be written\n' > "$HOME/.android/cache/smoke"
work_dir="$(mktemp -d)"
trap 'rm -rf -- "$work_dir"' EXIT
printf 'sdk metadata can be written\n' > "$ANDROID_SDK_ROOT/repositories.cfg"
export JAVA_HOME="$RUNNER_TOOL_CACHE/Java_Temurin-Hotspot_jdk/17.0.20-101/x64"
export PATH="$JAVA_HOME/bin:$PATH"
test -f "$JAVA_HOME.complete"
test -w "$JAVA_HOME/release"
grep -Fx 'JAVA_RUNTIME_VERSION="17.0.20.1+1"' "$JAVA_HOME/release"
"$JAVA_HOME/bin/java" -version 2>&1 | grep -F '17.0.20.1'
"$JAVA_HOME/bin/javac" -version | grep -Fx 'javac 17.0.20.1'
printf 'public class Smoke { public static void main(String[] args) { System.out.println("jdk healthy"); } }\n' > "$work_dir/Smoke.java"
"$JAVA_HOME/bin/javac" "$work_dir/Smoke.java"
"$JAVA_HOME/bin/java" -cp "$work_dir" Smoke | grep -Fx 'jdk healthy'

for package in cmdline-tools/20.0 cmake/3.22.1; do
  test -f "$ANDROID_SDK_ROOT/$package/package.xml"
done
"$ANDROID_SDK_ROOT/cmdline-tools/20.0/bin/sdkmanager" --sdk_root="$ANDROID_SDK_ROOT" --list_installed > "$work_dir/inventory"
awk -F '|' 'NF >= 3 { gsub(/^[[:space:]]+|[[:space:]]+$/, "", $1); gsub(/^[[:space:]]+|[[:space:]]+$/, "", $2); print $1 " " $2 }' \
  "$work_dir/inventory" > "$work_dir/packages"
while read -r package revision; do
  grep -Fx "$package $revision" "$work_dir/packages"
done <<'PACKAGES'
cmdline-tools;20.0 20.0
platforms;android-36 2
build-tools;35.0.0 35.0.0
build-tools;36.0.0 36.0.0
platform-tools 37.0.1
cmake;3.22.1 3.22.1
ndk;27.0.12077973 27.0.12077973
ndk;27.1.12297006 27.1.12297006
PACKAGES
grep -Fx 'AndroidVersion.ApiLevel=36' "$ANDROID_SDK_ROOT/platforms/android-36/source.properties"
test -s "$ANDROID_SDK_ROOT/platforms/android-36/android.jar"
for version in 35.0.0 36.0.0; do
  "$ANDROID_SDK_ROOT/build-tools/$version/aapt2" version
  "$ANDROID_SDK_ROOT/build-tools/$version/zipalign" -f 4 \
    "$ANDROID_SDK_ROOT/platforms/android-36/android.jar" "$work_dir/aligned-$version.jar"
  "$ANDROID_SDK_ROOT/build-tools/$version/zipalign" -c 4 "$work_dir/aligned-$version.jar"
  "$ANDROID_SDK_ROOT/build-tools/$version/apksigner" version
done
"$ANDROID_SDK_ROOT/platform-tools/adb" --version | grep -F 'Version 37.0.1'
"$ANDROID_SDK_ROOT/platform-tools/fastboot" --version | grep -F '37.0.1'
"$ANDROID_SDK_ROOT/cmake/3.22.1/bin/cmake" --version | grep -E '^cmake version 3\.22\.1(-|$)'
"$ANDROID_SDK_ROOT/cmake/3.22.1/bin/ninja" --version

distribution_dir="$HOME/.gradle/wrapper/dists/gradle-9.3.1-bin/23ovyewtku6u96viwx3xl3oks"
archive="$distribution_dir/gradle-9.3.1-bin.zip"
test -w "$archive"
test "$(find "$HOME/.gradle" -type f)" = "$archive"
printf '%s  %s\n' 'b266d5ff6b90eada6dc3b20cb090e3731302e553a27c5d3e4df1f0d76beaff06' "$archive" | sha256sum --check --strict
unzip -tq "$archive"
unzip -q "$archive" -d "$work_dir"
"$work_dir/gradle-9.3.1/bin/gradle" --offline --no-daemon --version > "$work_dir/gradle-version"
grep -Fx 'Gradle 9.3.1' "$work_dir/gradle-version"

cat > "$work_dir/library.cpp" <<'CPP'
#include <string>
extern "C" int ndk_smoke() { return std::string("android").size(); }
CPP
for version in 27.0.12077973 27.1.12297006; do
  ndk="$ANDROID_SDK_ROOT/ndk/$version"
  grep -Fx "Pkg.Revision = $version" "$ndk/source.properties"
  grep -F "path=\"ndk;$version\"" "$ndk/package.xml"
  compiler="$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android31-clang++"
  "$compiler" -shared -fPIC -static-libstdc++ -Wl,-z,max-page-size=16384 \
    "$work_dir/library.cpp" -o "$work_dir/$version.so"
  readelf -h "$work_dir/$version.so" | grep -E 'Machine:.*AArch64'
  readelf --dyn-syms --wide "$work_dir/$version.so" | grep -w ndk_smoke
done
SMOKE

docker run --rm --network none "$image" bash -c \
  'set -euo pipefail
   printf "poisoned\n" > "$ANDROID_SDK_ROOT/ndk/27.1.12297006/source.properties"
   printf "poisoned\n" > "$ANDROID_SDK_ROOT/cmdline-tools/20.0/bin/sdkmanager"
   printf "poisoned\n" > "$RUNNER_TOOL_CACHE/Java_Temurin-Hotspot_jdk/17.0.20-101/x64/release"
   printf "poisoned\n" > "$HOME/.gradle/wrapper/dists/gradle-9.3.1-bin/23ovyewtku6u96viwx3xl3oks/gradle-9.3.1-bin.zip"'
docker run --rm --network none "$image" bash -c \
  'set -euo pipefail
   grep -Fx "Pkg.Revision = 27.1.12297006" "$ANDROID_SDK_ROOT/ndk/27.1.12297006/source.properties"
   export JAVA_HOME="$RUNNER_TOOL_CACHE/Java_Temurin-Hotspot_jdk/17.0.20-101/x64"
   grep -Fx '\''JAVA_RUNTIME_VERSION="17.0.20.1+1"'\'' "$JAVA_HOME/release"
   "$ANDROID_SDK_ROOT/cmdline-tools/20.0/bin/sdkmanager" --version | grep -Fx "20.0"
   archive="$HOME/.gradle/wrapper/dists/gradle-9.3.1-bin/23ovyewtku6u96viwx3xl3oks/gradle-9.3.1-bin.zip"
   printf "%s  %s\n" "b266d5ff6b90eada6dc3b20cb090e3731302e553a27c5d3e4df1f0d76beaff06" "$archive" | sha256sum --check --strict'
bash images/actions-runner/tests/diagnostic-smoke.sh "$image"
echo "Runner image smoke and tool isolation checks passed."
