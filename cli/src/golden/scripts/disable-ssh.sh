#!/usr/bin/env bash
# locks the password of the user $1 and masks the ssh service and its socket, so
# the base image's password login cannot come back with a package upgrade, then
# checks both.
set -euo pipefail

guest_user=$1
passwd --lock "$guest_user"
systemctl mask --now ssh.service ssh.socket

if [ "$(passwd --status "$guest_user" | awk '{ print $2 }')" != "L" ]; then
  echo "error: the password of $guest_user is not locked" >&2
  exit 1
fi
for unit in ssh.service ssh.socket; do
  if [ "$(systemctl is-enabled "$unit" 2>/dev/null || true)" != "masked" ]; then
    echo "error: $unit is not masked" >&2
    exit 1
  fi
  if systemctl is-active --quiet "$unit"; then
    echo "error: $unit is still running" >&2
    exit 1
  fi
done
