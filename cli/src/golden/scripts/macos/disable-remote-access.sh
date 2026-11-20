#!/usr/bin/env bash
# turns off Remote Login and Screen Sharing for good and stops them now, then
# checks that launchd keeps both disabled and runs neither. systemsetup cannot do
# this through the guest agent, which lacks Full Disk Access.
set -euo pipefail

services=(com.openssh.sshd com.apple.screensharing)
for service in "${services[@]}"; do
  launchctl disable "system/$service"
  # a service that is not loaded makes bootout fail, which is the state wanted
  launchctl bootout "system/$service" 2>/dev/null || true
done
disabled=$(launchctl print-disabled system)
for service in "${services[@]}"; do
  if ! grep -Fq "\"$service\" => disabled" <<<"$disabled"; then
    echo "error: $service is not disabled" >&2
    exit 1
  fi
  if launchctl print "system/$service" >/dev/null 2>&1; then
    echo "error: $service is still loaded" >&2
    exit 1
  fi
done
