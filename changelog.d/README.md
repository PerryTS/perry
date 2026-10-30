# changelog.d/ — per-PR changelog fragments

One markdown file per change, added **in the same PR as the code**:

    changelog.d/<PR-number>-<short-slug>.md

The file body is the changelog entry (same style the old CHANGELOG.md blocks
had), **without** a version header — the filename is keyed on the PR number
precisely so contributors don't need to know which patch version they'll land
as, and so two in-flight PRs never collide on the same file.

At release time the fragments become the GitHub Release notes (newest PR
first), then get deleted. On the default tag-last flow (`/release` →
release-packages.yml with `cut_release=true`), CI concatenates them via
`cut_release_notes.sh --notes-only` when it creates the release, and the
maintainer afterwards runs:

    ./scripts/cut_release_notes.sh --fold vX.Y.Z

which removes exactly the fragments recorded at the tag and commits. The
legacy tag-first path (`./scripts/cut_release_notes.sh vX.Y.Z`) does both in
one go. History lives in GitHub Releases and in git history
(`git log -- changelog.d/`). `CHANGELOG.md` is a frozen archive of everything
up to v0.5.1264 and no longer grows.

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
