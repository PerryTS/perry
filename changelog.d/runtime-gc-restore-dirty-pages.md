### runtime: the post-minor coverage restore walks an old parent's in-body slots only on dirty pages

- After a minor clears the remembered set, `restore_surviving_dirty_coverage` re-derives it from the pre-clear
  dirty snapshot. For an old parent the dirty scan could not report complete it visited every slot, clean pages
  included. A Map store, Set store or array that spans several pages is such a parent on every minor, so each
  minor walked the whole container. On a chess search (a 36 000-entry `Map` transposition table, 296 minors) that
  was 7.8 M slot visits, 2.5% of the program's instructions.
- An old parent's in-body slot is described by its page, so the walk now visits those slots only on the pages the
  snapshot names (`gc/restore_walk.rs`). Slots outside the parent's body and malloc parents are owner qualified
  and keep the whole-owner walk. No remembered-set entry changes: a slot on a clean page that held a young child
  was already a lost edge before the restore ran.
- `PERRY_GC_RESTORE_DIRTY_PAGES=0` selects the whole-owner walk. `=verify` runs both and aborts if the
  whole-owner walk finds an entry the bounded one did not restore; test and debug builds always do.
  `PERRY_GC_DIAG=1` reports `clean_slots_skipped=` and `walk=` on the `[gc-restore-coverage]` line.
