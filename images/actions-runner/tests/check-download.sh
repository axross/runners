#!/usr/bin/env bash
set -euo pipefail

installer="$(dirname "$0")/../install-ndks.sh"
work_dir="$(mktemp -d)"
trap 'rm -rf -- "$work_dir"' EXIT
mkdir -p "$work_dir/bin"
cat > "$work_dir/bin/uname" <<'UNAME'
#!/usr/bin/env bash
echo x86_64
UNAME
cat > "$work_dir/bin/curl" <<'CURL'
#!/usr/bin/env bash
printf 'corrupt download\n' > "${@: -1}"
CURL
cat > "$work_dir/bin/unzip" <<'UNZIP'
#!/usr/bin/env bash
touch "$UNZIP_WAS_CALLED"
UNZIP
chmod +x "$work_dir/bin/"*
if ANDROID_SDK_ROOT="$work_dir/sdk" UNZIP_WAS_CALLED="$work_dir/extracted" \
  PATH="$work_dir/bin:$PATH" bash "$installer" > "$work_dir/output" 2>&1; then
  echo "A corrupt NDK download was accepted." >&2
  exit 1
fi
grep -F FAILED "$work_dir/output"
test ! -e "$work_dir/extracted"
test ! -e "$work_dir/sdk/ndk/27.0.12077973"
cat > "$work_dir/bin/uname" <<'UNAME'
#!/usr/bin/env bash
echo aarch64
UNAME
ANDROID_SDK_ROOT="$work_dir/arm-sdk" PATH="$work_dir/bin:$PATH" bash "$installer"
test ! -e "$work_dir/arm-sdk"
echo "Checksum rejection and non-x64 checks passed."
