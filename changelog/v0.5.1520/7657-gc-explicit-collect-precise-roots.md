### `gc()` runs on precise roots — the forced conservative stack scan is gone (#7558)

An explicit `gc()` was the one collection site in Perry that forced the
conservative native-stack scan. It no longer does: it consumes the same precise
root set every automatic collection in a production binary already uses
(`conservative_stack_scan_mode()` → `Auto` → `SkipDisabled`).

**What the scan was.** A workaround for a precise-rooting hole, not a property
`gc()` needs. #4977 reported `const keep = {…}; gc(); keep.nested.deep` reading
dangling-pointer garbage — a module-init/top-level local held only as a
native-stack alloca that neither the shadow stack nor the module-var scanners
covered — and #4998 forced the scan at the one site that could hide it. The hole
was later closed from the other end by the 2026-06→08 rooting campaign
(persistent shadow slots bound in function-entry setup, #6968/#6951/#6972;
`@perry_global_*` cells registered via `js_gc_register_global_root`; the
root-store-dominates-every-collection-point invariant gated by
`scripts/gc_root_dominance_check.py` with an empty allowlist). `js_gc_collect` is
a collection point by that invariant like any other, and a **full mark-sweep on
precise roots** already ships automatically at the microtask-pump safepoint
(#7148). The scan at `gc()` was grandfathered.

**What it cost.** Every retained-heap number this project quotes is read from
`process.memoryUsage()` after a `gc()`, so every one of them carried a
stack-residue term — non-deterministic run to run, and much larger than the 16%
on one probe that #7558 was filed for. `gc_ratchet.py classify` on `main`
`961777904`: **28.63% / 28.24% / 29.71% / 31.03%** of reported retention on
probes `01_nursery_churn`, `05_closure_capture`, `06_string_retention`,
`11_collect_at_depth`, 13.80% on `12_large_live_set`, non-zero on nine of
twelve. On this build the same command reports **excess 0.00% and spread 0 on
all twelve**, and `[gc-scan-fallback] site=manual_collect` no longer appears.

**A real behaviour change came with it.** `gc/tenuring.rs` deliberately refuses
to seed the adaptive tenuring threshold from a conservatively-scanned cycle, so
on any `gc()`-driven workload the seed had never fired and `tenuring_survivals`
sat at its power-on `4`. It fires now: on `09_try_catch_roots` and
`11_collect_at_depth` the threshold falls `4 → 1` and every survivor is promoted
on first copy — `copied_objects` 5,823 → 0 with `promoted_objects` 0 → 6,077,
and 5,830 → 0 with 0 → 6,150. `PERRY_GC_DIAG` on both arms confirms the copying
minor still ran (`eligible=true`, `[gc-copy-minor] ran`) and moved *more*
objects, to old-gen instead of survivor space; `heap_total_bytes` is
byte-identical, `freed_bytes` within 0.08%, `heap_used_bytes` and RSS fall, and
probe stdout is unchanged.

**Ratchet changes that follow from it.**

* `tolerances.json` `probe_overrides` is now **empty**. The #7554 entry taking
  `12_large_live_set.heap_used_bytes` out of the gating family is deleted
  because its cause is gone, not because it became inconvenient — that cell is
  bit-identical again over the pinned run and gates again.
* `check`'s liveness rule now asserts `copied_objects + promoted_objects > 0`
  rather than `copied_objects > 0`. Both counters are parsed from the same
  `[gc-copy-minor] ran` line and each names a *destination*; only the sum
  answers "did the copying minor move anything". Without this, the re-pin would
  have pinned `copied_objects = 0` on two probes and made the rule's `base > 0`
  guard permanently false exactly where it had most recently fired.
  `copied_objects` keeps its own two-sided band, so the same shift is still a
  `-100%` REGRESSION that must be re-pinned deliberately.
* `benchmarks/gc_ratchet/baseline/gc-ratchet-v1.json` is re-pinned on the pinned
  quiet host, with `main` at `961777904` reproducing the *previous* artifact
  (`gc-ratchet: OK`) on the same host and toolchain first, so every delta is
  attributable. The artifact regains single provenance (#7652).
* `classify` gains a second job: `excess 0` is now the expected reading on every
  row, so a non-zero `excess` column *is* the finding — either a forced scan came
  back at `gc()`, or an automatic site started firing on that workload.

**Detector, sabotage-verified.**
`explicit_gc_collects_precisely_and_a_native_stack_plant_dies` plants a
pointer-shaped word (NaN-boxed and raw-I64) in a live native-stack frame as the
only reference to a real GC object, calls `js_gc_collect()` from that frame, and
asserts the object is swept — with the per-thread scan override *cleared*, since
the test-isolation guard's pinned `Auto` would otherwise make a reintroduced
`force_full_scan` a silent no-op. `the_native_stack_plant_survives_when_the_scan_is_pinned_on`
runs the identical plant with the scan pinned `Full` and asserts it **survives**,
so a green detector means "the plant was findable and was not found" rather than
"the plant never landed". Re-adding the force makes the detector fail and leaves
the control green.

`ConservativeScanSite::ManualCollect` is deleted rather than kept
unconstructible, for the reason the `HostPressure` note in the same enum already
gives: an arm nothing can produce is a claim no test can check, and its `count=0`
would read as "the site is quiet" when the truth is "the site is gone".

`perry/gc` `minor()` deliberately keeps its forced scan: removing it there makes
the *copying* minor eligible, so the collection starts relocating survivors
rather than merely retaining less — a different risk with its own proof
obligation.
