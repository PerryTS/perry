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

## Measured — pinned quiet host (`perry-macos`, M1 mini)

> An earlier revision of this description carried a wall/user/RSS table taken
> while a second `run_public_baseline` was concurrently executing on the mini.
> **That table is superseded by this one.** The GC census rows were never
> affected — they are load-independent — and are unchanged.

Both arms `perry-dev`, identical package set, one target dir each. Workloads
compiled on the dev Mac with `PERRY_NO_AUTO_OPTIMIZE=1` and the **prebuilt
executables shipped** to the mini, so nothing was rebuilt on the measurement
host. 5 rounds, base/fix interleaved within each round, every row hash-verified.

**Idleness was gated, not assumed.** The run waits for all three of: no
`run_public_baseline` process, a `SCRIPT REAL EXIT=` marker in
`/tmp/baseline_mini.log`, and 1-min load < 2.0 — then settles 60 s, measures,
and **re-checks all three afterwards**. Load recorded by the A/B itself at its
own start: `1.62 2.04 2.45`.

Deltas are **medians of per-round paired deltas**, which is the statistic the
interleaving exists to support; see the `cycles` note below for why
median-of-medians is not safe here.

### json_pipeline

| | base | fix | Δ (paired) |
|---|--:|--:|--:|
| 200k wall | 1.53 s | **1.47 s** | **−3.9%** |
| 200k user CPU | 1.48 s | **1.42 s** | **−4.1%** |
| 200k peak RSS | 489.0 MB | **486.8 MB** | −0.5% |
| 500k wall | 4.09 s | **3.94 s** | **−3.7%** |
| 500k user CPU | 3.95 s | **3.80 s** | **−3.8%** |
| 500k peak RSS | 1,110.4 MB | 1,114.7 MB | +0.4% |

**All 20 paired json deltas are negative** — 200k wall −4.5/−3.9/−4.6/−3.9/−3.9%,
500k wall −3.9/−3.7/−3.4/−3.7/−3.9%. Output hashes identical at both sizes.

```
200k base real 1.54 1.53 1.53 1.53 1.53   fix 1.47 1.47 1.46 1.47 1.47
500k base real 4.10 4.09 4.09 4.09 4.11   fix 3.94 3.94 3.95 3.94 3.95
```

**The clean host both shrank the effect and shrank the noise.** Base 200k wall
now spans 1.53–1.54 s (0.7%) where under the concurrent baseline it spanned
1.63–1.75 s (7%). The honest win is **smaller** than the superseded table
claimed (−3.9%/−3.7% vs −4.9%/−4.1%), and that table's 200k RSS "win" (−3.7%)
was noise — it is −0.5% here.

### gc bench set (`gc-handoff/bench`)

| workload | wall Δ (paired) | RSS Δ | output |
|---|--:|--:|---|
| retain1 | **−4.6%** | **−6.6%** | identical |
| retain | **−4.5%** | −0.6% | identical |
| churn_alloc | −2.5% | +0.3% | identical |
| deeplist | −2.4% | +1.6% | identical |
| churn / churn_read / churn_num / push_cls / push_num / cycles | 0.0% | +0.0…+1.3% | identical |
| tree | +0.8% | −2.2% | identical |

All eleven produce byte-identical stdout. The wins land where the mechanism
predicts — `retain`/`retain1`/`deeplist` are the promote-heavy ones.

> **`cycles` is reported at +0.0%, not +18.5%.** Median-of-medians says +18.5%;
> that is an artifact. The workload is **bimodal in both arms** (rounds 1–3
> ≈ 0.79 s, rounds 4–5 ≈ 0.96 s), so the two arms' medians land on different
> modes. The paired deltas are 0.00 in three of five rounds and the run-1
> outlier is the whole difference. This is exactly what interleaving is for, and
> it is why every number above is a paired statistic.

### GC census — identical, and load-independent

`CENSUS 200k IDENTICAL`, `CENSUS 500k IDENTICAL`: same cycle sequence, same
`promoted_objects`/`promoted_bytes`, same sweep and reclaim. 200k promotes
**1,657,962 objects / 113,226,896 bytes**; 500k promotes **4,117,011 /
280,996,760** — all through the path this PR touches, none of it moving.

### gc-ratchet (the #7609 baseline), both arms, clean host

Both arms measured back-to-back in the same session on the clean host,
`measure --repeats 7`, then `check` on both profiles. 144 cells per arm.

| | base (`origin/main`) | fix |
|---|---|---|
| `shared_ci` (what CI gates on) | **OK** | **OK** |
| `pinned_host` | **FAILED**, 10 regression rows | FAILED, 15 rows |

**Read the base column first.** Pure `origin/main` fails `pinned_host` on this
host with ten RSS rows of its own (`03_cross_gen_writes` +3.83%,
`08_map_set_sidetables` +4.20%, `04_dead_after_deep_stack` +3.72%, …). The
pinned artifact was captured at `main 26b9c9d59` (0.5.1346) and we are at
0.5.1355, so the profile's RSS bands no longer describe this host/version.
**"fix fails `pinned_host`" is therefore not a statement about this PR** — the
only sound comparison is base vs fix in the same session, which is what follows.

**fix vs base, all 144 cells:**

- **GC semantics: 107 of 108 cells byte-identical.** The single exception is
  `12_large_live_set.heap_used_bytes` (59,946,104 → 59,944,160, −1,944 B) — the
  one cell the harness explicitly de-gates by probe override because it is
  conservative-stack-scan sample-dependent, with a documented spread of 9,072 B
  over 36 runs. The difference is under a quarter of that spread. Every
  `copied_*`, `promoted_*`, `freed_bytes`, `minor_cycles`, `step_cycles` and
  `heap_total_bytes` cell is identical.
- **Memory: 24 cells, median fix-vs-base +0.23%**, range −0.30% to +1.46%
  (largest: `07_array_grow_evacuate.peak_rss_bytes` +1.46%).
- **Wall: 12 cells, median fix-vs-base +0.0%.** These probes are microbenchmarks
  where the deferral has almost nothing to do; the promote-heavy work is
  json_pipeline's.

**And this retires the open question from the earlier revision.** I had flagged
`11_collect_at_depth.rss_bytes` as an unexplained ~+1.07 MB, with "allocator
segment granularity" as an untested hypothesis. Measuring **base on the same
clean host** answers it:

| | `11_collect_at_depth.rss_bytes` | vs pinned artifact |
|---|--:|--:|
| pinned baseline (0.5.1346) | 34,652,160 | — |
| **base arm = pure `origin/main`** | 35,651,584 | **+2.88%** (ok — just under the 1,039,565 band) |
| fix arm | 35,749,888 | +3.17% (REGRESSION — just over) |

**fix is +98,304 B (+0.28%) above base, not +1.07 MB.** Base already sat at 96%
of the allowance, so the cell tips over on a rounding-scale difference. The row
is ~91% pre-existing drift in `origin/main` and ~9% this PR. No allocator-granularity
story is needed, and the one I floated should be disregarded.

