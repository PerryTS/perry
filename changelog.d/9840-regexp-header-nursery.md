### Performance

- **A `RegExp` header is allocated in the nursery instead of the malloc arm.**
  A JS regex literal evaluates to a fresh `RegExp` every time it is reached, and
  `js_regexp_new` allocated each header with `gc_malloc`. On the claude-code TUI
  that is, per 400-character reply (`PERRY_GC_TRACE`), **199,873 of 199,926
  malloc-tracked GC allocations — 100.0 %**, 80 bytes each, 99.2 % of them
  freed, with the malloc registry swinging **101,929 entries down to 1,689**
  across a single minor. Every one of those paid a mimalloc allocation, a push
  onto `MALLOC_STATE.objects`, an insert into the malloc-registry `PtrHashSet`
  (which rehashes as it grows), and at death a malloc-sweep visit and a free —
  old-generation prices for an object that overwhelmingly dies young.

  Nothing required the malloc arm. `GC_TYPE_REGEXP` is already declared
  `ArenaOrMalloc` and movable; `GcMoveHookKind::RegExpSideTables` already rekeys
  `REGEX_POINTERS`, `REGEX_SOURCE_TABLE` and the expando owner after evacuation,
  and `GcLayoutSlotKind::RegExpFields` already traces the header's two string
  edges and its `meta` record. What kept production on `gc_malloc` was
  finalization: the copying minor's from-space flip runs no per-object finalize
  hooks, so a nursery header that died young would leak its `Arc` programs and
  its registry entries. That is now handled exactly as `Map`, `Set` and `Error`
  handle theirs — `finalize_dead_copied_minor_from_space_regexps` after a copied
  minor, `collect_dead_registered_regexps_post_trace` at sweep entry for the
  non-copying cycle kinds, and the ordinary old-generation sweep for a header
  that has been promoted.

  Every regex program cache (`REGEX_CACHE`, `FANCY_CACHE`,
  `REPEAT_MATCHER_CACHE`, `VALIDATED_PATTERNS`, the site cache) keys on pattern
  and flags CONTENT, not on the header address, so a moving header costs them
  nothing.

  Note that this **changes the collection schedule** rather than only removing
  work: the `MallocCount` trigger loses essentially all of its input on this
  workload, while ~16 MB per reply moves into the nursery. The schedule is
  reported with the change rather than assumed unchanged.
