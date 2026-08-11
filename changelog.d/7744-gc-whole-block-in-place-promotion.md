### perf(gc): whole-block in-place promotion of a fully-live young generation (#7742)

When a copying minor's nursery is (near-)entirely live, the collector now
relabels its blocks as old-gen instead of evacuating them object by object.
`retain.ts` 0.81 s → **0.53 s**, `retain_wide.ts` 1.33 s → **1.07 s**,
`retain1` 0.38 → 0.29, `retain_wide1` 0.38 → 0.27, `deeplist` 0.30 → 0.24
(peak RSS 117 MB → 97 MB). Promotion cost per object **243 ns → 105 ns** on `retain`
and **264 ns → 116 ns** on `retain_wide`, derived from the trace's own
promoted-object count and pause summed over the promoting cycles, not estimated.
All measurements are best-of-5 wall clock on the pinned quiet M1 mini, outputs
byte-verified against `node --experimental-strip-types` before timing.

#### Why the old path was pure overhead here

`retain.ts` builds a 3M-element array of records and drops none of them. Its GC
trace measures a young-survival ratio of **1.000** on every copying minor, and
its four promoting minors moved 2,097,155 objects in 509.7 ms of pause. Every
one of those moves paid for a fresh `arena_alloc_gc_old`, a `memcpy`, a
`layout_transfer`, `old_page_account_promoted_object`, the `GcMoveHookKind`
hooks, a forwarding stub, and the rewrite of every slot that referred to it — to
put an object somewhere it had no reason to be.

Whole-block promotion (V8 calls it page promotion) pays none of that. The block
changes generation; the bytes do not move, so nothing in the heap and nothing in
any address-keyed runtime side table needs rewriting.

#### The mechanism (`crates/perry-runtime/src/arena/promote.rs`)

Two halves around the existing trace:

* **Before the trace**, `retag_young_for_in_place_promotion` flips every in-use
  Eden and survivor block's page range to generation `Old`, space
  `HeapSpace::PromotedYoung`. From that instant the barrier predicates
  (`barrier_parent_needs_remembering`, `remembered_child_needs_tracking`) read
  old-gen semantics, which is what makes the trace record the right
  remembered-set edges; the distinct space is what still tells the copier "this
  was young at cycle start, so it owes one field scan". The blocks stay in their
  own arenas for the cycle, so no old-gen allocation can land in them mid-way.
* **Where the from-space reset would have run**, `finish_in_place_promotion`
  walks each block linearly (one `size` hop per object, no hashing), stamps
  `GC_FLAG_TENURED` on every header — `Old ⟹ TENURED` is what the generated
  write barrier's fast path is gated on (#7511) — registers the live objects
  into the old-gen page index in per-page bulk runs, hands the block to
  `OLD_ARENA` leaving the usual tombstone behind, and retags the range to plain
  `HeapSpace::Old`.

Unmarked objects on a promoted block are dead and are neither swept nor indexed;
they stay as old-gen garbage until the next full mark-sweep finds them through
the ordinary linear old-arena walk. That retention is the technique's entire
footprint cost.

#### The policy is a measurement, not a guess (`gc/promote_in_place.rs`)

Block liveness is not knowable before the trace and Eden blocks are recycled at
offset 0, so per-block history means nothing. The decision is per cycle, from
the previous cycle's measured young-survival ratio. Measured across the GC
benchmark set the population is bimodal by three orders of magnitude:

| workload | copying minors | young-survival ratio |
|---|--:|--:|
| `retain`, `retain1`, `retain_wide` | 5–7 | 0.999 – 1.000 |
| `deeplist` | 3 | 1.000 |
| `churn`, `churn_alloc`, `push_cls` | 105 | 0.000 – 0.004 |
| `push_num`, `cycles` | 16–18 | 0.000 |
| `tree`, `tree_wide`, `churn_read` | 0 | — no copying minor runs |

so the 95% threshold is justified by footprint, not by classification: a
mispredicted cycle retains at most 5% of the young generation. What makes the
per-cycle decision sound rather than optimistic is that **a promoting cycle
still traces, so it measures too** — a workload that flips from live to garbage
pays one nursery of retained garbage and then the policy turns itself off. A
running 32 MB cap on promoted dead bytes bounds the steady state the per-cycle
correction does not cover, and resets when a full collection reclaims them.
Measured `in_place_dead_bytes` and `in_place_sparse_blocks` (blocks under 50%
live) are **zero** on every benchmark in the set.

#### Three passes that a promoting cycle can prove are empty

