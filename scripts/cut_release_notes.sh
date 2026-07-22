#!/usr/bin/env bash
# Fold changelog.d/ fragments into a GitHub Release's notes, then delete them.
#
# Usage (maintainer, from a clean main at the commit to release):
#   ./scripts/cut_release_notes.sh v0.5.1265
#
# Creates the tag + GitHub Release at HEAD via `gh release create` (which
# triggers release-packages.yml, same as today), then commits the fragment
# removal — push that commit via the normal PR/bypass flow.
set -euo pipefail
cd "$(dirname "$0")/.."

tag="${1:?usage: cut_release_notes.sh vX.Y.Z}"

# Fragments are <PR>-<slug>.md; README.md is documentation, not an entry.
# Sort by PR number descending (newest change first in the notes).
frags=$(find changelog.d -name '[0-9]*.md' | sort -t/ -k2 -rn)
[ -n "$frags" ] || { echo "ERROR: no fragments in changelog.d/ — nothing to release." >&2; exit 1; }

notes=$(mktemp)
while IFS= read -r f; do
  cat "$f" >> "$notes"
  printf '\n\n' >> "$notes"
done <<< "$frags"

gh release create "$tag" --title "$tag" --notes-file "$notes"
# shellcheck disable=SC2086  # word-splitting intended; fragment names have no spaces
git rm -q $frags
git commit -m "chore(release): fold changesets into $tag release notes"
echo "Release $tag created ($(echo "$frags" | wc -l | tr -d ' ') fragments folded). Push the removal commit."
