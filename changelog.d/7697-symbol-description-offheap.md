### Fixed

- **A `Symbol`'s description no longer lives in an untraced payload slot (#7246).**
  `SymbolHeader::description` was a `*mut StringHeader` inside a payload the collector
  treats as opaque bytes: `alloc_symbol` gc_malloc's the header as `GC_TYPE_STRING`,
  whose type info is `pointer_free: true` / `GcRewriteDescriptorKind::Leaf` /
  `GcLayoutSlotKind::None`. That is right for a *string*, whose payload is bytes, and
  wrong for a *symbol*, whose payload's third word was a heap pointer — and the two
  share one GC type, so no descriptor could distinguish them. A symbol that was itself
  perfectly rooted could have its description reaped or relocated out from under it,
  and `String(sym)` / `sym.description` then read recycled memory. `SYMBOL_POINTERS`
  did not close it: `scan_symbol_pointer_metadata_roots_mut` rewrites recorded
  addresses *without* marking and never looks at `(*ptr).description`.

  The pointer is removed rather than traced. `alloc_symbol` copies the description text
  off the GC heap *before* it allocates — so a description pointer is never
  live-but-untraced — and leaves the field null; the text goes into
  `FRESH_SYMBOL_DESCRIPTIONS`, keyed on `SymbolHeader::id`. The key is the point: an id
  is copied verbatim by an evacuation, so that table needs no rekey pass, no root
  scanner, and no budgeted step twin (the shape #7239 found the one real drift in). A
  `GC_TYPE_SYMBOL` would have been the principled fix and touches 190 `GC_TYPE_STRING`
  sites; tracing the description from the side table needs a weak-table liveness
  ordering the side table cannot supply. Descriptions are pruned in
  `prune_dead_symbol_pointers` on the same verdict that prunes the pointers, which pays
  down the retention cost the issue named as this option's price.

  Blast radius stays small because `alloc_symbol` has exactly two callers, both fresh
  symbols; registered and well-known symbols are `Box::leak`'d and keep the
  process-global `REGISTERED_SYMBOL_DESCRIPTIONS`. The four readers now share one
  `symbol_description_text` helper.

  Witness — the issue's own reproducer, A/B across the runtime rebuild with the same
  compiler, under `PERRY_GC_HEAP_LIMIT=8 PERRY_GC_INCREMENTAL=0
  PERRY_CONSERVATIVE_STACK_SCAN=off PERRY_GC_FORCE_EVACUATE=1`: **`B 1` 5/5 before,
  `B 0` 10/10 after** (node 26.5.1: `B 0`), with movement confirmed live on the
  after-run (87 copying minors, `copied_objects` 6008 and 4743). Plus three knob-free
  unit tests — structural (the payload pointer stays null), behavioural (the
  description survives reclamation of the string it came from, with from-space bytes
  recycled into `'Z'`-filled strings first so a stale read cannot pass by luck), and the
  prune. Sabotage-verified: restoring `(*ptr).description = description` fails all three.
