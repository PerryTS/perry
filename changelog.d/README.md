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
