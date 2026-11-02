#!/usr/bin/env bash
# Turn changelog.d/ fragments into a GitHub Release's notes, and archive them
# under the version that shipped them.
#
# Layout:
#
#   changelog.d/            the pending queue: fragments no release has shipped
#   changelog/vX.Y.Z/       exactly the entries that shipped in vX.Y.Z, plus a
#                           manifest.json and (when the release had one)
#                           HIGHLIGHTS.md
#
# Nothing is deleted at release time. A released fragment moves into its
# version's folder, so the release's notes stay reproducible from the tree and
# `git log --follow changelog/vX.Y.Z/<fragment>` still reaches the PR.
#
# Modes:
#
#   cut_release_notes.sh --notes-only [--version vX.Y.Z]
#       Print the release body (HIGHLIGHTS.md, then "## All changes" and the
#       concatenated entries) to stdout; change nothing. Notices go to stderr.
#       With --version, read changelog/vX.Y.Z/ when it exists and the pending
#       queue otherwise. Used by release-packages.yml's create-release job.
#
#   cut_release_notes.sh --fold vX.Y.Z [--dry-run] [--no-commit]
#       Move the entries that shipped in vX.Y.Z into changelog/vX.Y.Z/ and write
#       its manifest. Run it on the release branch BEFORE the tag: then the
#       archive is part of the reviewed, tagged commit and the fold is not a
#       leftover manual step nobody remembers. When the tag already exists the
#       fold archives what the TAG shipped (`git show`), not whatever the
#       worktree happens to hold. Folding an already-folded version is refused.
#
#   cut_release_notes.sh vX.Y.Z [--no-commit]
#       Legacy tag-first flow: create the tag + GitHub Release at HEAD from the
#       pending queue via `gh release create` (which triggers
#       release-packages.yml), then fold.
#
#   cut_release_notes.sh --self-test
#       Prove both directions on throwaway git repositories. Covers notes from
#       the pending queue, notes from an archive, the fold, tag-first folds that
#       must archive the tagged text, the refusal to fold twice, and --dry-run.
#       CHANGELOG_ROOT points a run at a fixture instead of this
#       checkout; it is test plumbing and has no other use.
set -euo pipefail
# CHANGELOG_ROOT is test-only: --self-test points every run at a
# throwaway fixture repository instead of this checkout.
ROOT="${CHANGELOG_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
cd "$ROOT"
# Absolute path to this script: the legacy mode re-executes it after cd,
# where a relative "$0" no longer resolves.
SELF_PATH="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"

# ---------------------------------------------------------------------------
# --self-test: prove the notes/fold funnel on throwaway git repositories.
# Written before the archive existed, so it is the specification of it.
# ---------------------------------------------------------------------------
st_failures=0
st_pass() { printf '  ok   %s\n' "$1"; }
st_fail() { printf '  FAIL %s\n' "$1" >&2; st_failures=$((st_failures + 1)); }
st_eq() { if [ "$2" = "$3" ]; then st_pass "$1"; else st_fail "$1: expected [$3] got [$2]"; fi; }
st_has() { case "$3" in *"$2"*) st_pass "$1" ;; *) st_fail "$1: missing [$2]" ;; esac; }
st_lacks() { case "$3" in *"$2"*) st_fail "$1: unexpected [$2]" ;; *) st_pass "$1" ;; esac; }

# Status and every stream, for refusals and notices.
st_run() {
  local dir="$1"; shift
  if st_out=$(CHANGELOG_ROOT="$dir" bash "$SELF" "$@" 2>&1); then
    st_status=0
  else
    st_status=1
  fi
}

# stdout only: the release body, with notices excluded.
st_notes() {
  local dir="$1"; shift
  st_out=$(CHANGELOG_ROOT="$dir" bash "$SELF" "$@" 2>/dev/null) || true
}

st_count() { find "$1" -maxdepth 1 -type f -name '*.md' ! -name README.md ! -name HIGHLIGHTS.md | wc -l | tr -d ' '; }
st_json() { python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))[sys.argv[2]])' "$1" "$2"; }

# A repository holding three pending entries, a README and highlights.
st_fixture() {
  local dir="$1"
  mkdir -p "$dir/changelog.d"
  (
    cd "$dir" || exit 1
    git init -q -b main .
    git config user.email selftest@example.invalid
    git config user.name 'cut_release_notes self-test'
    printf '# fragments\n' > changelog.d/README.md
    printf '## Highlights\n\nfixture highlights\n' > changelog.d/HIGHLIGHTS.md
    printf 'entry alpha\n' > changelog.d/100-alpha.md
    printf 'entry beta\n' > changelog.d/9-beta.md
    printf 'entry unkeyed\n' > changelog.d/unkeyed-slug.md
    git add -A
    git commit -qm fixture
  )
}

