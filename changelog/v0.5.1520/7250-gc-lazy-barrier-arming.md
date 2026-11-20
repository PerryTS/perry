**perf(gc): the write barrier stops maintaining a remembered set nothing has read yet (#7187 Phase A).**
`classify_heap_generation` is the single largest symbol in `batch.ts` — 19.03% of
the program, 657M instructions, 2.3× the next symbol — and #7170 measured it
**with zero collections running**. It is reached from the write barrier, on the
sort's own element stores, maintaining a remembered set whose only consumer is a
collection that never happens. The remembered-set half of the barrier now starts
**unarmed** and returns immediately, before the parent decode; it arms on the
first *read*.

Measured on `batch.ts` (macOS arm64, release, existing `PERRY_GC_TRACE` counters,
base `8b024958f`): the program makes 2,265,777 barrier calls; 280,536 exit at the
#6011 non-pointer-child fast path, which is untouched; the remaining **1,985,241
(87.6%)** reached the remembered-set half and now do not. That deletes
**5,532,729 `classify_heap_generation` calls** (211,497 × 1 on the
parent-not-old exit, 1,773,744 × 3 — parent, child, slot — on the slow path) and
**1,773,744 `mark_dirty_old_page` calls**, of which only 517 (0.029%) ever
produced a new dirty page. Collections on `batch.ts`: zero, so all of it was
skippable.

**The invariant, and why the obvious version of it is wrong.** "Nothing has
collected, so there is no old generation and no edge to record" is false, and
believing it reproduces a bug this repo has already had: `arena_alloc_gc` routes
any allocation over `LARGE_OBJECT_THRESHOLD_BYTES` straight to
`arena_alloc_gc_old`, so old-gen exists from the first large allocation.
`batch.ts` is exactly that case — its 40 000-element arrays are born old while
every record they hold is nursery, making every element move a genuine old→young
edge. `note_array_slot_layout_only`'s comment in `array/header.rs` is the
postmortem of the last time this was got wrong ("155 edges, all born-old
array→young object; the store never hit any barrier"). So the contract is stated
on the *reading* side: **while unarmed the barrier records nothing, and in
exchange the first read of the remembered set on a thread does not trust the log
at all — it reconstructs the complete old→young edge set from the heap and arms
the barrier on the way past.** That is enforceable because
`remembered_dirty_snapshot()` is the sole read path — the budgeted/full cycle's
`RememberedSetRootMarkState`, the copying nursery fast path and its preflight,
the cycle's pre-clear coverage snapshot and the evacuation verifier all obtain
the dirty set there, and the only other touches of `DIRTY_OLD_PAGES` /
`EXTERNAL_DIRTY_SLOT_PAGES` / `REMEMBERED_SET` are the barrier's own writes, the
clear, and diagnostics (the arena's per-page `dirty` bit feeds `old_page_summary`
telemetry only). Arming inside that one function means no collector can observe
an unarmed, and therefore empty, log, and the #5029 verifier's predicate is
unchanged. Two orderings are load-bearing: **arm first, walk second** (a store
landing mid-walk must be logged, because the walk may already have passed its
parent), and the check sits **after** the child prologue (so the #6011 numeric
fast path pays nothing new and SATB shading is never skipped) and **before** the
parent decode (so the unarmed window also skips `decode_heap_addr`'s
raw-pointer arm, itself a `classify_heap_generation`). The flag is a global
`AtomicBool` rather than a thread-local — arming is monotone and conservative in
the early direction, while `thread_local!` costs a `_tlv_get_addr` call on macOS
on a per-heap-store path — while "has *this* thread reconstructed" stays
thread-local, so a worker armed early by another thread's collection still
reconstructs on its own first read.

**The gate asserts its subject was live** (`gc/tests/barrier_arming.rs`), over a
born-old parent holding a nursery child: (1) the unarmed barrier logged nothing
for a real old→young edge — that skip *is* the lever; (2) the first read
reconstructed exactly once, recovered the exact page the barrier skipped, and the
collector marked the child through it; (3) a **sabotage arm** proves that with
the reconstruct suppressed the same edge is uncovered and the child ends up
unmarked. Both mutations were verified to turn the suite red. No new env knob.
Census under the existing `PERRY_GC_TRACE` surface:
`write_barrier.unarmed_skips`, `armed`, `reconstructs`,
`reconstruct_recovered_{old,external}_pages`.

**End-to-end A/B on the built binaries** (same target dir, artifacts snapshotted
per arm, runtime archive md5s confirmed different). The branch's own census on
`batch.ts` reports `unarmed_skips = 1,985,241` — the base's
`calls − non_pointer_child_skips` to the unit — with `old_to_young_slow_hits`,
`parent_not_old_skips` and `dirty_page_mark_attempts` all at **0**, and
`reconstructs = 1` recovering 393 old pages (fewer than the 517 the barrier had
logged: the reconstruct records only edges live at collection time, so it is
strictly more precise). A probe that collects *before* the sort and again at the
end reports the armed steady state **counter-for-counter identical** across arms
(2,265,777 / 280,536 / 211,497 / 1,773,744 / 1,773,744 / 517), confirming the
change is a pure pre-collection skip. Compiled-binary size **−32,936 bytes
(−0.25%)**, so no size trade to state. Max RSS unchanged (24.3 MB both arms) —
the retained remembered set is ~517 page numbers, invisible at this scale, so
the RSS argument for lazy arming does not show up on this workload. 29 GC/repsel
gap tests byte-identical across arms; `cargo test -p perry-runtime --lib`
1639 passed / 0 failed single-threaded.

Also recorded, because the campaign brief expected otherwise: the
`Object.defineProperty(require, 'name', …)` preamble in
`compile/cjs_wrap/wrap.rs` contributes **0%** of this cost. The "barrier" it arms
is the compile-time `Ptr<Shape>` promotion barrier (`ptr_shape_report.rs`'s
`MODULE_BARRIER` / rule 5, which `collectors/cjs_scaffolding.rs` exempts), not
the GC write barrier; `wrap.rs` contains no write-barrier references, and
`batch.ts` is never CJS-wrapped. Rank 1 is 100% intrinsic runtime barrier.

Phase A only: it removes the pre-collection cost, which on a program that never
collects — most of what Perry compiles — is the whole barrier. Capping the armed
steady state is #7187's B3/B4, unstarted; the 12.91% layout side-table (#5094,
#6759) is untouched by design, since `layout_note_slot` sits outside the barrier
at every call site.
