#!/usr/bin/env bash
# prints gpg's machine-readable listing of the key file at $1, from a throwaway
# home so that nothing is written to the user's.
set -euo pipefail

home=$(mktemp -d)
trap 'rm -rf "$home"' EXIT
LC_ALL=C gpg --homedir "$home" --batch --no-tty --show-keys --with-colons "$1"