self_test() {
  local tmp before
  SELF="$SELF_PATH"
  tmp=$(mktemp -d)
  # shellcheck disable=SC2064
  trap "rm -rf '$tmp'" EXIT
  printf 'cut_release_notes.sh --self-test\n'

  # A. The pending queue is the notes source, newest PR first.
  st_fixture "$tmp/a" || { st_fail 'fixture a'; return 1; }
  st_notes "$tmp/a" --notes-only
  before="$st_out"
  st_has 'pending: highlights first' 'fixture highlights' "$before"
  st_has 'pending: entries heading' '## All changes' "$before"
  st_has 'pending: alpha entry' 'entry alpha' "$before"
  case "$before" in
    *'entry alpha'*'entry beta'*'entry unkeyed'*) st_pass 'pending: newest PR first' ;;
    *) st_fail 'pending: newest PR first' ;;
  esac

  # B. Folding the pending queue archives it instead of deleting it.
  st_run "$tmp/a" --fold v1.0.0 --no-commit
  st_eq 'fold: exit status' "$st_status" 0
  for f in 100-alpha.md 9-beta.md unkeyed-slug.md HIGHLIGHTS.md manifest.json; do
    if [ -f "$tmp/a/changelog/v1.0.0/$f" ]; then st_pass "fold: archived $f"; else st_fail "fold: archived $f"; fi
  done
  if [ -f "$tmp/a/changelog.d/README.md" ]; then st_pass 'fold: README stays pending'; else st_fail 'fold: README stays pending'; fi
  st_eq 'fold: pending queue emptied' "$(st_count "$tmp/a/changelog.d")" 0
  st_eq 'fold: manifest version' "$(st_json "$tmp/a/changelog/v1.0.0/manifest.json" version)" v1.0.0
  st_eq 'fold: manifest entry count' "$(st_json "$tmp/a/changelog/v1.0.0/manifest.json" entries)" 3

  # C. The folded version's notes come from its own folder, byte for byte.
  st_notes "$tmp/a" --notes-only --version v1.0.0
  st_eq 'archive: notes are reproducible' "$st_out" "$before"
  st_notes "$tmp/a" --notes-only --version 1.0.0
  st_eq 'archive: bare version accepted' "$st_out" "$before"

  # D. Folding twice is refused, not silently re-done.
  st_run "$tmp/a" --fold v1.0.0 --no-commit
  st_eq 'fold twice: exit status' "$st_status" 1
  st_has 'fold twice: names the archive' 'already' "$st_out"

  # E. A fragment merged after the fold belongs to the next release only.
  printf 'entry late\n' > "$tmp/a/changelog.d/999-late.md"
  st_notes "$tmp/a" --notes-only --version v1.0.0
  st_lacks 'archive: no double count' 'entry late' "$st_out"
  st_notes "$tmp/a" --notes-only
  st_has 'pending: late entry is pending' 'entry late' "$st_out"
  st_lacks 'pending: archived entries not replayed' 'entry alpha' "$st_out"

  # F. --dry-run writes nothing.
  st_fixture "$tmp/c" || { st_fail 'fixture c'; return 1; }
  st_run "$tmp/c" --fold v3.0.0 --dry-run
  st_eq 'dry run: exit status' "$st_status" 0
  if [ -e "$tmp/c/changelog/v3.0.0" ]; then st_fail 'dry run: wrote an archive'; else st_pass 'dry run: wrote nothing'; fi
  st_eq 'dry run: pending queue intact' "$(st_count "$tmp/c/changelog.d")" 3

  # G. Tag-first folds archive what the TAG shipped, not what the worktree holds.
  st_fixture "$tmp/b" || { st_fail 'fixture b'; return 1; }
  (
    cd "$tmp/b" || exit 1
    git tag v2.0.0
    printf 'entry alpha rewritten after the tag\n' > changelog.d/100-alpha.md
    rm changelog.d/9-beta.md
    git add -A
    git commit -qm after-tag
  )
  st_run "$tmp/b" --fold v2.0.0 --no-commit
  st_eq 'tag fold: exit status' "$st_status" 0
  st_eq 'tag fold: shipped text wins' "$(cat "$tmp/b/changelog/v2.0.0/100-alpha.md")" 'entry alpha'
  st_has 'tag fold: warns about a post-release edit' 'edited after v2.0.0' "$st_out"
  if [ -f "$tmp/b/changelog/v2.0.0/9-beta.md" ]; then st_pass 'tag fold: restores a deleted entry'; else st_fail 'tag fold: restores a deleted entry'; fi
  st_eq 'tag fold: pending queue emptied' "$(st_count "$tmp/b/changelog.d")" 0

  # H. Bad input is refused with a non-zero status.
  st_run "$tmp/a" --fold not-a-version
  st_eq 'invalid version: exit status' "$st_status" 1
  st_run "$tmp/a" --notes-only --version
  st_eq 'missing version value: exit status' "$st_status" 1

  printf '\n'
  if [ "$st_failures" -eq 0 ]; then
    printf 'self-test passed\n'
    return 0
  fi
  printf 'self-test FAILED: %s assertion(s)\n' "$st_failures" >&2
  return 1
}

