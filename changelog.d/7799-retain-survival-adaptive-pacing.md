### GC: a heap whose young generation is not dying no longer schedules full mark-sweeps that free nothing

`retain.ts` **0.542 s → 0.345 s**, `retain_wide.ts` **1.099 s → 0.454 s**, `retain1` 0.299 → 0.134,
`retain_wide1` 0.276 → 0.157, and — not targeted — `deeplist.ts` 0.245 → 0.123.
Peak RSS *fell* on every one of them. Quiet M1 mini, best-of-5, vs `origin/main` @ `0a2bf15bd`.

These programs build a multi-million-element array of records and keep every one
alive. Nothing is ever garbage. Before this change **79% of `retain` and 88% of
`retain_wide` was GC pause**, and most of that pause was full mark-sweeps that
found the heap fully live:

| bench | fulls | what each reclaimed |
|---|--:|---|
| `retain` | 1 (161 ms) | 11.9% |
| `retain_wide` | 2 (98 + 512 ms) | 6.8%, then 9.6% |
| `deeplist` | 1 (127 ms) | **0.0%** |
| `tree` / `tree_wide` | 40 each | 87.8% / 92.3% |

#### 1. Survival-adaptive major pacing

`arena_growth_full_escalation_due` escalated a minor to a full once the arena
grew past `growth_num` (default 2) times the last full's live set. "Escalate when
the heap doubles" is the right rule for a heap that accumulates garbage and the
wrong rule for one that does not: when **everything allocated stays alive**,
doubling is the program working, and each escalation marked a bigger all-live
heap than the last.

#7726/#7733's retrospective backoff cannot fix this. It prices a full *after*
paying for it, and on a monotonically growing live heap deferring a full only
makes the next one more expensive — there is no schedule of futile fulls that is
cheap. The useless full has to be predicted.

The prediction is a measurement the collector already takes. `young_survival_permille`
separates the two populations by two orders of magnitude, with nothing in between:

| workload | young survival (permille) |
|---|--:|
| `churn`, `churn_alloc`, `push_cls` | 0 – 4 |
| `cycles` | 0 |
| `shapes` | 713 – 920 |
| `retain`, `retain_wide`, `deeplist` | 999 – 1000 |

So a copying minor that measures ≥ 900 permille now marks the heap RETAINING,
which (a) multiplies the escalation growth band by 4 and (b) re-baselines
arena-growth pacing on the occupancy that survived the collection. (b) is what
makes (a) reachable: before the first full the baseline is 0, so the boundary
degenerates to the absolute `PERRY_GC_MAJOR_PACING_FLOOR_MB`, and **any** program
retaining more than 32 MB paid a whole-heap mark-sweep for doing so, once,
unconditionally.

The same signal and the same multiplier are applied to `old_reclaim_pressure_due`'s
growth band. `credit_promoted_bytes_to_old_baseline` (#7592) already exempts
old-gen growth a minor proved live, but a large object is allocated *straight into*
old-gen and never passes through promotion, so its bytes are uncredited growth even
when they are the program's live data — on `retain.ts` that is the element array
itself. With the arena-growth escalation correctly declining, this band became the
binding constraint and fired a 452 ms full that reclaimed 7.6%: the identical
futile-full shape, one trigger over, reached by the same route.

Two properties keep the blast radius small, and both are asserted:

* **It can only make fulls rarer, never more frequent.** The baseline only
  ratchets up and the multiplier is ≥ 1, so the escalation boundary is never
  lower than before. The exposure is deferred reclamation, not extra pauses —
  and measured peak RSS went *down* on every affected benchmark, because the
  fulls being skipped were reclaiming 0–12%.
* **A single non-retaining minor disarms it**, with no decay window, so a heap
  that stops retaining paces tightly again on its very next collection. `tree` /
  `tree_wide` run no minors at all, never arm it, and are unchanged (40 fulls
  each, 87.8% / 92.3% yield, same wall time).

`retaining` is emitted in the `major_pacing` GC trace object: a run that never
armed the band must not be indistinguishable from one that did and had nothing
to skip.

#### 2. An all-pointer array's dirty-card scan was O(live array), not O(dirty pages)

`scan_dirty_object_slots` has two arms. Its `Range` arm intersects a slot range
with the remembered set's dirty-page set directly; its `Slot` arm answers "is
this slot on a dirty page?" with a **hash-set probe per slot**. A JS array whose
elements are all pointers selects `LayoutSlotMask::AllPointers`, which reported
itself as `Masked` and therefore emitted one `Slot` descriptor per element — so a
3M-element array cost 3M probes on every minor, to find the few hundred pages the
remembered set already knew were dirty. `dirty_slot_ranges_scanned == 0` in every
`retain.ts` GC trace was recording exactly this: the cheap arm was never reached.

`AllPointers` selects every index in `0..slot_count`, which *is* a contiguous
range, so it now emits one `Range`. Worth 9% on `retain` on its own, before any
pacing change.

`dirty_slot_ranges_for` gained a second traversal arm at the same time: it walked
the whole dirty-page set per range, which is right for one huge array and
quadratic for a heap holding many small pointer ranges. It now walks whichever
side is smaller.

#### 3. Two hot-path readings that were paid for and not used

* `classify_heap_space_in_range` is split into an `#[inline(always)]` cache-hit
  arm and an `#[inline(never)]` miss arm, exactly as #7469 did for its sibling
  `classify_heap_generation` and for the same reason: with the map lookup inlined
  alongside, the whole function stayed out of line and every call paid its own
  `_tlv_get_addr` for the cache base. This one is the copying minor's inner loop.
* `CopyingPointerSet::classify_arena` read both survivor-space thread-locals
  before the match that selects a kind. On Darwin there is no local-exec TLS, so
  each is a real `_tlv_get_addr` — two per classified pointer, on workloads that
  never touch a survivor. They can only answer `Survivor0`/`Survivor1`/`Unknown`,
  so the non-survivor arms are hoisted above them; no verdict changes.

#### Refuted along the way

Batching the per-slot `old_page_account_dirty_slot` map probe into one update per
4 KB page (and hoisting the per-slot weak-target check to a per-object one)
measured as **exactly zero** — `retain` 0.344 vs 0.345 s. Not shipped.

#### Validation

All 19 corpus programs byte-identical to `node --experimental-strip-types` with
exit 0. `gc-handoff/apps/iso_miss.ts` prints `checksum 437840 misses 0`, including
under `PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800` and
`PERRY_GC_VERIFY_EVACUATION=1`. Protected set unchanged within noise: `churn`
0.421, `churn_alloc` 0.374, `push_cls` 0.358, `push_num` 0.137, `churn_read`
0.022, `cycles` 0.193, `tree` 1.631, `tree_wide` 2.113, `fib40` 0.394, `interp`
1.889, `asyncpipe` 0.710, `shapes` 0.219, `pipeline` 0.543.
