`delete obj[k]` no longer rebuilds the shape key index.

The delete clones the keys array, so the clone has a new address, misses
`indices`, and rebuilt the whole index — decoding and FNV-hashing all ~500
surviving property names on EVERY delete of a 500-key object.

The survivors are the same strings in the same order minus one, so the index
is shifted instead of recomputed: drop the removed slot, decrement every slot
above it, touch no key bytes. Only a fully-built index is carried over; a
partially built one is dropped exactly as before, since shifting it would
misalign the un-indexed tail.

Safe by construction rather than by argument: `shape_slot_lookup` re-validates
the stored key against the requested bytes before returning a slot, so an
index that is wrong yields a MISS and the caller's existing fallback, never a
wrong property.

Interleaved A/B, min-of-15 at quiet load (~1.0): `bench_populated_delete`
5023 → **4641 ms, −7.6%** (mean −7.4%). Combined overwrite and realistic-name
read unchanged.

With #9000 and #9001 this brings the benchmark from 5938 to 4641 ms (−21.8%).
Suite 2787 passed, no warnings; adversarial property differential byte-identical
to node.