for _arg in "$@"; do
  if [ "$_arg" = "--self-test" ]; then
    self_test
    exit $?
  fi
done

pending_dir="changelog.d"

die() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }

warn() {
  if [ "${GITHUB_ACTIONS:-}" = "true" ]; then
    printf '::warning::%s\n' "$*" >&2
  else
    printf 'warning: %s\n' "$*" >&2
  fi
}

# Entries are root-level regular files named `<PR>-<slug>.md` (or `<slug>.md`
# when the number was omitted). README.md is documentation and HIGHLIGHTS.md is
# the hand-written summary printed above the entries, so neither is an entry.
# Newest PR number first, unnumbered entries after them by name. The contract
# matches the changeset gate in test.yml and create-release in
# release-packages.yml. The order is taken from the BASENAME, never from a
# slash-delimited field: a pending path is `changelog.d/<name>` but an archived
# one is `changelog/vX.Y.Z/<name>`, and `sort -t/ -k2` would order the archive
# by its version segment instead of its PR number.
list_entries() {
  local dir="$1" name
  find "$dir" -maxdepth 1 -type f -name '[0-9]*.md' -exec basename {} \; \
    | sort -rn \
    | while IFS= read -r name; do printf '%s/%s\n' "$dir" "$name"; done
  find "$dir" -maxdepth 1 -type f -name '*.md' ! -name '[0-9]*' ! -name README.md ! -name HIGHLIGHTS.md | sort
}

# HIGHLIGHTS.md (hand-written summary) first, then every entry.
write_notes() {
  local dir="$1" entries="$2" f
  if [ -f "$dir/HIGHLIGHTS.md" ]; then
    cat "$dir/HIGHLIGHTS.md"
    printf '\n\n## All changes\n\n'
  fi
  while IFS= read -r f; do
    [ -n "$f" ] || continue
    cat "$f"
    printf '\n\n'
  done <<< "$entries"
}

notes_for() {
  local dir="$1" entries
  entries=$(list_entries "$dir")
  [ -n "$entries" ] || return 1
  write_notes "$dir" "$entries"
}

normalize_version() {
  case "$1" in
    v*) printf '%s' "$1" ;;
    *) printf 'v%s' "$1" ;;
  esac
}

validate_version() {
  case "$1" in
    v[0-9]*.[0-9]*.[0-9]*) printf '%s' "$1" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+$' ;;
    *) return 1 ;;
  esac
}

write_manifest() {
  local archive="$1" version="$2" source_kind="$3" count="$4" highlights="$5" folded_at
  folded_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  if [ "$highlights" = 1 ]; then highlights=true; else highlights=false; fi
  cat > "$archive/manifest.json" <<EOF
{
  "version": "$version",
  "source": "$source_kind",
  "entries": $count,
  "highlights": $highlights,
  "folded_at": "$folded_at"
}
EOF
}

mode="release"
version_raw=""
dry_run=0
no_commit=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --notes-only) mode="notes-only"; shift ;;
    --fold)       mode="fold"; shift ;;
    --dry-run)    dry_run=1; shift ;;
    --no-commit)  no_commit=1; shift ;;
    --version)
      [ "$#" -ge 2 ] || die "--version needs a value (vX.Y.Z)"
      version_raw="$2"
      shift 2 ;;
    -*) die "unknown option: $1" ;;
    *)
      version_raw="$1"
      shift ;;
  esac
done

if [ "$mode" = "notes-only" ]; then
  if [ -n "$version_raw" ]; then
    version=$(normalize_version "$version_raw")
    validate_version "$version" || die "not a release version: $version_raw (expected vX.Y.Z)"
    archive="changelog/$version"
    if [ -d "$archive" ]; then
      notes_for "$archive" || die "$archive holds no entries — the archive is empty."
    else
      warn "changelog/$version does not exist: these notes came from the pending queue in changelog.d/. Fold $version into changelog/ before the tag so its notes stay reproducible."
      notes_for "$pending_dir" || die "no fragments in changelog.d/ and no changelog/$version — nothing to release."
    fi
  else
    notes_for "$pending_dir" || die "no fragments in changelog.d/ — nothing to release."
  fi
  exit 0
