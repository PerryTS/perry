### Fixed — the two pacing defaults that made #7682's fix cost 2–10× (follow-up to #7687)

#7687 landed the first of three changes: an allocation-point collection may no
longer MOVE anything, because that program point is described by neither root
lowering. Correct, and on its own **not shippable** — it took
`test_gap_gc_index_get_receiver_rooting` from **0.66 s to 6.6 s** and the #7682
interpreter from 4.78 s to 9.06 s. This is the rest.

#### The scavenge nursery cap applies only when the minor can evacuate

The cap's basis is `copying_from_space_in_use_bytes()`, which a **non-moving**
minor does not reduce — it sweeps in place and from-space stays occupied. So a
capped trigger firing a non-moving minor is due again on the very next block:
one whole-arena collection per 1 MB allocated, O(n²) in the live set. Confirmed
without a rebuild via the tuning dial — `PERRY_GC_SCAVENGE_NURSERY_MB=4096`
takes that test to **0.13 s**. Same shape as #7592, whose fix was likewise to
key a band on something a collection actually moves.

#7056's own 2x2 already said the cap and the evacuating minor "ship together,
because either alone is a bad trade". That was advisory; `policy::nursery_cap_active`
makes it load-bearing, and hands the cap back automatically once the collection
can move again.

#### Moving-loop back-edge polls are default-ON again (#7161's stopgap retired)

Both conditions #7161 named are met:

* **Its correctness reason is closed.** Its own title is "pending #7154"; #7154
  closed 2026-08-01, and that class now has a static gate
  (`gc-root-dominance.yml`) whose allowlist is empty.
* **Its codegen-quality reason is discharged by its own stated condition** —
  *"until the poll is emitted only in loops that actually ALLOCATE"*. It already
  is: `emit_gc_loop_safepoint` consults `loop_purity::loop_may_allocate`, so
  vectorizable loops stay call-free. Only the doc comment above the flag never
  got updated.

And after #7687, leaving it off was the *more* dangerous state. Nursery pressure
has exactly two precise collection points — this poll and the microtask-pump
boundary — and a compute-only program reaches neither with polls off. "Polls
off" never meant "collect later, precisely"; it meant "never collect precisely
at all", which is why every nursery collection landed where #7687 must forbid
movement.

`gc-moving-witnesses` already runs `gc_repsel_matrix.sh --arms loop_polls
--filter test_gap_gc_` on every PR, so the configuration this makes default is
the one that job has been gating over the whole 56-file corpus all along.

#### Measured, pinned quiet host, 5 repeats

| arm | wall | peak RSS | answer |
|---|---|---|---|
| before #7687 (unsound) | 4.78 s | 88.4 MB | 437839 ✗ |
| #7687 alone (main today) | 9.06 s | 58.2 MB | 437840 ✓ |
| polls off, cap gated | 5.20 s | 248.7 MB | 437840 ✓ |
| **with this change** | **4.80 s** | **88.4 MB** | **437840 ✓** |

Same speed and the same footprint as the wrong answer it replaces. 37 copying
minors, now **all** `declared_safepoint=true` where every one previously said
`false`, and zero alloc-point valve fires.

#### The two #7577 witnesses

`generator_attach_prototype`'s pair inject their collection at an allocation
point, which after #7687 neither moves nor — with polls on — happens there at
all. Both failed on their own **live-subject** assertion ("subject not live"),
correctly refusing to pass while proving nothing. They now pin
`force_shipped_default_gc_pacing()` plus a scan override.

It must be that guard and not `force_legacy_gc_pacing()`, which also turns
scavenge off — and scavenge is the disjunct that routes nursery pressure to the
direct arm at all, since `registered_root_scanners_block_budgeted_gc()` reduces
to "any COPY-ONLY scanner" under `gc_incremental_enabled()` and that registry
holds only a mutable one. With scavenge off the trigger goes to the budgeted
stepper, which is non-moving by construction, and the symptom is a third route
to the same message. The file records all three.

**`gc-ratchet` baseline: deliberately NOT regenerated here.** The pinned
artifact records the evacuation accounting of the pre-#7682 collector, and this
change moves those counters — the copying minors are the same ones, but they now
run at a declared safepoint rather than at an allocation point. Re-pinning is a
maintainer act on the pinned quiet host
(`benchmarks/gc_ratchet/run_gc_ratchet_baseline.sh --pin --notes …`), it is
explicitly out of scope for this PR, and until it happens the `gc-ratchet` job
is expected to report a breach on the evacuation family. Nothing in this change
depends on that artifact.

**Reduced at merge to the nursery-cap fix alone (part 2 deferred).** The
back-edge-poll default flip was measured to cost more than it bought and is
held for a follow-up:

| configuration | `index_get_receiver_rooting` | #7480's kernel |
|---|--:|--:|
| `main` (#7687 alone) | 6.94 s | 17 ms |
| this PR **with** the poll flip | 0.76 s | **126 ms** |
| this PR **as merged** | **0.11 s** | 17 ms (16–31 vs main's 16–31) |

The flip made its own regression test **2.4x slower** and cost **6.6x** on
#7480's kernel: a back-edge poll is a call, so `LlBlock::contains_gc_unsafe_call`
declines the element-shape fast clone (#7669) and the deref block branches
unconditionally to the slow arm. The nursery-cap fix does all of the recovery.

The precision argument for polls stands and is not withdrawn — 0 declared
safepoints with them off against 38 with them on, and after #7687 "polls off"
means "never collect precisely at all". It should land with **per-arm** poll
emission: `emit_gc_loop_safepoint` already consults
`loop_purity::loop_may_allocate`, and applying that predicate *after* the
versioned split gives the fast clone (provably call-free, therefore
non-allocating) no poll and the slow clone one. Teaching the admission to ignore
polls instead would be unsound — `element_shape_loop.rs` defines call-freeness as
"no allocation that can move the array runs while the clone does", and the fast
clone holds a preheader-derived base (#7660's shape).

`test_moving_loop_minor_on_by_default_7682` is removed with the flip it pinned.
