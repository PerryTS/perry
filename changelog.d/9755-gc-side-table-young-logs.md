### Performance

- **Minor collections no longer walk every runtime side table.** A copying
  minor's three root-scan passes — and a budgeted minor's initial root scan
  and final remark — visited every entry of the closure dynamic-prop tables,
  the string-keyed descriptor tables, the shape family/slot-index maps and
  the transition cache on every collection, to discover that nothing in them
  pointed at the nursery. On the compiled claude-code TUI
  that was ~35k shape families, ~120k descriptors and ~13k closure owners
  per walk, 41 minors per streamed reply, all reporting `slots=0`: 34–56 ms
  of scanner time per minor.

  Each of those tables now keeps a **young-entry log**
  (`crates/perry-runtime/src/gc/young_log.rs`): the keys of entries that may
  hold a pointer a minor can act on (nursery, longlived or malloc-GC — an old
  object is neither moved nor swept by any minor, and never becomes young
  again). Writers note the key before publishing the entry; a minor-scoped
  scanner (`RuntimeRootVisitor::young_scope`) visits only the logged keys,
  with the same per-entry body as the full walk, and re-logs an entry iff it
  is still relevant afterwards; a full trace walks everything and rebuilds
  the log. The copied-minor and fallback-minor dead-owner prunes of the same
  tables iterate the log as well. Under `debug_assertions` every minor-scoped
  walk first re-derives the relevant set from the authoritative table and
  panics on a key the log does not name, so a writer that forgets to note is
  a red test rather than a silently collected object. `PERRY_GC_DIAG=1`
  prints `[gc-young-log]` rows (logged / visited / kept / table size) per
  table and cycle.

  The **shape cache** was measured and deliberately left on its plain walk.
  Its canonical keys arrays are allocated in the longlived arena, which
  `addr_is_minor_relevant` must answer `true` for, so no entry ever leaves a
  log there: on the claude-code TUI the log named 100 % of the table in every
  one of 107 collections (0 % skipped) and cost **35 % more** than the walk it
  replaced. The four tables above skip 75–93 %.

- **The post-minor remembered-set coverage restore is proportional to what
  the dirty scan could not cover.** `restore_surviving_dirty_coverage`
  (#5029) re-walked every slot of every object on the pre-cycle dirty pages
  after each copying minor. The minor's own dirty scan already re-remembers
  each slot it visits with the same predicate, so objects it visited
  completely (every pointer slot on a dirty page and inside the body) are
  now skipped; multi-page arrays and owners of out-of-body buffers are still
  walked. Debug builds walk the skipped objects too and panic if the walk
  would have added a page. `[gc-restore-coverage]` reports objects
  walked/skipped and pages added.
