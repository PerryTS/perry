**fix(async): a completed plain-async activation's box cells are released and reused — the ~500 B/request malloc-side accumulation is gone** (#7933 follow-up)

Every plain-async activation boxes its body locals and state-machine control cells into 8-byte `std::alloc` cells registered in `BOX_REGISTRY`. #7933 (PR #7939) stopped a completed activation from *retaining* its locals by clearing the releasable cells, but kept every cell malloc-resident and registered forever (registry monotonicity backed the perry#4898 pointer rejection and the #7906 positive cache), and never touched the control cells. Net effect: cell + registry bytes per completed activation, linear for the life of the process, and invisible to every GC counter because none of it is in the GC heap — the arena reported ~250 KB in use after every collection while `vmmap` showed 104.5 MB dirty in the mimalloc tag.

The release is now real, with pooled reuse:

- New HIR `Stmt::ReleaseBoxes(ids)` (a reclamation hint — safe to drop, but must be remapped like a `LocalSet` target by id-substitution passes) replaces the per-cell `LocalSet(id, undefined)` release, and the terminal release set now covers the whole activation frame including `__gen_state`/`__gen_done`/`__gen_executing` and the pending-completion record.
- Codegen lowers it to `js_box_release` / `js_i32_box_release` / `js_bool_box_release`, mirroring `emit_preallocate_boxes`' kind selection, through local slots or closure-capture slots.
- The runtime release clears the cell, de-registers it, evicts the positive-cache slot, and parks the address in a per-kind quarantine that drains into a free pool at the outermost microtask-pump exit once the task queue is empty; `js_*box_alloc*` reuses pooled cells before calling `std::alloc`. Cell memory is never handed back to the allocator, so "was a box" can never become "is another object" — perry#4898 and #7906 survive unchanged. A stray duplicate resume of a terminal activation observes parked terminal values (`__gen_done`=true, `__gen_state`=-1) and takes byte-for-byte the pre-release short-circuit path; by the time the pool is eligible for reuse the task queue has drained, so no such resume can exist.

