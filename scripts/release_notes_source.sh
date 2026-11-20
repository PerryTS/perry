#!/usr/bin/env bash
# Print the directory that holds a version's release-note entries.
#
#   release_notes_source.sh vX.Y.Z
#
# Prints `changelog/vX.Y.Z` when that archive exists (the fold already landed on
# the release branch) and `changelog.d` when the version is still pending, then
# exits 0. Exits 1 when neither holds an entry: that is the "nothing to
# release" condition the release preflight refuses on.
#
# The version's own archive wins, so a folded release keeps resolving to the
# entries it shipped even after new fragments are merged for the next release.
# One caller reads it for the truncation notice in release-packages.yml, so the
# count and the link name the set that was actually used.
#
# CHANGELOG_ROOT is test-only: it points a run at a fixture checkout.
set -euo pipefail
ROOT="${CHANGELOG_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
cd "$ROOT"

has_entries() {
  [ -n "$(find "$1" -maxdepth 1 -type f -name '*.md' \
    ! -name README.md ! -name HIGHLIGHTS.md -print -quit 2>/dev/null)" ]
}

[ "$#" -ge 1 ] || { printf 'usage: %s vX.Y.Z\n' "$0" >&2; exit 2; }

version="$1"
case "$version" in
  v*) ;;
  *) version="v$version" ;;
esac

if [ -d "changelog/$version" ] && has_entries "changelog/$version"; then
  printf 'changelog/%s\n' "$version"
  exit 0
fi
if has_entries changelog.d; then
  printf 'changelog.d\n'
  exit 0
fi
exit 1
