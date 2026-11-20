Ordered `Map` deletes are O(1) instead of O(N).

Emptying an N-entry Map was O(N²) three ways at once: every delete memmoved the
trailing entries down, span-barriered every moved slot, and walked all three
side indexes decrementing every offset above the hole. Per-delete cost doubled
with N (0.96 µs at N=2k to 8.2 µs at N=16k) where node is flat at ~0.03 µs —
263× at N=16k. On the ECS archetype-migration row, `delete_entry_at_index` and
its memmove were ~14% of the frame.

A delete now tombstones in place: the key slot takes a reserved hole marker, the
value slot is cleared through the barriered store so SATB marking still shades
the overwritten child, and the live `size` drops while the array extent (`used`,
a new `MapHeader` field) stays put. Raw entry indices are therefore stable — no
shifting, no span barrier over the tail, and the side indexes only forget the
deleted key, so the offset-repair walkers are deleted outright.

The hole marker can never collide with a stored key: `normalize_zero`
canonicalizes it to `undefined` on the resolved-key path, the string-key path
writes a STRING_TAG-boxed pointer that cannot equal it, and compaction only
moves keys that were already normalized on insert.

Insertion order is unchanged: iteration walks raw indices and skips holes, and
delete-then-re-add still appends. Compaction runs when tombstones outnumber live
entries, before growing, or when a raw-indexed accessor observes holes — after
which the typed `for…of` lane's `used == size` admission holds again, so the
codegen lane self-heals rather than misreading holes.

The GC contract bounds the Map slot descriptor's range by `used`, with
`size ≤ used ≤ capacity` as the corruption guard; holes are non-pointer markers
the tag-filtered scan skips.
