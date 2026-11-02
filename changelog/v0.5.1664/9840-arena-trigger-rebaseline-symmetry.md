### Fixed

- **A nursery collection triggered by malloc pressure now re-baselines the
  whole-arena GC trigger, as `GC_NEXT_TRIGGER_BYTES`'s own contract already
  said it did.** The cell is documented as "bumped after each
  `gc_collect_inner` based on collection effectiveness". It was not:
  `gc_finish_arena_trigger_collection` re-baselined it, and
  `gc_finish_malloc_trigger_collection` — the finisher for the *same nursery
  collection* with the malloc sweep added — did not. So the whole-arena
  threshold was measured from the last **arena-kind** collection rather than
  from the last collection, and a run of `MallocCount` minors could walk the
  arena total across a threshold nothing had refreshed.

  The asymmetry predates the budgeted split (`9d3bd2e3b`'s pre-split
  `gc_check_trigger` had the same two branches) and is correct in the *other*
  direction, which is unchanged and still pinned by
  `test_gc_check_trigger_copied_minor_without_malloc_sweep_preserves_malloc_trigger`:
  an arena minor that skips the malloc sweep must not move the malloc trigger.
  A `MallocCount` minor has no such exemption — it swept the arena.

  Measured on the perry-compiled claude-code TUI (`PERRY_GC_DIAG=1`, per
  firing, four 3300-character captures across two independently built
  binaries): the streaming turn ran a strict 6:1 pattern in which six
  `MallocCount` minors promoting ~3.2 MB each crossed the stale arena
  threshold *inside the sixth minor*, and at the very next safepoint the arena
  arm fired on a nursery of **856 bytes** (`promoted_bytes=216
  freed_bytes=640`) — one collection in seven, paying the whole
  per-collection fixed cost (root scan, side-table prune, dirty-page restore)
  to free 640 bytes.

  **The length matters and every figure here states it.** Shape (b) needs a
  run of promoting `MallocCount` minors to walk the total across the stale
  threshold, so it exists on the long reply only: the 3300-character captures
  run 48–62 `MallocCount` firings each, and the 400-character capture runs
  **zero** (its collections are all `ArenaBytes` plus a handful of
  `OldGenBytes`). At 400 characters this change is therefore expected to be
  flat on every counter, and that is a prediction rather than a hope — there is
  no producer for the shape at that length, with or without the in-flight
  change that moves `RegExpHeader`s (the arm's only measured input on this
  program) to the nursery.

  Full collections are deliberately excluded: after a full that released
  blocks, the un-moved trigger sits *further* above the new total, which is the
  conservative direction, and a full's cadence belongs to the old-generation
  band rather than to this arm.

  This is the same symmetry #9831 gave the tiny-parse pressure guard's base
  cell, applied to the one pacing quantity still keyed to a single collection
  kind. The two cells are in different units on purpose and stay that way: the
  guard's base is `arena_in_use_bytes()` (bump offsets, what it reads at each
  parse boundary); this trigger's base is `arena_total_bytes()` (committed),
  which is what `next_arena_trigger_base()` is compared against.

  One coupling beyond the trigger, stated because it touches a fix that landed
  hours earlier: `GC_STEP_BYTES` had exactly one production writer — the arena
  finisher — and #9831 made it an *input* to the tiny-parse pressure guard's
  headroom. Scoring a `MallocCount` minor's productivity therefore moves that
  guard too. That is the same symmetry rather than a side effect (the step is
  documented as "collection effectiveness", not "arena-kind collection
  effectiveness"), and on the compiled claude-code TUI it moves the guard in
  the *conservative* direction. Estimated over 209 `MallocCount` firings in the
  four 3300-character captures, `pct_freed` has a median of 4–5 % and lands
  `<10 %` in 194 cases, `10–24 %` in 10, `25–84 %` in 5 and `>84 %` in none —
  so 93 % of these collections take the "< 10 % → double" band and push the
  step up, which raises the guard's headroom. `[gc-arena-rebaseline]` carries
  `pct=` and `step=` for both arms so this is read off a capture rather than
  estimated from a neighbouring diagnostic, which is all the pre-fix diag
  allowed.

  The arm's dueness predicate is byte-identical, so when it is due it still
  fires the same collection. `PERRY_GC_ARENA_REBASELINE_ALL=0` restores the old
  asymmetry — its OFF state is asserted in CI as the knob kill-policy requires,
  by a test that is simultaneously the sabotage proof for the two ON-state
  tests (the OFF branch *is* the deleted call) — and `PERRY_GC_DIAG=1` gains a
  `[gc-arena-rebaseline] arm=…` line attributing each re-baseline to the
  finisher that performed it.
