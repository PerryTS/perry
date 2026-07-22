# changelog.d/ — per-PR changelog fragments

One markdown file per change, added **in the same PR as the code**:

    changelog.d/<PR-number>-<short-slug>.md

The file body is the changelog entry (same style the old CHANGELOG.md blocks
had), **without** a version header — the filename is keyed on the PR number
precisely so contributors don't need to know which patch version they'll land
as, and so two in-flight PRs never collide on the same file.

At each release tag the maintainer runs:

    ./scripts/cut_release_notes.sh vX.Y.Z

which concatenates all fragments (newest PR first) into the GitHub Release
notes, then deletes them. History lives in GitHub Releases and in git history
(`git log -- changelog.d/`). `CHANGELOG.md` is a frozen archive of everything
up to v0.5.1264 and no longer grows.

CI: a PR that touches `crates/` must add a fragment here (enforced in the
`lint` job). Apply the `skip-changelog` label to opt out — typo fixes,
CI-only churn, etc.
