### GC: a promoted block's page-object list is described, not stored

Whole-block in-place promotion (#7742/#7888) walked every promoted object to
stamp `GC_FLAG_TENURED`, and while there pushed each object's header address
into `OLD_GEN_PAGE_OBJECTS[page]` — 8 bytes per object, held for the life of the
process, so a later reader could answer "which objects live on this page".

A promoted block is contiguous and parses linearly by `GcHeader::size`, which is
the parse `old_arena_walk_objects` (the old-gen sweep's own enumerator) already
performs over every old block. The list was a derivable fact being stored. It is
now recorded as one `PromotedPageRun { first_header, last_header, count }` per
page and expanded only if a reader actually asks for that page —
`gc-handoff/bench/retain.ts` asks for none of them.

Peak RSS: `retain` 311.1 → 293.5 MB (−5.7 %), `retain_wide` 420.8 → 400.8
(−4.7 %), `retain1` 108.8 → 99.4 (−8.6 %), `deeplist` 92.7 → 83.8 (−9.6 %).
`in_place_promotion` phase: `retain_wide` 41.3 → 34.6 ms, `retain` 18.2 → 17.6.
On the 11 corpus programs that never promote in place the phase is 0.0 ms in
both arms and RSS moves ≤ 0.2 MB.

**What the measurement that motivated this also settled**, since both recorded
figures for `retain`'s GC share (62 % and 93 %) predate #7888 and are wrong:

* `retain` is **33 %** GC pause, `retain1` 54 %, `retain_wide` 37 %,
  `retain_wide1` 44 % — and **`shapes` is 7 %** (one minor, 4.6 ms, survival
  30‰; the 16 400-byte born-tenured cliff is gone, so it is a dispatch/string
  benchmark now, not a GC one).
* A three-arm probe over the promotion walk found the `GC_FLAG_TENURED` stamp is
  **free** (deleting it measures *slower* than keeping it on three of four
  benchmarks), the page-index build is 50–67 % of the walk, and the residual
  ~4.5 ns/object — finding each header in order to stamp it — is structural:
  moving it means the generated write barrier testing a page instead of a header
  bit, and with no contiguous heap reservation that is a hash probe on every
  pointer store.

Correctness is held structurally rather than by memory, because a reader that
skipped the expansion would see an empty page and the dirty scan it feeds would
lose every old→young edge out of those objects.
`every_page_object_reader_expands_promoted_runs` enumerates every function in
`arena/page_meta.rs` touching `OLD_GEN_PAGE_OBJECTS` and requires it to expand
first or carry a written exemption, with stale exemptions failing too (the shape
of #7624's `deferred_registration_flush_sites`); both halves of that gate fired
during development. A described run is valid only while the block's object
boundaries cannot move, so `GcCycleState::new_full` — the one constructor whose
sweep frees old-gen objects in place — expands every pending run before it
starts, `unregister_old_block_pages` discards rather than expands, and only
`PromotionLiveness::AssumeAllLive` pages may be described at all (a traced
promoting cycle's liveness lives in marks that `clear_marks` destroys, so that
path keeps the eager list unchanged).
