#!/usr/bin/env bash
# upgrades the base's ubuntu packages and installs what the signing key check
# needs, all from ubuntu's own archive.
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive NEEDRESTART_SUSPEND=1
apt_options=(
  -o DPkg::Lock::Timeout=600
  -o Acquire::Retries=3
  -o Dpkg::Options::=--force-confdef
  -o Dpkg::Options::=--force-confold
)

apt-get "${apt_options[@]}" update
apt-get "${apt_options[@]}" --yes dist-upgrade
apt-get "${apt_options[@]}" --yes --no-install-recommends install ca-certificates curl gnupg
