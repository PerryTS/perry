### Old-object page registration is deferred off the promote path (#7624)

Extracted from #7623 per its audit: that PR's static-pretenure half was a
measurement confound and is not merging, but the `register_old_object_pages`
finding inside it stands alone — and pays on current `main`, with no codegen
change and no allocator-policy change.

**The cost.** `register_old_object_pages` was written for the occasional
old-gen birth. Per object it pays two `RefCell` borrows, two `Vec` allocations,
a hash lookup, and a **linear `contains` scan of that page's object list** —
which grows as the page fills, so a burst of births into one 4 KiB page is
quadratic in the objects it lands there. Since **#7613's promote-on-first-copy**
that is no longer an occasional path: a copying minor promotes straight into
old-gen (`gc/copying.rs`'s `move_young` → `arena_alloc_gc_old`), so
json_pipeline pushes ~113 MB of promotions per run through it.

**The change.** `arena_alloc_gc_old` records `(header_addr, total_size)` in a
thread-local buffer (`arena/page_meta.rs`); one batched flush folds the burst
in, holding a single borrow of each table, allocating no per-object `Vec`, and
scanning only the portion of a page's object list that **predates the batch**.
A bump-allocated promotion burst fills fresh pages, where that prefix is empty
and the dedup scan disappears. Skipping in-batch entries is sound because they
are pairwise distinct — an address cannot be handed out twice without an
intervening free, and no free happens without a flush; hole reuse, the reason
the dedup exists, hands back an address registered *before* the batch and is
still covered.

Allocation policy is deliberately unchanged: the `old_free_take_exact` hole
probe stays. (#7623 also dropped it on its pretenure allocator; that is a
separate change with its own RSS consequences and is not here.)

**Caller disposition.**

| caller | disposition | why |
|---|---|---|
| `gc/copying.rs:614` promote (`arena_alloc_gc_old`) | **defer** | the target: per-object, ~113 MB/run since #7613 |
| `gc/oldgen.rs:1735` evacuate-tenured-nursery (`arena_alloc_gc_old`) | **defer** | per-object, same function |
| `typedarray`, `buffer`, `native_arena`, `json_tape` (via `arena_alloc_gc_old_born_tenured`), `arena_alloc_gc` large-object arm | **defer** | inherited; rare/large, so neither helped nor harmed, and one code path is easier to reason about than two |
| `gc/oldgen.rs:1843` defrag relocation (`arena_alloc_gc_old_excluding_pages`) | **eager** | rare; per-object cost dominated by the `copy_nonoverlapping` beside it; runs inside `old_arena_walk_objects_on_pages`' callback. Keeping it eager narrows the proof obligation |

**Soundness — one rule.** Every reader **and every remover** of
`OLD_GEN_PAGE_OBJECTS` / `OLD_GEN_PAGE_META` flushes first. Both tables are
thread-locals private to `arena/page_meta.rs`, so the toucher set is closed and
the rule is checkable.

Removers matter as much as readers, and that is the part that is easy to get
wrong: a removal that runs while an entry is still deferred is a **no-op**, and
the later flush then puts the dead object back — a resurrected index entry
pointing into swept or recycled memory.

| flush site | kind | why it cannot rely on cycle start |
|---|---|---|
| `old_pages_begin_gc_cycle` | cycle start | all three constructors route through it (`gc/mod.rs` minor, `gc/cycle.rs` `new_full`, `gc/policy.rs` budgeted) |
| `old_arena_walk_objects_on_pages` | reader | a copying minor's root scan promotes **before** the remembered-set walk reads the index |
| `OldArenaPageObjectCursor::new` | reader | same index, incremental (budgeted) reader |
| `old_page_summary` | reader (`META`) | a deferred entry also owes `allocated_bytes`/`object_count` |
| `old_page_meta_snapshot` | reader (`META`) | drives `gc/oldgen_defrag.rs` page selection — real policy |
| `old_pages_reset_sweep_accounting` | reader (`META`) | closes the promote→sweep window inside a full cycle |
| `old_page_meta_for_tests` | reader (`META`) | keeps existing allocate-then-inspect tests honest |
| `unregister_old_object_pages` | remover | resurrection |
| `old_arena_page_index_remove_object` | remover | resurrection |
| `unregister_old_block_pages` | remover | resurrection into a recycled block |
| size cap (64k entries, 1 MB) | bound | the buffer cannot grow without a collection |
| `old_arena_page_index_clear_for_tests` | **discards** | a caller asking for an empty index must not get a repopulated one |

Two consequences worth recording:

- `classify_heap_generation` — every barrier remember-decision — reads the
  **block-level** `PAGE_GENERATIONS` map, populated by `register_old_block_pages`
  when a block is created. It never consults the object index, so it is
  unchanged. (The #7623 audit reached the same conclusion for its shape; this
  was re-verified for this caller set.)
- The per-object `META` writers (`old_page_account_swept_object`,
  `old_page_account_promoted_object`) call `refresh_policy_bits()`, which reads
  `allocated_bytes`. They can run while a registration is pending and therefore
  recompute a bit from a stale count — but the flush itself calls
  `refresh_policy_bits()` for every page it touches, and every reader flushes
  first, so no reader can observe a stale bit. They stay flush-free so the
  per-object sweep path pays nothing.

**Tests.** Seven unit tests in `arena/tests.rs`, one per obligation, all
**sabotage-verified**: a harness removes one flush site at a time and requires
the matching test to go red — 9 cases, 9 caught. That includes "revert the
promote path to eager registration", which turns
`old_gen_birth_defers_its_page_registration` red, so a later refactor cannot
silently make this inert (the #7024/#6942 "gate whose subject never ran"
failure mode). `every_cycle_constructor_routes_through_the_flush_point` is the
second half of the cycle-start claim: one test proves
`old_pages_begin_gc_cycle` flushes, that one proves all three constructors
still call it. `every_old_gen_birth_path_sets_tenured` stays green.

<!--MEASUREMENTS-->