**Measured on `07c8040bf`** (i.e. after #8204's header shrink + GC pacing change and #8196's side-table prunes, neither of which moved this residue). `asyncpipe` at BATCHES=120/600/1200, `PERRY_GC_DIAG=1` `[box-stats]`:

| BATCHES | allocs | releases | resident cells | peak RSS before | peak RSS after |
|--:|--:|--:|--:|--:|--:|
| 120 | 192,868 | 192,868 | **65,915** | 40.9 MB | 41.5 MB |
| 600 | 964,228 | 964,228 | **65,915** | 83.2 MB | 58.6 MB |
| 1200 | 1,928,428 | 1,928,428 | **65,915** | 148.9 MB | 79.7 MB |

Allocations grow exactly linearly (1,607 cells/batch) and `releases == allocs` at every size, while the malloc-side residue `allocs - pool_reuses` is a **constant 65,915** across a 10× range — one inter-`tick()` cascade's working set, slope zero. Both registries are empty at exit. Baseline peak RSS grows ~100 KB/batch; `vmmap` Memory Tag 240 (mimalloc) resident falls 141.1 MB → 71.6 MB at BATCHES=1200 (dirty 104.5 → 57.2 MB), which is the whole of the RSS delta. Program stdout is byte-identical at all three sizes.

Corpus, best-of-5, instructions and peak RSS together (same-binary run-to-run spread on this host is ±0.42% instructions / ±0.15% RSS, so every positive below is at or under the noise floor):

| row | instructions | peak RSS |
|---|--:|--:|
| **asyncpipe_big** | **−10.38%** | **−42.06%** (141.4 → 81.9 MB) |
| pipeline | −4.07% | ±0.00% |
| asyncpipe | −0.96% | −2.41% |
| iso_STRWORK / streq | −0.15% | −0.35% / +0.13% |
| interp | −0.07% | +0.15% |
| churn_alloc | −0.07% | −0.33% |
| fib35 | −0.03% | −0.90% |
| shapes | +0.11% | −0.29% |
| iso_SUMLOOP | +0.27% | −0.48% |

The instruction win is not incidental: not growing `BOX_REGISTRY` without bound removes the rehash traffic its own doc comment already blamed for ~3% CPU on promise-heavy workloads. All 10 rows produce byte-identical stdout across the two arms.

The GC ratchet is unmoved: an A/B of both arms over all 14 probes compares 126 gated (probe, metric) cells — retention, evacuation and promotion counters — with **0 differences** and correctness `pass` on every probe in both arms (non-vacuous: `copied_objects` and `promoted_objects` are non-zero on several rows).

Release and reuse run *while the collector is active*, not merely in GC-quiet code: `asyncpipe` at BATCHES=1200 reports `minors=15`, `copied_objects=600`, `promoted_objects=14`, `loop_polls=16` alongside its 1,928,428 releases and 1,862,513 pool reuses, with stdout unchanged.

**Known limitation, measured.** Reuse is gated on reaching the outermost microtask-pump exit with an empty task queue — the boundary that makes a stray duplicate resume impossible. A program that never returns to an empty task queue until it ends (one continuous `await` cascade with no timer or I/O between stages) therefore performs its releases but never harvests them: `pool_reuses=0`, and the malloc residue is what it was before. On a synthetic fixture of exactly that shape (400 iterations over 7 async exit-path shapes, no timers) the release work is unrecovered overhead worth **+1.32% instructions and +0.3 MB peak RSS** (run-to-run spreads 0.79% / 0.07%, so both are real if small). Real event-driven workloads drain between events — which is why `asyncpipe`, whose `tick()` is a `setTimeout`, harvests 1.86 M of 1.93 M cells — and none of the ten corpus rows shows the degenerate shape. The boundary is deliberately not loosened here: every weaker rule permits a cell to be reused while an activation could still resume, which is the one way this change could corrupt state rather than merely fail to help.

**Exit-path coverage.** A fixture exercising normal return, `throw` after an `await`, early `return` from inside a loop after a suspend, `await` on a rejected promise, `try`/`finally` across a suspend on both terminal arms, loop-created closures capturing a per-iteration binding across a suspend, and async-generator `.return()` / full drain — 400 iterations each — produces output byte-identical to the Node oracle on **both** arms. The release is live on it (`releases=20027` of `allocs=25628`), and the 5,601 cells it does *not* release are exactly the ones still registered at exit, so every cell is accounted for as either released or still reachable.

`PERRY_GC_DIAG=1` now prints a `[box-stats]` line (allocs / pool_reuses / releases / resident_cells / registry sizes) at exit. Regression gates assert the counters, because the leak is behaviourally invisible and a test that merely runs to completion cannot catch it: `perry-runtime` `release_tests` (residue bounded not linear, parked-cell inertness, release idempotence, flush-gated reuse, foreign-pointer no-op) and `perry-codegen` `release_boxes_lowering` (an emitted release must reach the IR; kind selection; capture path).

**Hardening found by auditing the fan-out.** The new variant needed 94 exhaustive-match arms, but six *further* sites take `ReleaseBoxes` through a pre-existing `_ => {}` catch-all, so `rustc` flagged nothing. Three of them renumber LocalIds, which the variant's own doc declares incorrect — an unremapped `PreallocateBoxes` merely allocates a cell nobody reads, but an unremapped `ReleaseBoxes` frees a **still-live** local's cell and hands it to the next allocation. None is reachable today (inlining runs before the async transform, the cross-module harvest refuses bodies containing a release, and the max-id scans feed a `next_local_id` computed earlier), but that rests entirely on pipeline ordering nothing enforces. Now remapped in `inline/substitute.rs`, `perry-hir/src/analysis.rs`' two canonical remappers, and `generator/per_iteration.rs`; the `generator/id_scan.rs` and `deforest/walk.rs` max-id scans include the release ids; and `perry-codegen/src/boxed_vars.rs` states explicitly why it must *not* collect them. Those new arms are scoped strictly to `ReleaseBoxes` — the same gap exists for the `Prealloc*` variants at three of those sites, but it is pre-existing, benign in its failure direction, and closing it would change codegen for existing programs, so it is documented and left alone.

`scripts/gc_root_dominance_check.py`:

- The "box" immovable-source exemption rested on *"boxes are never freed"*, which this change falsifies, while its probe only grepped for `dealloc(`/`arena_alloc(` — all of which a *recycle* path passes. The exemption would have stayed green on a dead premise, which the script's own docstring calls strictly worse than no exemption. Re-argued on the property the design actually preserves (cell memory is never returned to the allocator, so an address never stops naming box-cell memory and can never become another kind of object), and the probe now additionally requires the reuse path to stay quarantine-gated. Sabotage-tested: bypassing the quarantine, and introducing a real `dealloc`, each turn it red.
- The three `js_*box_release` names were added to `gc_call_effects.rs` only, breaking the one-way containment four comments there assert — the same one-sided drift that cost #7510 358 spurious violations. They are now in `NONCOLLECTING` too, and `box_and_closure_helpers_stay_contained_in_the_checker_authority` machine-checks the relation for that family instead of trusting prose. (A whole-table subset is deliberately *not* asserted: 28 pre-existing entries diverge, and since the checker treats an unknown callee as collecting, closing that gap would make it *less* conservative — a separate decision needing per-name evidence.)

Also refreshes every monotonicity claim the release invalidated, including the load-bearing correctness argument in `expr/literals_vars.rs` that let a `box_ptr` outlive a collecting call on the strength of "never freed".
