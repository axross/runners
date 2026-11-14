#!/usr/bin/env bash
# turns off the automatic download and installation of macOS updates and the App
# Store's automatic updates, then reads each setting back. The automatic check for
# updates stays on: the guest agent cannot turn it off, and a check installs nothing.
set -euo pipefail

settings=(
  "/Library/Preferences/com.apple.SoftwareUpdate AutomaticDownload"
  "/Library/Preferences/com.apple.SoftwareUpdate AutomaticallyInstallMacOSUpdates"
  "/Library/Preferences/com.apple.SoftwareUpdate CriticalUpdateInstall"
  "/Library/Preferences/com.apple.SoftwareUpdate ConfigDataInstall"
  "/Library/Preferences/com.apple.commerce AutoUpdate"
)
for setting in "${settings[@]}"; do
  read -r domain key <<<"$setting"
  defaults write "$domain" "$key" -bool false
done
for setting in "${settings[@]}"; do
  read -r domain key <<<"$setting"
  if [ "$(defaults read "$domain" "$key")" != "0" ]; then
    echo "error: $key in $domain is not off" >&2
    exit 1
  fi
done
