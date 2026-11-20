Fixed live objects lost by the incremental (budgeted) collector's sweep
(#11842). A package manager compiled with Perry wrote wrong files, lost
resolved packages or crashed on 10-40% of cold installs with the incremental
collector on, and never with `PERRY_GC_INCREMENTAL=0`. Objects the program
allocates while a budgeted sweep is still running carry no mark, and two steps
of the sweep could still reach or lose them:

- The old-generation hole list from the previous sweep stayed usable during
  the sweep. A same-size old allocation (a large buffer, say) could land in a
  hole inside a block the sweep had not walked yet; the walk then found that
  live, unmarked object and freed it, and the next same-size allocation took
  the same bytes. An old-reclaiming sweep now stops hole reuse when it starts
  (`old_free_forget_until_rebuild`); its own rebuild lists the holes again when
  the walk ends (`gc/old_free.rs`, `gc/oldgen/sweep_objects.rs`).
- The block cleanup's last step moves Eden's current block back to the first
  empty one and repoints codegen's inline allocator there. It did so without
  first storing the inline allocator's offset into the block it was filling,
  so objects placed there since the sweep began sat past the block's recorded
  fill, invisible to walkers, and the block's next fill overwrote them. The
  step now stores the offset first (`ArenaResetEmptyBlocksState::finish` in
  `arena/reset.rs`). This one also affected budgeted minor cycles.

New instruments (instrument builds only, `PERRY_GC_INSTRUMENTS=1` at compile
time):

- `PERRY_GC_PROTECT_OLD_SWEEP=1` (or `poison`), the non-moving sweeps'
  counterpart of `PERRY_GC_PROTECT_FROMSPACE`: what a sweep frees is recorded,
  poisoned with a NaN-boxed pointer to unmapped memory and (old/malloc, at
  `=1`) page-protected, and never reused: no hole is listed, no swept block is
  reset or released, no malloc object is handed back. A fault report names the
  freed object the fault address or a register points into and which sweep
  freed it. With it on, the unfixed package manager passed 48/48; bisecting which
  reuse it blocks found both bugs.
- `PERRY_GC_BUDGETED_OLD_RECLAIM=1`: old-gen reclaim never runs as a
  synchronous full at an allocation point or precise safepoint, only as a
  budgeted full started at a runtime safepoint, so an async program's major
  collections all run incrementally.

Tests: `gc::tests::sweep_window_births` (both bugs and the instrument; each
bug test fails without its fix) and
`test-files/test_gap_gc_11842_sweep_window_hole_reuse.ts` (6 corrupted buffers
before the fix, 0 after).
