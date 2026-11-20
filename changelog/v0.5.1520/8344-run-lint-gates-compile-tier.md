### Changed

- **`scripts/run_lint_gates.sh` now covers the compile-level CI gates too.** It
  derived its command list from test.yml's `lint` job only, so it reported
  "all 48 gates passed" while `main` was red on the separate `warnings` job —
  which is what happened after #8333 left a test helper unused (fixed by
  #8339), through every PR audited in between.

  The new compile tier mirrors `warnings` (`cargo check --workspace
  --all-targets` under `-D warnings`) and `check` (`cargo clippy --workspace`),
  deriving the host-compatible package scope from
  `scripts/workspace_architecture.py` exactly as both jobs do, so it cannot
  drift from CI. `SKIP_COMPILE_GATES=1` keeps a fast path and the summary then
  says `(compile tier SKIPPED)`, so a fast run cannot be mistaken for a full one.
