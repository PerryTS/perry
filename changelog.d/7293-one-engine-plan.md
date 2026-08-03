Adds `docs/engine-plan.md` as the single entry point for the engine effort —
correctness and performance — replacing five overlapping documents and a 55 KB
uncommitted working file that existed only on one machine.

Detail stays in the linked RFCs; sequencing and rationale live in the plan. Folds
in the representation-selection campaign's durable results: the framing (the fix
is in the proofs, not the value representation), the measured per-site win, the
net ~0% coverage result and why, the scoreboard change from promotion counts to
opaque `js_*` calls removed, and the three ways a promotion goes unconsumed.

Records the 2026-08-03 measurements that confirm the framing: the three worst
benchmarks lose on a missing *proof* (non-negativity plus an upper bound on an
array index, #7286 — worth 10.7x on matrix_multiply), not a missing
representation. Also records #7287, which contradicts the scoreboard by being
guard-bound with zero hot-path `js_*` calls while 7.9x behind.
