Ordered `Set` deletes are O(1) instead of O(N) — the Set twin of #9020.

#8993 removed `Set.delete`'s per-delete index re-hash, but every delete still
shifted the surviving elements down and span-barriered the moved slots, so
emptying a Set stayed O(N²): per-delete cost grew 0.80 µs at N=1k to 5.36 µs at
N=8k, against node's flat ~0.027 µs.

A delete now tombstones the slot in place with the reserved hole marker, the
live count drops while the array extent (`used`, a new `SetHeader` field) stays
put, and raw indices are therefore stable — the lookup index only forgets the
deleted value, and nothing is repaired. Compaction runs when holes outnumber
live elements, before growing, or when a raw-indexed reader observes them.

The marker can never be a stored value: `normalize_zero` canonicalizes it to
`undefined` on every insert path, and compaction only moves values that were
already normalized.

Insertion order is unchanged — iteration walks raw indices and skips holes, and
delete-then-re-add appends at the end. The two raw-slot readers that could have
been defeated by a hole compact first: `js_set_to_array`'s bulk memcpy and the
subset/disjoint element walkers, so a hole can neither leak into an array nor
break a subset check.

The GC contract bounds the element range by `used`, with `size ≤ used ≤ capacity`
as the guard; holes are non-pointer markers the tag-filtered scan skips.
