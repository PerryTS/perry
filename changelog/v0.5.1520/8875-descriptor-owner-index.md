Sped up `Object.keys` / `for…in` on arrays, and every GC cycle, by indexing
property descriptors by their owner instead of scanning the whole table.

Descriptors live in two process-global maps keyed by `(owner_address, key)`.
That shape answers "does owner X have key K?" in one lookup, but it cannot
answer "does owner X have **any** descriptors?" — that is a question about a
group of entries, and the map is only indexed by the full pair. So four call
sites answered it the only way that shape allows, by walking every entry and
filtering on the owner:

```rust
property_descriptors.keys().any(|(ptr, _)| *ptr == owner)
```

The cost of enumerating one small array therefore grew with how many
descriptors *every other object in the program* held. The sites:

* `js_object_keys`' array branch, twice — per enumeration, just to decide
  whether a per-index `enumerable` check was needed at all;
* `accessor_descriptor_keys_for_obj`, on the own-keys path;
* `transfer_descriptor_owner`, on every `ArrayHeader` growth;
* `scan_descriptor_roots_mut`, on **every GC cycle** — so since the moving
  young-gen scavenge became the default (#7019) this was a per-collection tax
  proportional to the program's total descriptor count rather than to what
  actually moved.

Profiling `claude -p` put 46.6% of main-thread samples in shapes/descriptors,
with a `HashMap` `Keys` iteration the single hottest self-time entry by 4× over
anything else.

`DescriptorTables` now carries `attr_keys_by_owner` / `accessor_keys_by_owner`
mirroring the two maps, so each of those becomes a hash lookup. Measured with
`Object.keys(array)` × 20 000 while unrelated objects hold N descriptors, on an
otherwise idle machine (best of 6 in-process rounds, 15 process runs; `min` is
the steady-state estimate since GC pauses only ever add time):

| descriptors elsewhere | node | before (min/med) | after (min/med) |
|---:|---:|---:|---:|
| 0 | 1 ms | 11 / 31 ms | 8 / 8 ms |
| 1 000 | 1 ms | 15 / 45 ms | 7 / 8 ms |
| 4 000 | 1 ms | 21 / 88 ms | 8 / 8 ms |
| 16 000 | 0 ms | 62 / 226 ms | 7 / 8 ms |

Before scales with descriptors on objects it never touches; after is flat, like
node, and the gap keeps widening with descriptor count. The variance goes too —
after, `min ≈ median` (7 vs 8) where before it was 62 vs 226, because the scan
was dragging the whole descriptor table through cache on every collection.

Also fixes a pre-existing correctness bug that the new tests caught:
`transfer_descriptor_owner` moved descriptors to the new address but never
carried the per-object Bloom summary (`attr_key_bits` / `accessor_key_bits`). A
freshly grown array has a null `meta`, for which
`owner_may_have_descriptor_entries` answers `false` **authoritatively** — so
after an array grew, `Object.keys` and `getOwnPropertyDescriptor` silently lost
every accessor it had. That was equally true before this change: the gate sat in
front of the old scan, so the scan never ran for the new owner either.
