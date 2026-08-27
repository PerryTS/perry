Stopped the transition cache from keeping shapes alive: its `next_keys` edge is
now weak, and an entry whose target died is reaped.

`scan_transition_cache_roots_mut` visited `next_keys` with `visit_usize_slot`,
which **marks**. With 16384 slots the cache could therefore pin 16384 keys
arrays — and, through them, their shape descriptors — whether or not any live
object still had that shape.

That is a direct contributor to the shape table growing without bound between
full collections, measured at **786,205 descriptors on a workload holding under
400 live objects**, with the shape scanner's cost tracking it (3.6 ms → 490 ms
per call).

A transition entry is a pure cache: it answers *"adding key k to shape S yields
shape T"*. If nothing has shape T any more, the answer is worthless, so pinning
T's keys array to keep it answerable is backwards. `key_ptr` in the same entry
was already weak and metadata-only for exactly this reason; this makes the pair
consistent.

Both halves move together, and have to:

* `scan_transition_cache_roots_mut` now visits `next_keys` rewrite-only, so a
  surviving target's address stays correct but a dead one is not resurrected;
* `prune_dead_transition_cache_entries` gains `is_dead_owner(entry.next_keys)`,
  so an entry whose target did not survive is dropped rather than left dangling.

Weakening the edge without the reaping half would leave a stale pointer in the
cache.

The regression test seeds an entry with a **live** `prev_shape_id` on purpose.
An earlier version used `prev_shape_id = 0`, which the prune's pre-existing
`shape_descriptor_by_id(..).is_none()` clause already treats as dead — so it
passed with the new clause deleted and proved nothing. Sabotage-checked in its
final form: removing `is_dead_owner(entry.next_keys)` fails it.
