**`fix(gc-ratchet)`: record a rebase-stable provenance field (#7666 follow-up).**

A gc-ratchet re-pin is written on a branch and **rebased at merge time** (the maintainer adds the version bump), which orphans the commit the pin recorded. #7666's artifact names `a8f73122d` — the object still resolves in a local clone, but `git merge-base --is-ancestor a8f73122d origin/main` is false, so it is not in the history a future reader will search.

This is not #7652's substance (that was 143 cells from one commit and one from another; provenance here is single and uniform). It is #7652's **shape**, and unlike #7652 it recurs on *every* pin rather than on one surgical edit.

The artifact now also records `code_tree` — the tree hash of `crates/`, i.e. exactly the code whose behaviour a probe measures. It survives the rebase **and** the version bump, which touches only `Cargo.toml` / `Cargo.lock` / `CLAUDE.md`. Resolving it is a one-liner against any candidate commit: `git rev-parse <commit>:crates` either matches or does not.

Backfilled on the shipped baseline as `dd8020788…`, verified identical at both the orphaned `a8f73122d` and the measurement commit `7bde3de24` — which is itself the check that the field means what it claims.

Two tests, both sabotage-verified: the shipped artifact must carry a 40-hex `code_tree` (removing it reddens), and `code_tree_hash()` must return `HEAD:crates` rather than `HEAD` — a commit hash there would be the very field it replaces and would change on every version bump, which is half of what makes `commit` useless for this. `code_tree_hash()` returns `"unknown"` rather than raising when git is unavailable: a missing provenance note must not fail a measurement run.
