### Fixed

- **GC: a time-budgeted incremental step no longer contains an unbounded atomic
  work unit (#7903).** `js_gc_step_us` consults its clock *between* work units
  and never during one, so the budget it advertises is exactly as strong as the
  most expensive single unit. Weak processing charged **one unit per registered
  holder** — and a `FinalizationRegistry` is one holder however many records it
  owns, so `process_finreg_after_mark` walked an arbitrarily long record array
  inside that one unit. A single registry with a million registrations was an
  atomic, heap-sized "work unit" sitting behind a time-budgeted API, and no
  amount of tightening the microsecond budget could reach it.

  Weak processing now charges **one unit per record** and keeps a cursor *into*
  the record array (`crates/perry-runtime/src/weakref/sliced.rs`).

  The previous atomicity was not accidental, and preserving what it protected is
  most of the change. `FinalizationRegistry.prototype.unregister` **rebuilds**
  the entries array without the matching records, so every index after a removed
  element shifts down; a resumed index-only cursor would skip exactly as many
  records as were removed before it, and a skipped record is a weak slot that
  never gets tombstoned — on a non-moving budgeted cycle its target is then
  swept and the slot dangles. That is a use-after-free, not a latency bug.

  So the cursor is validated rather than trusted: alongside the record index it
  carries the *identity* of the array it indexes (the value word of the
  registry's `entries` field plus that array's length). Both mutator mutation
  paths change one of the two — `unregister` installs a freshly built array,
  `register` pushes — so on resume a mismatch means the held indices are
  meaningless and that registry's scan restarts from 0 against the new array.
  Restarting is safe because a rescan is idempotent: the first pass writes
  `undefined` into a collected record's target and `false` into its pending flag
  after enqueueing, so a second pass enqueues nothing and clears nothing twice.
  Restart-on-mutation alone is livelock-shaped, so restarts are capped; past the
  cap a registry is finished in one atomic pass **and charged as such**. The
  worst case is therefore stated rather than implied.

- **Final root remark is now measured instead of claimed (#7903).**
  `AtomicFinalizeSubphase::FinalRootRemark` re-scans the roots and then drains
  everything they newly reach, both at `usize::MAX`. Its inline comment claimed
  the phase was "bounded by root-set size, not heap size" — true of the scan,
  **false of the drain**, since a root installed after the initial scan can
  anchor an arbitrarily large graph. Both ways to bound it were rejected on
  correctness grounds: yielding mid-drain invalidates the remark itself (the
  mutator installs new roots, so the scan must repeat, with no termination
  guarantee), and yielding after the liveness decision is the weak-read race
  tracked in #7900. The phase therefore stays deliberately atomic, is documented
  as such, and its cost is now reported separately from the general step maximum
  so a heap-sized pause cannot hide behind "the collector's worst step".

### Added

- **`[gc-step-bounds]` diagnostic line (#7903).** Emitted on the existing
  `PERRY_GC_DIAG=1` path — **no new environment knob**, per the GC knob
  kill-policy. Reports `step_max_us`, `final_remark_max_us` / `final_remarks`,
  `weak_records` / `weak_max_records_per_step`, `weak_steps_sliced`, and
  `weak_registry_restarts` / `weak_registry_atomic_finishes`.

  `weak_steps_sliced` is the **subject-was-live** counter: a run reporting `0`
  has not exercised the sliced path at all, whatever else it reports. Most
  programs register no weak holders and will legitimately report zeros across
  the whole line, which is exactly why the five new acceptance tests
  (`crates/perry-runtime/src/gc/tests/step_bounds.rs`) build the pathological
  registry directly and assert the liveness counter *before* asserting any
  bound — including an adversarial case where the mutator restructures the
  entries array in every single window, which must reach the bounded atomic
  fallback rather than restarting forever.

- `docs/src/internals/gc-step-bounds.md` — which collector phases are
  intentionally atomic, what each phase's defensible bound is, and how to read
  the new line.
