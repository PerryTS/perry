**fix(async): a completed plain-async activation's box cells are released and reused — the ~500 B/request malloc-side accumulation is gone** (#7933 follow-up)

Every plain-async activation boxes its body locals and state-machine control cells into 8-byte `std::alloc` cells registered in `BOX_REGISTRY`. #7933 (PR #7939) stopped a completed activation from *retaining* its locals by clearing the releasable cells, but kept every cell malloc-resident and registered forever (registry monotonicity backed the perry#4898 pointer rejection and the #7906 positive cache), and never touched the control cells. Net effect: cell + registry bytes per completed activation, linear for the life of the process, and invisible to every GC counter because none of it is in the GC heap — the arena reported ~250 KB in use after every collection while `vmmap` showed 104.5 MB dirty in the mimalloc tag.

The release is now real, with pooled reuse:

- New HIR `Stmt::ReleaseBoxes(ids)` (a reclamation hint — safe to drop, but must be remapped like a `LocalSet` target by id-substitution passes) replaces the per-cell `LocalSet(id, undefined)` release, and the terminal release set now covers the whole activation frame including `__gen_state`/`__gen_done`/`__gen_executing` and the pending-completion record.
- Codegen lowers it to `js_box_release` / `js_i32_box_release` / `js_bool_box_release`, mirroring `emit_preallocate_boxes`' kind selection, through local slots or closure-capture slots.
- The runtime release clears the cell, de-registers it, evicts the positive-cache slot, and parks the address in a per-kind quarantine that drains into a free pool at the outermost microtask-pump exit once the task queue is empty; `js_*box_alloc*` reuses pooled cells before calling `std::alloc`. Cell memory is never handed back to the allocator, so "was a box" can never become "is another object" — perry#4898 and #7906 survive unchanged. A stray duplicate resume of a terminal activation observes parked terminal values (`__gen_done`=true, `__gen_state`=-1) and takes byte-for-byte the pre-release short-circuit path; by the time the pool is eligible for reuse the task queue has drained, so no such resume can exist.