fi

if [ "$mode" = "fold" ]; then
  [ -n "$version_raw" ] || die "usage: cut_release_notes.sh --fold vX.Y.Z [--dry-run] [--no-commit]"
  version=$(normalize_version "$version_raw")
  validate_version "$version" || die "not a release version: $version_raw (expected vX.Y.Z)"
  archive="changelog/$version"
  [ ! -e "$archive" ] || die "$archive already exists — $version is already folded. Nothing to do."

  highlights=0
  if git rev-parse -q --verify "refs/tags/$version" >/dev/null 2>&1; then
    # The release already exists: archive what the TAG shipped, so a fragment
    # edited or deleted after the tag is still archived as published.
    source_kind=tag
    entries=$(git ls-tree --name-only "$version" -- "$pending_dir/" \
      | grep -E "^$pending_dir/[^/]+\.md$" \
      | grep -vx "$pending_dir/README.md" || true)
    if git cat-file -e "$version:$pending_dir/HIGHLIGHTS.md" 2>/dev/null; then highlights=1; fi
  else
    source_kind=pending
    entries=$(list_entries "$pending_dir")
    if [ -f "$pending_dir/HIGHLIGHTS.md" ]; then highlights=1; fi
  fi
  [ -n "$entries" ] || die "no fragments recorded at $version — nothing to fold."
  count=$(printf '%s\n' "$entries" | grep -c . || true)

  if [ "$dry_run" = 1 ]; then
    printf 'Would archive %s entries (%s source) into %s:\n' "$count" "$source_kind" "$archive"
    printf '%s\n' "$entries" | sed 's/^/  /'
    [ "$highlights" = 0 ] || printf '  %s\n' "$pending_dir/HIGHLIGHTS.md"
    exit 0
  fi

  mkdir -p "$archive"
  while IFS= read -r path; do
    [ -n "$path" ] || continue
    base=${path##*/}
    if [ "$source_kind" = "tag" ]; then
      git show "$version:$path" > "$archive/$base"
      # A fragment edited after the release keeps its shipped text here; the
      # edit survives only in git history. Say so, never drop it quietly.
      if [ -f "$path" ] && ! cmp -s "$path" "$archive/$base"; then
        warn "$path was edited after $version was tagged: the archive keeps the tagged text, the edit stays in git history"
      fi
      git rm -q --ignore-unmatch -- "$path" >/dev/null 2>&1 || true
    else
      git mv -- "$path" "$archive/$base"
    fi
  done <<< "$entries"
  if [ "$highlights" = 1 ]; then
    if [ "$source_kind" = "tag" ]; then
      git show "$version:$pending_dir/HIGHLIGHTS.md" > "$archive/HIGHLIGHTS.md"
      git rm -q --ignore-unmatch -- "$pending_dir/HIGHLIGHTS.md" >/dev/null 2>&1 || true
    else
      git mv -- "$pending_dir/HIGHLIGHTS.md" "$archive/HIGHLIGHTS.md"
    fi
  fi
  write_manifest "$archive" "$version" "$source_kind" "$count" "$highlights"
  git add -- "$archive"
  if [ "$no_commit" = 1 ]; then
    printf 'Archived %s entries for %s in %s (not committed).\n' "$count" "$version" "$archive"
    exit 0
  fi
  git commit -q -m "chore(release): archive changelog fragments for $version"
  printf 'Archived %s entries for %s in %s. Push the archive commit.\n' "$count" "$version" "$archive"
  exit 0
fi

# Legacy tag-first flow: create the release from the pending queue, then fold.
[ -n "$version_raw" ] || die "usage: cut_release_notes.sh [--notes-only | --fold] vX.Y.Z"
version=$(normalize_version "$version_raw")
validate_version "$version" || die "not a release version: $version_raw (expected vX.Y.Z)"

notes=$(mktemp)
# shellcheck disable=SC2064
trap "rm -f '$notes'" EXIT
notes_for "$pending_dir" > "$notes" || die "no fragments in changelog.d/ — nothing to release."

# --target pins the tag to the checked-out commit; gh's default is the tip
# of the default branch, which may have moved past HEAD.
gh release create "$version" --target "$(git rev-parse HEAD)" --title "$version" --notes-file "$notes"
fold_args=(--fold "$version")
if [ "$no_commit" = 1 ]; then
  fold_args+=(--no-commit)
fi
rm -f "$notes"
trap - EXIT
exec bash "$SELF_PATH" "${fold_args[@]}"
