**GC ratchet hardening** (follow-up to #7045), three fixes found by watching the
gate actually run:

- **Unverified correctness is now a gate failure.** `gc_ratchet.py check` failed
  a probe whose stdout stopped matching the Node oracle, but passed one whose
  correctness could not be established at all (`unchecked` — Node missing, Node
  non-zero, or no oracle supplied). That made "we did not verify"
  indistinguishable from "we verified and it was fine", which is the exact hazard
  the oracle diff exists to close: a probe that silently stops allocating exits 0
  and reports a beautifully small retained heap, and the ratchet would have read
  that as a memory improvement. `check` now fails on any status other than
  `pass`, `validate_artifact` refuses a baseline pinned from an unverified run,
  and the CI measure step resolves the oracle explicitly rather than searching.
- **`main`-branch runs stopped cancelling each other.** The concurrency group was
  keyed only on `github.ref` with unconditional `cancel-in-progress`. `main` is
  busy and the macOS runner pool is deep enough that a run sits queued 40+
  minutes, so every merge cancelled the previous `main` run before it ever got a
  runner — three consecutive `main` runs cancelled, zero executed. A gate that is
  always cancelled never fails, which is `continue-on-error: true` in a different
  hat. Cancellation is now scoped to `pull_request`; `main` runs queue instead.
- **Cross-host behaviour is now measured, not assumed.** The first CI run
  executed on a 3-core virtualised M1 at load 25.6, against a baseline captured
  on an 8-core M1 at load 1.2. Retention (`heap_used_bytes`,
  `heap_total_bytes`) and five of the seven evacuation counters were
  **bit-identical on all eight probes**; `copied_objects`/`copied_bytes` drifted
  at most 0.06% (~80× inside their band); peak RSS stayed within 0.6%; wall time
  was **54–60% slower**. That last figure is the empirical justification for
  excluding wall time from the shared-CI profile — any band tight enough to catch
  a GC slowdown would have made every CI run red on day one. Recorded in
  `benchmarks/gc_ratchet/README.md`.

The #7045 baseline validates unchanged; nothing needs re-pinning.