**Measured on `b8d32ab19`** with both arms rebuilt at that commit (the earlier run on `07c8040bf` reproduced within 0.3 MB on every row, so #8204's header shrink and GC pacing change, #8196's side-table prunes, and #8211/#8212/#8162 none of them move this residue). `asyncpipe` at BATCHES=120/600/1200, `PERRY_GC_DIAG=1` `[box-stats]`:

| BATCHES | allocs | releases | resident cells | peak RSS before | peak RSS after |
|--:|--:|--:|--:|--:|--:|
| 120 | 192,868 | 192,868 | **65,915** | 41.0 MB | 41.8 MB |
| 600 | 964,228 | 964,228 | **65,915** | 83.3 MB | 58.8 MB |
| 1200 | 1,928,428 | 1,928,428 | **65,915** | 149.0 MB | **79.9 MB** |

Allocations grow exactly linearly (1,607 cells/batch) and `releases == allocs` at every size, while the malloc-side residue `allocs - pool_reuses` is a **constant 65,915** across a 10× range — one inter-`tick()` cascade's working set, slope zero. Both registries are empty at exit. Baseline peak RSS grows ~100 KB/batch; `vmmap` Memory Tag 240 (mimalloc) resident falls 141.1 MB → 71.6 MB at BATCHES=1200 (dirty 104.5 → 57.2 MB), which is the whole of the RSS delta. Program stdout is byte-identical at all three sizes.

### Peak RSS across workload size, and the floor that is left

`asyncpipe`, matched arms at `b8d32ab19`, peak RSS best-of-5, stdout byte-identical at every point:

| BATCHES | 30 | 60 | 90 | 120 | 300 | 600 | 1200 |
|---|--:|--:|--:|--:|--:|--:|--:|
| base MB | 21.89 | 28.53 | 36.11 | 40.97 | 57.41 | 83.27 | 149.03 |
| fix MB | 22.69 | 29.45 | 35.92 | 40.78 | 49.23 | 58.20 | 79.30 |
| **delta** | **+0.80** | **+0.92** | −0.19 | −0.19 | −8.17 | −25.06 | **−69.73** |

**This does not yet meet "strictly not worse on every row": below ~75 batches the change still costs ~0.9 MB.** Threading the free list through the cells moved the crossover from ~200 batches to between 60 and 90 and improved the 1200 row from −63.8 to −69.7 MB, but it did not remove the floor.

The residue is **not** the reuse pool, which now costs zero bytes, and it is not a pool-sizing policy question. Measured mechanism:

- The quarantine is published only at an outermost microtask-pump exit with an empty task queue, and **that boundary is reached 6 times in a 30-batch run and 46 times in a 1200-batch run** — a tight `await` cascade resolves through the `AsyncStep` fast path without pumping. Every release in between is held as an address in the quarantine `Vec`.
- At BATCHES=30 that is 48,238 addresses (all of them — `pool_reuses` is literally **0**, the release does its work and harvests nothing), which at 8 B plus `Vec` doubling is ~0.7 MB, plus the ~80 KB page-touch cost. That is the +0.80 MB, fully accounted for.
- Relaxing the boundary is not the lever it looks like: dropping the `MICROTASK_RUN_DEPTH == 1` clause was measured and moved flush count only 6 → 46, because `run_microtasks` itself is what runs that few times. `flush_attempts == flushes` at every size, so the empty-queue test never blocks a flush.

**Capping or decaying the free list cannot fix this, and would cost most of the win.** Reuse is bounded by `flushes x cap`, so against the measured 46 intervals at BATCHES=1200:

| cap | 1,024 | 4,096 | 8,192 | 16,384 | 32,768 | 65,536 |
|---|--:|--:|--:|--:|--:|--:|
| max reuse | 2.4% | 9.8% | 19.5% | 39.1% | 78.2% | 96.6% |

The natural high-water mark, 65,915, *is* one flush interval's working set — the knee is already where the pool sits. And a cap would not return memory anyway: capped-out cells are still minted and, by design, never handed back to the allocator, so capping converts pooled cells into freshly-minted ones. Decay and page-return are blocked by the same invariant (cell memory is never returned, which is what perry#4898 and #7906 rest on).

Removing the floor therefore means publishing released cells somewhere more frequent than the microtask-pump exit. That was investigated rather than assumed, and **no safe earlier publish point exists within this change's scope**:

- *Per-kind split — refuted.* The tempting argument is that only the i32/i1 CONTROL cells need their parked terminal values, because generated code reads them raw, while body-local `Box` cells go through the registry-checked `js_box_get_bits` and so read `undefined` once de-registered no matter what their bytes hold — which would let body locals publish immediately at release. It does not hold. A stray resume **writes `__gen_sent` before it reads `__gen_done`** (`generator/lower/async_step.rs`: *"it writes `__gen_sent` before reading it, then short-circuits on the un-released `__gen_done`"*). `__gen_sent` is a plain `Box` cell. While it is merely de-registered that write is dropped, which is exactly why the current design is safe; but a republished cell is **re-registered to a different activation**, so the write would land on that activation's live local. Body-local cells are therefore no safer to publish early than the control cells.
- *Dropping `MICROTASK_RUN_DEPTH == 1` — measured, rejected.* It moved flush count only 6 → 46, because `run_microtasks` itself runs that few times, and it spends a documented safety clause to buy almost nothing.

What would work is **per-activation reachability**: refcount the queued `Task::AsyncStep`s per activation (increment on enqueue, decrement on dispatch) and publish an activation's cells when its count reaches zero. That is safe by construction rather than by a global queue predicate, and it would publish almost immediately after a terminal state — collapsing both the quarantine and the floor. It also puts an increment and a decrement on the async enqueue/dispatch hot path and changes the pump's contract, so it belongs in its own change with its own test pass, not bolted onto this one.

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
