**`MERGE_GUIDE.md`: the process for auditing and landing the open-PR queue.**
Captures the merge-train protocol (cherry-pick onto one branch, fix gates on
the train, validate once as a tree, land via `gh pr merge --rebase --admin` so
per-commit authorship survives, then prove `git diff origin/main train<N>` is
empty), the per-PR audit checklist, and the failure modes that have cost real
time here: a stale `refs/pull/<n>/head` landing code the author has already
replaced, a stale `libperry_{runtime,stdlib}.a` making both arms of an A/B
identical, `RUST_TEST_THREADS=1` for the two non-parallel-safe suites,
`--profile perry-dev` compiling out `debug_assert!`, and the rebase-merge
coherence stamp. Also records which merge-conflict shapes an automated resolver
mangles, and the rule for when a PR is held for end-to-end evidence rather than
landed: when its failure mode is silent (a hang, not an error) and the shipped
tests only prove mechanics. Referenced from `CLAUDE.md`.
