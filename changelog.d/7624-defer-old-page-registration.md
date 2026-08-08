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
| size cap (8k entries, 128 KB) | bound | the buffer cannot grow without a collection |
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

**Tests.** Seven per-obligation unit tests in `arena/tests.rs`, all
**sabotage-verified**: a harness removes one flush site at a time and requires
the matching test to go red — 9 cases, 9 caught. That includes "revert the
promote path to eager registration", which turns
`old_gen_birth_defers_its_page_registration` red, so a later refactor cannot
silently make this inert (the #7024/#6942 "gate whose subject never ran"
failure mode). `every_cycle_constructor_routes_through_the_flush_point` is the
second half of the cycle-start claim: one test proves
`old_pages_begin_gc_cycle` flushes, that one proves all three constructors
still call it. `every_old_gen_birth_path_sets_tenured` stays green.

Those seven pin the flush sites that exist *today*; they are blind to one added
later. `deferred_registration_flush_sites` closes that — it enumerates every
function in `page_meta.rs` touching either table and requires a flush or a
written exemption, and fails on a **stale** exemption too so the list cannot rot
into suppression. It is not hypothetical: on its first run it caught
`OldArenaPageObjectCursor::next` (deliberately flush-free, now exempt with the
argument attached). Both of its arms are sabotage-verified — a bogus exemption
and a newly added unflushed toucher each turn it red.

## Measured — pinned quiet host (`perry-macos`, M1 mini, load ~1.3)

Both arms `perry-dev`, identical package set, one target dir each. Workloads
compiled on the dev Mac with `PERRY_NO_AUTO_OPTIMIZE=1` and the **prebuilt
executables shipped** to the mini, so nothing was rebuilt on the measurement
host. Run only after the owner's `run_public_baseline` had exited. 5 rounds,
base/fix interleaved within each round, every row hash-verified.

### json_pipeline (medians of 5)

| | base | fix | Δ |
|---|--:|--:|--:|
| 200k wall | 1.64 s | **1.56 s** | **−4.9%** |
| 200k user CPU | 1.58 s | **1.50 s** | **−5.1%** |
| 200k peak RSS | 489.0 MB | **471.1 MB** | **−3.7%** |
| 500k wall | 4.36 s | **4.18 s** | **−4.1%** |
| 500k user CPU | 4.21 s | **4.02 s** | **−4.5%** |
| 500k peak RSS | 1,110.4 MB | 1,114.7 MB | +0.4% |

Fix is faster in every paired round at both sizes. (The one 200k `fix` row
reading 2.11 s is round 1 only, first touch of a freshly rsync'd binary — its
*user* CPU is 1.54 s, i.e. normal; it is I/O, not compute, and it is left in
the raw log rather than dropped.)

Output hashes identical at both sizes, and the **GC census is identical** at
both sizes — same cycles, same `promoted_objects`/`promoted_bytes`, same sweep
and reclaim. That is the check that this is bookkeeping and not a behaviour
change: 200k promotes 1,657,962 objects / 113,226,896 bytes and 500k promotes
4,117,011 / 280,996,760, all through the path this PR touches, and none of it
moves.

### gc bench set (medians of 5, `gc-handoff/bench`)

| workload | wall Δ | RSS Δ |
|---|--:|--:|
| retain | **−4.2%** | −0.6% |
| retain1 | **−4.3%** | **−6.6%** |
| deeplist | −2.3% | +1.2% |
| tree | −0.5% | −2.4% |
| churn / churn_alloc / churn_num / churn_read / push_num | 0.0% | +0.1…+1.3% |
| cycles | +1.0% | +1.0% |
| push_cls | +2.5% | +0.4% |

All eleven produce byte-identical stdout. The wins land where the mechanism
predicts — `retain`/`retain1`/`tree`/`deeplist` are the promote-heavy ones. The
two small positives are at the 10 ms resolution of `/usr/bin/time` on 0.4 s and
1.0 s workloads.

### What the measurement changed in the patch

Both are recorded because they are the reason the final numbers look the way
they do — and because one of them is a correction to a claim I made first.

1. **+31 MB peak RSS at 500k** (1,110 → 1,142 MB, reproducibly, all 5 rounds).
   Not the deferral — the *flush*: ~63 flushes per run, each `mem::take`ing the
   pending buffer so the next burst re-grew a ~1 MB `Vec` from empty, plus a
   second ~1 MB staging `Vec` for the page-meta updates, allocated and freed per
   batch. The flush now holds both table borrows at once and applies the meta
   update inline, and hands the pending buffer back to its thread-local. **A
   flush allocates nothing.**
2. **The 64k-entry cap** (1 MB resident) was inherited from #7623, where the
   buffer backed a different shape. Cut to **8,192 entries (128 KB)** — it still
   amortises the per-batch loop ~8,000×, and with an allocation-free flush the
   extra batches cost only the loop entry. This was done believing it would
   clear the `gc-ratchet` row below; it did not, and that is written up there
   rather than quietly dropped.

### gc-ratchet (the #7609 baseline), both arms, same session

Both arms measured in the same session on the pinned host, `measure --repeats 7`
then `check` on both profiles.

- **`shared_ci`** (the profile CI gates on): **OK on both arms.**
- **`pinned_host`** (stricter — also gates memory and time): base **0 regression
  rows**; fix **1**, `11_collect_at_depth.rss_bytes` 34,652,160 → 35,749,888
  (**+3.17%**, band 1,039,565).

Every GC counter on every probe is `+0.00%` on both arms — `copied_objects`,
`copied_bytes`, `promoted_objects`, `promoted_bytes`, `freed_bytes`,
`minor_cycles`, `step_cycles`, `heap_used_bytes`, `heap_total_bytes` — which is
the same "bookkeeping only" result the census gives, reproduced by an
independent harness across twelve probes.

**The one open row, stated honestly.** I first attributed it to the deferral
buffer and cut the cap 64k → 8k to remove it. **That was wrong, and the
measurement says so**: the cell did not move (+1,081,344 B at a 1 MB buffer →
+1,097,728 B at a 128 KB buffer — it should have shed ~0.9 MB). Two further
facts point away from the deferral: `11_collect_at_depth` promotes **zero**
objects, so this PR's path is inert on it, and the base arm produced zero
regression rows on the same host in the same session, so it is not host drift
either. The remaining hypothesis — untested — is allocator segment granularity
shifting under a runtime ~10 KB larger. Flagged rather than explained away; it
does not affect `shared_ci`, and the 8k cap is kept because 128 KB is better
than 1 MB on its own terms, not because it fixed this.

