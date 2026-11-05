`delete obj[k]` stops rebuilding the shape key index — for real this time.

#9002 shifted the index onto the post-delete keys array, but the delete tail
ends with `shape_drop` on that same array, so the migrated index was discarded
immediately and the next lookup rebuilt it by re-hashing every surviving key
name. (#9002's own measured win came from pruning the STALE old entry, not
from preserving a usable index; its description has been corrected.)

A migrated index is *shifted to match* the compacted array, so it is current,
not stale. Skipping the drop when the migration succeeded is what actually
stops the rebuild. Everything else is unchanged: a partially built index, or a
delete that took another path, still drops exactly as before.

Interleaved A/B, min-of-15 at quiet load (~1.0): `bench_populated_delete`
4361 → **2781 ms, −36.2%** (mean −36.2%). Combined overwrite and
realistic-name read unchanged.

Cumulative across #9000–#9003 and this: **5938 → 2781 ms, −53.2%** on perry's
worst object-model gap. Suite 2787 passed, no warnings; adversarial property
differential byte-identical to node.
