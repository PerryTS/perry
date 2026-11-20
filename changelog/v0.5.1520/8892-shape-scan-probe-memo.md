Sped up the shape-table GC scanner by probing each distinct keys-array address
once per pass instead of once per shape.

`scan_shape_table_rekey_mut` is the single most expensive root scanner on
`claude -p` — **178.9 ms, 53.3% of all scanner time**. Measuring it on a
shape-heavy workload showed where that goes:

| phase | calls | total | per call | share |
|---|---:|---:|---:|---:|
| mark | 10 | 61.4 ms | 6.1 ms | 10.4% |
| rewrite | 10 | 528.5 ms | 52.9 ms | **89.6%** |

The cost is not computation. Each descriptor's probe runs
`classify_heap_space_in_range` and then reads the GC header at a scattered
address — two likely cache misses, per descriptor, per collection.

Shapes share keys arrays at a measured, stable **2.5:1**, so the loop paid that
~2.5 times per distinct address. The probe is now memoised per pass. This is
sound by construction: the same addresses are visited, just once each, and
forwarding is a pure function of the address within one pass. The carrier flag
is part of the memo key — carriers take `visit_usize_slot`, which MARKS in mark
modes, so a non-carrier's cached answer must not be allowed to satisfy a
carrier's marking duty. The per-descriptor bookkeeping is lifted into a shared
helper so the memoised and probing paths cannot drift apart; only the probe is
deduplicated.

Measured on the same workload: **3074.7 ms → 2685.9 ms of scanner time, −12.6%**.

That is well short of the 2.5× the sharing ratio suggests, because a lookup in a
300k-entry map costs nearly as much as the probe it replaces — one cache miss
traded for another. The memo is therefore a reused thread-local scratch map
rather than a fresh allocation per scan; at this size, allocating one every
collection is exactly the churn the memory-parity work is trying to remove.

### The larger finding, not fixed here

The shape table **grows without bound between full collections**. On a workload
that never holds more than 400 live objects it reached **786,205 descriptors**,
and scanner cost tracks it directly: **3.6 ms → 490 ms per call**.

The mechanism: `prune_dead_owner_side_tables_copied_minor` is nursery-only by
construction, so a keys array that is promoted and *then* dies is not reclaimed
until a full collection — while the scanner walks the whole table on every
minor. That, not the per-probe cost, is why this scanner dominates.

Fixing it means either pruning promoted-then-dead shapes sooner, or not walking
the whole table on a minor. The second is the better fix and needs the collector
to say whether old-page defrag runs in the cycle: without that, skipping
tenured entries is unsound, because a defragging moving collection *can* move
them, and a stale keys pointer is silent heap corruption. Deliberately left for
its own change rather than guessed at here.