A remembered-set entry is only ever created for a nursery-generation child or a
malloc-registry child. After the retag the first population is empty by
construction — every in-use young block was taken — and when the malloc registry
was empty at cycle start so is the second. On such a cycle
`visit_slot_with_parent`'s re-decode-and-remember,
`rebuild_evacuated_old_to_young_remembered_set`, and
`restore_surviving_dirty_coverage` can only insert nothing, and are skipped;
`debug_assert_no_remembering_possible` re-derives the premise from the heap in
debug builds. This is where most of the win is: it took `retain`'s promoting
minors from 42.8/86.5/107.8/156.2 ms to 23.4/46.7/57.6/84.0 ms.

#### Pacing: the part that made the first cut a 1.52 s regression

A copying minor recycles Eden's blocks, so Eden's free capacity survives the
collection and the arena-bytes trigger's runway is "that capacity + the step".
Promotion hands the blocks away. The first working version therefore collapsed
Eden's high-water from 57 MB to 18 MB on `retain`, doubled the collection count,
and bought a second 690 ms full collection — 0.81 s → **1.52 s** from a change
that had made every individual promotion cheaper. The capacity is now returned
to the next trigger as one-shot **headroom** rather than as re-reserved blocks;
re-reserving also fixes the pacing but maps memory the program may never reach
(measured: `retain_wide` peak RSS 470 MB against baseline's 447 MB).

#### Also here

`CopyingPointerSet::classify_arena` resolved the page-generation range twice per
visited slot — once for the user pointer, once for the header 8 bytes below it.
It now resolves once and answers the second from the range base, which is also
the guard that keeps a garbage candidate at the very start of a range from
becoming a read of the unmapped page below. `classify_heap_space` self-samples
on a retain profile: 201 → 108.

#### Escape hatch and instruments

`PERRY_GC_PROMOTE_IN_PLACE=0|off|false` reverts to object-by-object evacuation;
both states of the parse are asserted (`promote_in_place_knob_parses_both_states`),
per the GC knob kill-policy. `PERRY_GC_FORCE_EVACUATE` and `PERRY_GC_ZEAL`
disable the path outright — they exist to make objects MOVE, and a promoting
cycle moves nothing, so leaving it on would quietly stop exercising their
subject. The trace gains `in_place_promotion`, `in_place_promoted_objects`,
`in_place_promoted_blocks`, `in_place_dead_bytes`, `in_place_sparse_blocks`,
`young_survival_permille` and `remembering_skipped`, and the end-to-end test
gates on the promoted-object count rather than on "nothing threw".

#### Validation

`gc-handoff/apps/iso_miss.ts` prints `checksum 437840 misses 0` plain, under
`PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800`, and under
`PERRY_GC_VERIFY_EVACUATION=1`. It is an interpreter, so it takes the ordinary
copying path on all 50 of its minors and retires 50 quarantine sets — i.e. the
canary's instrument coverage is exactly what it was, and it does **not** cover
the new path. What does cover it: `PERRY_GC_FROMSPACE_SCAN_ABORT=1 ./n_retain`
(4 promoting cycles, up to 2,111,418 objects / 21,085,616 words scanned per
cycle) reports `missing_rewrites=0 dangling=0 never_dirty=0 lost_dirty=0
dirty_but_missed=0` on every cycle — those last three are precisely the
"remembered set is missing an edge" counters that the skipped passes could have
broken. `retain`, `retain_wide` and `deeplist` also produce correct output under
`PERRY_GC_VERIFY_EVACUATION=1` and under the from-space quarantine.

`cargo test -p perry-runtime --release`: 1982 passed, 0 failed.

`gc_ratchet` (7 repeats, `shared_ci`): **every gated semantic cell —
correctness, heap bytes, cycle counts, copied/promoted/freed — is identical to
`origin/main` measured on the same host, including `12_large_live_set`.** The
six cells the profile reports as REGRESSION are reported identically by
`origin/main` itself, so they are pre-existing drift between the pinned artifact
and current `main`. The flip side of that identity: none of the 13 ratchet
probes runs a promoting cycle, so the ratchet does not yet cover this path.

No protected GC benchmark regressed: `churn` 0.41, `churn_alloc` 0.37,
`push_cls` 0.35, `push_num` 0.14, `churn_read` 0.02, `cycles` 0.19,
`deeplist` 0.24, `tree` 1.63, `tree_wide` 2.10 s. Peak RSS `retain`
326 → 316 MB, `retain_wide` 447 → 446 MB.

`retain` remains 4.4× Node (0.53 vs 0.12) and `retain_wide` 7.1× (1.07 vs 0.15).
The residue is no longer promotion bookkeeping: on a `retain_live_big` profile
the remaining GC time is the remembered-set scan over the array's dirty element
slots (26%) and one full collection that reclaims nothing (28%). #7742 stays
open for those.
