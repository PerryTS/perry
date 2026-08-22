#!/usr/bin/env bash
# Print the explicitly governed perry-ext-* package set for release builds.

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
registry="$repo_root/workspace-architecture.json"

packages=$(
  sed -n \
    's/^[[:space:]]*"\(perry-ext-[A-Za-z0-9-]*\)"[[:space:]]*:[[:space:]]*{[[:space:]]*$/\1/p' \
    "$registry" | sort
)
if [ -z "$packages" ]; then
  echo "no release extension packages found in $registry" >&2
  exit 1
fi

printf '%s\n' "$packages"
