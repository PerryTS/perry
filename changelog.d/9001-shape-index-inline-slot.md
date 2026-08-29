The shape key index stops allocating once per key.

`ShapeIndex::slots` mapped each content hash to a `Vec<u32>`, so every index
build made one heap allocation PER KEY — and the index is rebuilt on every
populated delete, so a 500-key object was making ~500 `Vec` allocations per
`delete`. Allocator and page churn dominates that benchmark (`clear_page_erms`
5.6%, `mi_free` 4.2%, `RawVecInner::finish_grow` 2.9%), well above the lookup
work the index exists to do.

A bucket holds more than one slot only on a genuine FNV-1a collision between
two distinct property names, so the common case is exactly one. Store it
inline and promote to a `Vec` only when a collision actually occurs.

Interleaved A/B, min-of-15 at quiet load (~1.0): `bench_populated_delete`
**5810 → 5004 ms, −13.9%** (mean −13.9%, the two agree). The combined
overwrite loop and realistic-name read are unchanged, as expected for a change
confined to index construction.

Suite 2787 passed, no warnings. Adversarial property differential is
byte-identical to node.
