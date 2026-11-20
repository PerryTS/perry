`delete obj[k]` stops allocating when the keys array is owned.

The delete cloned the keys array unconditionally, because one array is shared
by every object that built the shape through a `transition_cache_lookup` hit —
mutating it in place would drop entries from siblings that never deleted
anything.

Sharing is tracked, though: the caches stamp `GC_FLAG_SHAPE_SHARED` when they
publish an array, and both the ordinary `[[Set]]` growth path and
`object_ops::keys_array` already treat that bit as authoritative for exactly
this clone-or-mutate decision. An array without it has a single owner and can
be compacted in place, which removes the last per-delete allocation here — a
~500-element clone, 200k of them on `bench_populated_delete.ts` — and keeps
the array's address, so the key index needs only a slot shift rather than a
migration to a new id.

**The shape must be re-published** for the same array at its new key count.
Without that the object keeps a stamped ShapeId whose descriptor still claims
the pre-delete count, and the shape-facts audit catches it outright
("published ShapeId disagrees with authoritative ObjectHeader facts") — an
earlier version of this change failed precisely there.
`publish_object_shape_from` versions a same-pointer change internally, and
`keys_changed` is false, so the typed layout is preserved rather than marked
unknown.

Interleaved A/B, min-of-13 at quiet load (~1.0): `bench_populated_delete`
2768 → **2026 ms, −26.8%** (mean −27.0%). Combined overwrite and
realistic-name read unchanged.

Cumulative this session: **5938 → 2026 ms, −65.9%**, i.e. ~280× node → ~96×.
Suite 2788 passed, no warnings; adversarial property differential and
computed-key differential both byte-identical to node.
