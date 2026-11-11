#!/usr/bin/env bash
# writes the apt source on standard input to the path $1, installs the packages
# in the arguments after $2 (each name=version) at those versions and holds
# them, then adds the user $2 to the docker group.
set -euo pipefail

sources=$1
guest_user=$2
shift 2
export DEBIAN_FRONTEND=noninteractive NEEDRESTART_SUSPEND=1
apt_options=(
  -o DPkg::Lock::Timeout=600
  -o Acquire::Retries=3
  -o Dpkg::Options::=--force-confdef
  -o Dpkg::Options::=--force-confold
)

staging=$(mktemp "$sources.XXXXXX")
trap 'rm -f "$staging"' EXIT
cat >"$staging"
test -s "$staging"
chmod 0644 "$staging"
mv -f "$staging" "$sources"

apt-get "${apt_options[@]}" update
apt-get "${apt_options[@]}" --yes --no-install-recommends install "$@"
for spec in "$@"; do
  apt-mark hold "${spec%%=*}"
done
for spec in "$@"; do
  installed=$(dpkg-query --show --showformat='${Version}' "${spec%%=*}")
  if [ "$installed" != "${spec#*=}" ]; then
    echo "error: ${spec%%=*} is at $installed, not ${spec#*=}" >&2
    exit 1
  fi
done
usermod --append --groups docker "$guest_user"
