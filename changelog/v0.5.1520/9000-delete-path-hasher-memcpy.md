Two costs removed from the populated-delete path — perry's worst object-model
gap against node (`bench_populated_delete.ts`, ~280×).

**1. The shape key index no longer hashes a hash.** `ShapeIndex::slots` was a
std `HashMap<u64, Vec<u32>>` whose key is ALREADY an FNV-1a content hash, so
every probe ran SipHash over it — no added distribution, real time.
`hash_one::<&usize>` plus `sip::Hasher::write` were **14.7% of self time** in
that benchmark, second only to `shape_slot_lookup`, which is what performs the
probes. It now uses the `PtrHasher` the runtime's other id-keyed registries
already use.

**2. The keys-array clone is a memcpy.** Each delete allocates a fresh keys
array and previously copied the surviving keys one `f64` at a time; it is now
two `copy_nonoverlapping` runs. Safe by the code's own existing argument: the
destination is a freshly allocated, still-unpublished array whose layout is
rebuilt before publish — which is why the per-element writes carried no GC
barrier either — and the two allocations cannot overlap.

Interleaved A/B, min-of-15 at quiet load: 5938 → 5847 ms (−1.5% min, −2.2%
mean). Smaller than the profile share suggests, because the delete path is
dominated by allocator and page churn (`clear_page_erms` 5.6%, `mi_free` 4.2%,
`RawVecInner::finish_grow` 2.9%) rather than by hashing — the index is rebuilt
per delete, and each of its ~500 buckets is a separate `Vec<u32>`. That is the
next target and is untouched here.

Suite 2787 passed, 0 failed, no warnings. Adversarial property differential
(delete-then-reread, accessor over a cached slot, prototype fallback, freeze,
key-order shapes, overflow slots) is byte-identical to node.
