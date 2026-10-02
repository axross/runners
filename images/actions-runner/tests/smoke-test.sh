#!/usr/bin/env bash
set -euo pipefail

image="${1:?Usage: bash images/actions-runner/tests/smoke-test.sh <image>}"
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
  'printf "poisoned\n" > "$ANDROID_SDK_ROOT/ndk/27.1.12297006/source.properties"'
docker run --rm --network none "$image" bash -c \
  'grep -Fx "Pkg.Revision = 27.1.12297006" "$ANDROID_SDK_ROOT/ndk/27.1.12297006/source.properties"'
echo "Runner image smoke and SDK isolation checks passed."
