### perf(gc): the first copying minor decides its promotion from its own trace (#7937)

Whole-block in-place promotion (#7742/#7888) could never apply to the **first**
copying minor of a thread: the policy reads the *previous* cycle's measured
young-survival ratio and cycle 0 has no previous cycle. On the fully-live
workloads that one cycle was 58–81% of all GC pause.

Cycle 0 now **attempts** the promotion and decides afterwards, from the ratio it
just measured. That is possible because a promoting cycle's trace is exactly a
mark pass over the blocks it would keep — `retag_young_for_in_place_promotion`
runs *before* the trace, after which no address classifies as `Nursery`, so
`move_young` is unreachable and the Cheney pass degenerates into marking. Below
`FIRST_CYCLE_PROMOTE_SURVIVAL_PERMILLE` (500) the attempt **rolls back** and the
cycle re-runs as an ordinary copying minor.

Measured on `gc-handoff/bench` + `gc-handoff/apps` against `main` @ `d78efca41`
(19/19 byte-exact against `node --experimental-strip-types`, exit 0, both arms
built here), instructions retired and peak RSS — both load-independent, which
the dev box at load 35–97 required:

| program | cycles | full | instructions | peak RSS |
|---|---|--:|--:|--:|
| `retain1` | 4 → **2** | 1 → **0** | **−77.0%** | **−28.3%** |
| `deeplist` | 4 → **2** | 1 → **0** | **−76.9%** | **−29.0%** |
| `retain_wide1` | 5 → **3** | 1 → **0** | **−66.0%** | **−5.1%** |
| `retain_wide` | 9 → **7** | 2 → **1** | **−41.4%** | **−2.8%** |
| `retain` | 8 → **5** | 2 → **1** | **−41.0%** | **−10.8%** |
| `asyncpipe` | 1 → 1 | 0 | +2.1% | +0.3% |
| `shapes` | 1 → 1 | 0 | +1.5% | +0.8% |
| `churn` `churn_alloc` `push_cls` `push_num` `cycles` `tree` `tree_wide` `interp` `iso_miss` `pipeline` `churn_read` `fib40` | unchanged | 0 | ±0.1% | ±0.5% |

**No program gains a cycle or a full collection**, and five lose one or two of
each. The two regressions are the wasted mark of a rolled-back attempt over
6 536 and 6 686 live objects — the price of the measurement, paid once. Wall
clock is deliberately not quoted: the dev box ran at load 35–97, and absolute
seconds belong to the quiet mini.

★ Part of that headline is `main` being in a bad state rather than this change
being that good. Between `8260a9e50` and `d78efca41` the `retain` cluster
regressed on `main` alone — `retain` 2 840 M → 12 421 M instructions with 0 → 2
`old_gen_bytes` fulls, `retain1` 1 396 M → 3 724 M, `deeplist` 1 172 M →
3 910 M — and the fulls are the same granularity-sensitive arm described below.
Measured against the earlier base, this change is worth −14% to −31%. Both
numbers are real; the second is the one to quote for the mechanism.

#### The rollback, and why its obligation list is two items long

`undo_in_place_promotion_retag` (the retag is the only physical commitment —
nothing moved, no block changed arenas, no bump pointer moved, and
`finish_in_place_promotion` has not run) and `clear_marks`. Everything else the
attempt did is a *provable* no-op on a promoting cycle: every root and slot
rewrite (nothing moved), and every remembered-set insertion — `skip_remembering`
is a **precondition** of attempting at all, which is why the attempt is gated on
`malloc_registry_empty_at_start`. It is also gated on
`untraced_promotion_instrument_veto`, so the stress/verify instruments are never
shown a discarded trace.

★ A retag is not only a relabel: `retag_block_space` to `HeapGeneration::Old`
also mints one zeroed `OldPageMeta` per **4 KB** of every block it touches, so a
speculative attempt mints one for the whole young generation. Measured cost when
it did: `tree_wide` 67.8 → 74.8 MB, `tree` 35.8 → 39.3, `churn` 25.0 → 26.6 —
+4–10% peak RSS on the 11 programs whose cycle 0 rolls back. **Removing the
entries in the rollback does not give it back** — a `HashMap` never returns its
capacity, and 61 MB of young generation is 15 616 pages in a table grown to
32 768 slots. So the attempt does not mint them:
`retag_block_space_deferring_old_page_registration`, sound exactly there because
a speculative attempt requires `skip_remembering`, which is the proof that no
remembered-set insertion — the only reader that wants a promoted page's metadata
before it exists — can happen between the retag and the finish. On commit,
`finish_in_place_promotion` mints them on the way past.

Both halves of the rollback are **sabotage-verified**: deleting the retag undo,
or minting the page metadata eagerly, each turns
`a_first_cycle_attempt_that_its_own_trace_refutes_rolls_back_and_evacuates` red
(`copied_objects: 0` and `old_pages: 256 vs 0` respectively).

#### The `old_gen_bytes` absolute arm is granularity-sensitive, and that had to be fixed first

`old_reclaim_pressure_due`'s first arm is `old_in_use >= T && baseline < T`, and
`baseline` is credited by every promotion
(`credit_promoted_bytes_to_old_baseline`) — so it is a race between two
quantities moving in the same direction at different step sizes. **Whether it
fires depends on the size of the promotion steps, not on any property of the
heap.** Instrumented on `retain`: same program, same live set, same total
promotion, and changing the schedule from (18.7 MB, 34.6 MB) to
(17.7 MB, 17.8 MB) makes it fire twice — `old_in_use=52.3 MB, baseline=35.5 MB,
T=48 MB` — buying two full mark-sweeps that cost **588 ms against a 55 ms GC
budget**, with the proportional arm correctly declining (`band=128 MB`) and
`GC_MAJOR_PACING_RETAINING` armed.

That is the futile-full shape #7592 removed one arm over: a heap whose young
generation is not dying is retaining live data, and a full mark-sweep cannot
lower the number being watched. #7592 exempted the *proportional* arm via
`GC_MAJOR_PACING_RETAINING` and left the absolute one un-exempted; it is the
absolute one that fires. It now stands down while that latch is armed. The
proportional arm still bounds old-gen growth, so this defers reclamation rather
than removing it — the same trade the existing multiplier makes. Both states are
asserted by `the_absolute_old_reclaim_arm_stands_down_on_a_retaining_heap`.

#### Also

* New promotion telemetry `in_place_dead_blocks` / `in_place_dead_block_bytes`:
  promoted blocks with **zero** live objects, i.e. the ones a promotion kept for
  nothing. Distinct from `in_place_sparse_blocks` (under 50% live) and the
  distinction is load-bearing — on a speculatively promoting cycle 0 `churn`
  reads 17 sparse of 18 blocks but 15 fully dead, `tree_wide` 61 and 60.
* `FIRST_CYCLE_PROMOTE_SURVIVAL_PERMILLE` is 500 rather than the steady-state
  950 because it bounds a different thing: 950 bounds a *prediction* that can be
  wrong repeatedly, while cycle 0 is reading its own measurement and can only be
  wrong once, for at most `(1 − ratio) ×` the scavenge nursery cap. On today's
  corpus the cycle-0 population is bimodal with an empty 25→992 band and 950
  would classify it identically — but three days ago `asyncpipe`'s cycle 0
  measured **770‰ over 172 415 objects**, squarely between the modes, and at 950
  it would have paid that whole mark pass and then rolled back. #7959 changed
  its shape and the outlier vanished. A threshold that only works while the
  corpus happens to be bimodal is fitted to a coincidence; this one is justified
  by its exposure.
