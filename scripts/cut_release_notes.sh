#!/usr/bin/env bash
# Fold changelog.d/ fragments into a GitHub Release's notes.
#
# Modes:
#
#   cut_release_notes.sh vX.Y.Z
#       Legacy tag-first flow (maintainer, from a clean main at the commit to
#       release): creates the tag + GitHub Release at HEAD via `gh release
#       create` (which triggers release-packages.yml), then commits the
#       fragment removal — push that commit via the normal PR/bypass flow.
#
#   cut_release_notes.sh --notes-only
#       Print the concatenated fragment notes to stdout and change nothing.
#       Used by release-packages.yml's create-release job (tag-last flow) to
#       build the release body from the fragments at the release SHA.
#
#   cut_release_notes.sh --fold vX.Y.Z
#       Post-release cleanup for the tag-last flow: the release already
#       exists (created by CI), so only remove the fragments and commit.
#       Removes exactly the fragments recorded at the tag — fragments merged
#       to main AFTER the release SHA stay put for the next release.
set -euo pipefail
cd "$(dirname "$0")/.."

# Fragments are root-level regular files named <PR>-<slug>.md; README.md is
# documentation, not an entry. Sort by PR number descending (newest change
# first in the notes). Contract matches the changeset-gate job in test.yml
# and the create-release job in release-packages.yml.
list_fragments() {
  find changelog.d -maxdepth 1 -type f -name '[0-9]*.md' | sort -t/ -k2 -rn
}

concat_fragments() {
  local out="$1"
  while IFS= read -r f; do
    cat "$f" >> "$out"
    printf '\n\n' >> "$out"
  done <<< "$2"
}

mode="release"
case "${1:-}" in
  --notes-only) mode="notes-only"; shift ;;
  --fold)       mode="fold"; shift ;;
esac

if [ "$mode" = "notes-only" ]; then
  frags=$(list_fragments)
  [ -n "$frags" ] || { echo "ERROR: no fragments in changelog.d/ — nothing to release." >&2; exit 1; }
  notes=$(mktemp)
  concat_fragments "$notes" "$frags"
  cat "$notes"
  exit 0
fi

tag="${1:?usage: cut_release_notes.sh [--notes-only | --fold] vX.Y.Z}"

if [ "$mode" = "fold" ]; then
  # Only the fragments that existed at the release SHA were folded into the
  # release notes — remove exactly those.
  frags=$(git ls-tree --name-only "$tag" -- changelog.d/ | grep -E '^changelog\.d/[0-9][^/]*\.md$' || true)
  [ -n "$frags" ] || { echo "ERROR: no fragments recorded at $tag — nothing to fold." >&2; exit 1; }
  removed=0
  while IFS= read -r f; do
    if [ -f "$f" ]; then
      git rm -q -- "$f"
      removed=$((removed + 1))
    fi
  done <<< "$frags"
  [ "$removed" -gt 0 ] || { echo "ERROR: fragments at $tag are already gone from the worktree." >&2; exit 1; }
  git commit -m "chore(release): fold changesets into $tag release notes"
  echo "Folded $removed fragments for $tag. Push the removal commit."
  exit 0
fi

frags=$(list_fragments)
[ -n "$frags" ] || { echo "ERROR: no fragments in changelog.d/ — nothing to release." >&2; exit 1; }

notes=$(mktemp)
concat_fragments "$notes" "$frags"

# --target pins the tag to the checked-out commit; gh's default is the tip
# of the default branch, which may have moved past HEAD.
gh release create "$tag" --target "$(git rev-parse HEAD)" --title "$tag" --notes-file "$notes"
while IFS= read -r f; do
  git rm -q -- "$f"
done <<< "$frags"
git commit -m "chore(release): fold changesets into $tag release notes"
echo "Release $tag created ($(echo "$frags" | wc -l | tr -d ' ') fragments folded). Push the removal commit."
