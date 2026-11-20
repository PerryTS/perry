perf(gc): stop re-recording the page the write barrier just recorded (#7187 Phase B).

`mark_dirty_old_page` is the tail of the remembered-set half of the write
barrier: it inserts the written slot's 4 KiB page into the thread's
`DIRTY_OLD_PAGES` modbuf and mirrors the fact into the arena's per-page
metadata — two thread-local accesses and two hash operations, on every
old→young store. Measured on `benchmarks/app-patterns/kernels/batch.ts` with
the barrier armed, it fires **1 774 374 times and produces 517 distinct
pages**: 99.97% of the work re-inserts a page that is already there. #7170's
ranked profile puts the symbol at 6.73% of that whole program.

A **one-entry, thread-local last-page cache** now answers those repeats. The
shape was picked from the page sequence, not from intuition — simulating cache
designs over the exact sequence the barrier produces:

| shape | hit rate | calls left |
|---|---:|---:|
| **1-entry (chosen)** | **99.7817%** | **3 873** |
| 2-entry LRU | 99.8730% | 2 253 |
| 4-entry LRU | 99.8730% | 2 253 |
| 512-entry direct-mapped | 99.9423% | 1 023 |
| 2048-entry direct-mapped | 99.9531% | 832 |

The stores arrive in long same-page runs (3 872 runs, mean 458, longest
13 803), so the redundancy is *consecutive* repetition and one entry captures
it. Every larger shape buys ≤0.17 percentage points for more state, an index
computation and — for the direct-mapped variants — kilobytes of thread-local
storage on a path that runs on every heap store.

**The invariant.** If the cache holds page `P`, then on this thread
`P ∈ DIRTY_OLD_PAGES` *and* `P`'s `OldPageMeta.dirty` is already `true`. Those
are exactly what `mark_dirty_old_page(P)` establishes, so under the invariant
the call is a pure no-op. The cache can only suppress a *repeat* of a recording
that already happened, never a first one — the remembered set stays complete,
which is the whole correctness bar (a page holding an old→young edge that is
not recorded is a live object freed by the next minor). It is maintained by
three rules: the cache is populated only after **both** halves have just been
established (`arena::old_page_mark_dirty` now reports whether a metadata entry
existed, and a page recorded in only one place is deliberately not cached); it
is invalidated at every point that can falsify either half — `clear_one_dirty_
old_page`, the sole `DIRTY_OLD_PAGES` removal, plus `arena::old_page_clear_
dirty` and `arena::unregister_old_block_pages`; and it is thread-local, like
both structures it summarises, so one thread's mark can never suppress
another's.

Interaction with Phase A (#7250): an unarmed barrier never reaches
`mark_dirty_old_page`, so this cache is simply never consulted while unarmed.
The reconstruct that arms the barrier rebuilds the log through the same
function and only ever inserts, so it populates the cache exactly as the
barrier would and cannot falsify the invariant.

Measured before/after on `batch.ts` (armed via a leading `perry/gc` `collect()`;
macOS arm64, `perry-dev` profile, separate target dir per arm, artifact hashes
asserted different). Every pre-existing barrier counter is **identical** across
the two arms — `calls` 2 265 777, `non_pointer_child_skips` 280 536,
`parent_not_old_skips` 211 497, `old_to_young_slow_hits` 1 773 744,
`dirty_page_mark_attempts` 1 773 744, `new_dirty_pages` 517, `new_inserts` 517
— and the only difference is the new `dirty_page_cache_hits`, 0 → **1 770 501**.
Calls reaching the modbuf and the arena metadata: **1 773 744 → 3 243**. The
page set is unchanged and proven so in-process rather than by comparing two
ASLR'd runs: an instrumented build recorded both the set of pages the barrier
*asked* to mark and the set that actually reached the recording body, and they
are equal (517) in both arms. `batch.ts` output stays byte-identical to the
pinned Node oracle.

New `--lib` tests in `gc/tests/dirty_page_cache.rs` (four), each asserting its
own subject was live rather than that nothing broke: the fast path really
fires (`dirty_page_cache_hits > 0` under a forced trace guard), a genuinely new
page is never swallowed, a clear really invalidates — so a store to the same
page afterwards is recorded again — and a minor whose stores were 255/256 cache
hits still has `missing_edges == 0` with the young child marked through the
dirty page. Sabotage-checked both ways: deleting the fast path fails three of
the four, deleting the clear-side invalidation fails the completeness test with
"the next store to that page would be dropped and the old→young edge lost for
good".
