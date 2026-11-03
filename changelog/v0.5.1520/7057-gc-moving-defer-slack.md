Fixed the moving-GC deferral so the copying minor is reachable under an explicit
heap budget, and made the per-PR GC matrix arms assert that it actually ran.

`gc_check_trigger`'s deferral arm — the one that hands an allocation-point
nursery trigger to the next precise-root safepoint, and therefore to the copying
minor #7019 shipped — was guarded by an absolute committed-arena cap derived from
`budget_scaled(128 MB, 1, 4, 2 MB)`. That is byte-for-byte the formula behind
`gc_trigger_absolute_ceiling_bytes()`, so under any heap budget small enough for
the ceiling to reach the 16 MB nursery cap (every `PERRY_GC_HEAP_LIMIT` ≤ 64, and
every device budget a small container or watch-class device derives) the two
collapse to one number. A nursery trigger is due exactly when
`arena_total_bytes() >= trigger` while the deferral required
`arena_total_bytes() < cap`: same number, so the two predicates were exact
complements and the deferral was unreachable. Control fell through to the
alloc-point minor under `ManualGcScanGuard::force_full_scan()`, the collector
reported `[gc-copy-minor] eligible=false fallback=conservative_stack`, and a
heap-limited deployment silently ran the pre-#7019 non-moving collector. Measured
on the representation corpus at `--pressure 8`: the `default` arm collected on 13
of 22 rows and ran **zero** copying minors on all 22.

The allowance is now a slack measured **from the deferral point**
(`GC_MOVING_DEFER_SLACK_BYTES`, `gc_moving_defer_slack_dyn_bytes()`): the first
deferral of a cycle is unconditional, and the safety valve fires once the arena
has grown one slack past it, retiring the pending request so the baseline cannot
go stale and pin the deferral off for the rest of the process. A delta cannot
collapse into an absolute trigger at any heap budget. No env knob was added.

The `default` arm now runs a real copying minor — `[gc-copy-minor] ran
copied_objects=3576 … eligible=true fallback=none` on
`test_gap_repsel_canonical_i32`, 8 604 215 objects over 154 copying minors on
`test_gap_repsel_gc_stress` — 12 of 22 corpus rows, up from 0. Full matrix
(`--arms all --pressure 8`, 440 cells) moves `PASS=324 UNVER=91 XFAIL=1 FAIL=24`
to `PASS=325 UNVER=100 XFAIL=1 FAIL=14`; all 14 residual failures are the single
pre-existing #6981 cell `test_gap_repsel_p4a3_numarray_barriers`, and no corpus
row broke that was not already red.

That also closes the per-PR gate hole in #6993: the relocating-minor defect class
was previously invisible to `--arms pr`, and the proof it no longer is, is a cell
that changed colour — `default × test_gap_repsel_p4a3_numarray_barriers` went
from PASS (`cycles=1 scavenged=0`) to FAIL (`exit=139 scavenged=3594`), the same
SIGSEGV only the push-only evacuating arms could produce before.

Gate mechanics hardened alongside: a `scavenge` liveness requirement reads the
copying minor's own `[gc-copy-minor] ran copied_objects=` counter rather than the
sum that also counts the C4b mark-sweep evacuation (#7025), and is given to the
four arms whose subject is the relocating young-gen minor. A compiled-program
regression test (`crates/perry/tests/gc_copy_minor_under_heap_limit.rs`) pins the
observable that distinguishes the two worlds under `PERRY_GC_HEAP_LIMIT=8`:
`copied_objects > 0`. Exit 0 does not — the broken build exited 0 and collected.

One measured consequence is recorded rather than smoothed over:
`test_gap_specabi_reassign`, a 20-line program with no loop and therefore no
back-edge poll to drain the deferral, now exits before collecting under the
pressure knob and is reported UNVER instead of PASS.
