#!/usr/bin/env bash
# writes the apt source on standard input to the path $1, creates the docker group
# with the id $3, installs the packages in the arguments after $3 (each
# name=version) at those versions and holds them, then adds the user $2 to the
# group. the id is fixed because the runner image's docker group has it.
set -euo pipefail

sources=$1
guest_user=$2
docker_gid=$3
shift 3
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

if ! getent group docker >/dev/null; then
  groupadd --system --gid "$docker_gid" docker
fi
docker_group=$(getent group docker)
if [ "${docker_group#docker:x:"$docker_gid":}" = "$docker_group" ]; then
  echo "error: the docker group is not $docker_gid: $docker_group" >&2
  exit 1
fi

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
