### gc: a heap where nothing dies stopped being marked over and over

`gc-handoff/bench/retain.ts` builds a 3 M-element array of `{a, b}` records,
keeps every one of them live, then sums a field. **Nothing is ever garbage**,
and Perry spent **1.26 s of a 1.31 s run inside the collector** (96%). Node
does it in 0.13 s. Two independent causes, both measured on the pinned quiet
M1 against `c156f8a41`.

#### Two full mark-sweeps that reclaimed 4 MB between them

`PERRY_GC_TRACE=1 ./n_retain` before:

| cycle | kind | pause | arena in-use before → after |
|---|---|--:|---|
| 3 | **full** | 160.6 ms | 67 → 63 MB |
| 6 | **full** | 483.5 ms | 204 → 204 MB |

`arena_growth_full_escalation_due` escalates a minor to a full once arena
live-bytes pass K× (K = 2) the live set measured after the last full. Its own
doc-comment claimed that "a workload with a legitimately large *stable* live
set (retain-style) does not over-escalate — its arena hovers near its own
baseline". `retain`'s live set is not stable, it **grows**: every doubling
crosses the threshold, and each resulting full marks a heap where nothing has
died. 644 ms of pause for 4 MB, and the second full moved arena in-use by
exactly zero.

So price a full by what it reclaims. A full that shrinks arena in-use by less
than `MAJOR_PACING_PRODUCTIVE_YIELD_PCT` (20%) shifts the next escalation
threshold left by one — capped at 2, so the multiplier tops out at 8× — and a
productive full resets the shift to 0 in one step. The yield is deliberately
measured on the **same** metric the escalation gate reads, so the two cannot
disagree about whether a full helped, and it is deliberately scoped to
*escalated* fulls: an explicit `gc()` never moves the backoff, or a
`gc()`-in-a-loop test would drive it straight to the cap. Old-gen garbage is
unaffected either way — `old_reclaim_pressure_due` still forces its own full.

Churn-shaped workloads reclaim most of the heap on every full, hold the shift
at 0, and are paced exactly as before.

#### Every evacuated object took a process-global mutex to hash an empty map

`object_static_prototype_owner_moved` — the `ObjectOverflowFields` move hook,
run once per moved object — took a `Mutex<HashMap<usize, u64>>` and ran a
SipHash `remove` against the residual `Object.setPrototypeOf` registry.

That registry is empty in any program that never re-prototypes a
non-meta-capable owner, and a latch already says so: `OBJECT_PROTOTYPES_NONEMPTY`,
stored `Release` *before* the insert, and read by both siblings
(`object_static_prototype`, `prune_dead_object_prototype_owners`). The move
hook was the one reader that skipped it, so a 3 M-record promotion paid 2.5 M
lock/unlock pairs and 2.5 M hash probes against an empty map. That is why a
single-threaded benchmark profiled with `pthread_mutex_lock` and
`std::hash::random::RandomState` in its top ten.

#### The bug this nearly shipped as

The first cut recorded the pre-full arena reading at the two
`gc_start_budgeted_cycle_for_pressure` call sites. Both correct, both the wrong
sites: the escalation the shipped safepoint path actually takes lives in
`gc::gc_collect_minor_with_trigger_inner`, so the reading was never recorded,
`update_major_pacing_backoff` early-returned on every cycle, and the change
measured **1.31 s → 1.28 s** — the mutex fix alone — with every test green and
the decision function correct in isolation.

The recording now happens **inside** `arena_growth_full_escalation_due` on the
`true` verdict, so a call site added later is priced by construction, and the
GC trace gained a `major_pacing` block (`baseline_bytes`, `backoff_shift`,
`escalate_above_bytes`) so the subject can be asserted live rather than
inferred from a green run. That block is what showed the backoff sitting at 0
through all seven cycles.

#### Not fixed here, measured and named

`retain` is still ~5× Node, and the residue is one number: **~220 ns of
per-object bookkeeping for every promoted object** (2.5 M promotions on this
workload), spread over `arena_alloc_gc_old`, `layout_transfer`,
`old_page_account_promoted_object`, the move hooks, and two page-generation
classifications per visited slot. The structural answer is V8-style whole-page
promotion — relabel a nearly-all-live Eden block as old-gen instead of
evacuating it object by object — which needs none of that per-object work.
Second on the list: `gc::verify::restore_surviving_dirty_coverage` walks
**every** slot of a parent on a pre-cycle dirty page, where the scan it repairs
(`scan_dirty_object_slots`) walks only the slots on dirty pages — 8.8% of a
pure-retain profile, re-walking the whole 3 M-element backing store on every
minor.
