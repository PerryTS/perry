# changelog.d/ — per-PR changelog fragments

One markdown file per change, added **in the same PR as the code**:

    changelog.d/<PR-number>-<short-slug>.md

The file body is the changelog entry (same style the old CHANGELOG.md blocks
had), **without** a version header — the filename is keyed on the PR number
precisely so contributors don't need to know which patch version they'll land
as, and so two in-flight PRs never collide on the same file.

`changelog.d/` is the **pending queue**: it holds only fragments that no release
has shipped yet. `HIGHLIGHTS.md`, when present, is the hand-written summary
printed above a release's entries, and it belongs to the release being prepared.

## At release time: archive, never delete

A released fragment is not deleted. It moves into the folder of the version that
shipped it:

    changelog/v0.5.1520/<PR-number>-<short-slug>.md
    changelog/v0.5.1520/HIGHLIGHTS.md     # only when that release had one
    changelog/v0.5.1520/manifest.json     # version, source, entry count

Fold on the release branch, **before** the tag:

```bash
./scripts/cut_release_notes.sh --fold v0.5.1664 --dry-run   # what would move
./scripts/cut_release_notes.sh --fold v0.5.1664             # move + commit
```

`changelog.d/` is then empty and the next cycle starts from zero.

A fragment is archived **once, under the first release that shipped it**. A
later tag whose tree still carries an earlier release's entries — the shape of
a repository where that earlier fold never ran, which is exactly how
`changelog/v0.5.1664/` was produced — archives only its own new entries, clears
the already-archived ones from the queue anyway, and reports how many it
skipped.

Why a folder and not `git rm`: the fold used to be a manual *post-release* step
and it never once ran — 0 `fold changesets` commits, 4,211 fragments, 5.7 MB of
text, and every release body truncated at GitHub's 125,000-character limit
because the notes carried months of relaunched history. A release's notes also
have to stay reproducible: `--notes-only --version vX.Y.Z` reads
`changelog/vX.Y.Z/` when it exists, so re-running a release job after the fold
produces the same notes. Git history is not a substitute — `git log --
changelog.d/` cannot answer "what shipped in 0.5.1520" without a tree walk over
opaque slugs.

The fold archives **what the tag shipped**. A fragment edited after the release
keeps its shipped text in the archive, the edit stays in git history, and the
fold warns about it. Folding an already-folded version is refused.

`CHANGELOG.md` is a frozen archive of everything up to v0.5.1264 and no longer
grows. History lives in `changelog/v*/`, in the GitHub Releases, and in git.

## What keeps this from rotting again

`scripts/check_changelog_archive.py` is a required `lint` gate. It fails when an
archive is malformed (no `manifest.json`, wrong `version`, wrong entry count),
when one fragment is both archived and pending, when one fragment sits in two
release folders, or when the pending queue passes 6,000 fragments. That last number is a smoke alarm, not a style limit: a normal
cycle is large (v0.5.1520 shipped 1,509 entries; the 0.5.1664 cycle is ~2,700),
so the cap only trips on the signature of a fold that stopped happening for
several releases — the state that produced this folder layout in the first
place.

CI: a PR that touches `crates/` must add a fragment here (enforced in the
`lint` job). Apply the `skip-changelog` label to opt out — typo fixes,
CI-only churn, etc.

## Getting the number right

`scripts/check_changeset_fragment.sh` reads the fragment name a PR adds, and
treats the two mistake classes differently:

- an all-zero prefix (`changelog.d/0000-<slug>.md`) is a **hard failure** — no
  PR is ever number 0, so it is always an unfilled placeholder;
- a real but different number is a **warning**, because two legitimate shapes
  do that deliberately: a backfill renaming older fragments, and a stacked PR
  whose fragment names the parent change.

Getting it wrong is otherwise silent: nothing reads the fragment until a
release is cut, and it then attributes the change to a different PR.
`PENDING-<issue>-<slug>.md` does not match the shape the gate accepts
(`^changelog\.d/[0-9]+-`), so a new fragment is always named for its PR.
