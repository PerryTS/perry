**Map/Set: mutation during `for…of` is O(1) per step again, and never skips an entry.**

`#9020` made ordered `delete` O(1) by tombstoning entries in place, and moved
the squeeze that used to happen per delete into the raw-index *readers*
(`js_map_entry_key_at` / `js_set_value_at`) as a "self-heal": the first raw
read that saw a hole compacted the whole collection. The `for…of` fast path
reads raw slots on every step, so a loop that deletes while iterating paid one
full compaction per delete — 50,000 entries with 12,500 deletes inside the walk
took 13–32 s against Node's 0.07 s (190–460×), quadratic in the collection
size; the identical deletes with no iterator open cost 0.5 s.

Worse, every raw-index cursor (the fast path, the iterator objects) recovered
from a squeeze by re-finding the last returned key and, when that key was
itself deleted, reading `cursor-1` — which assumed exactly ONE hole had been
squeezed. Deleting several already-visited entries plus the current one in a
single loop body skipped entries, and enough holes ended the loop early
(40 entries, 21 deleted at the 21st: Perry visited 21, Node 40).

Fixed the way V8 transitions its ordered-hash-table iterators: every squeeze
(`compact_*`, and `clear` while a walk may be open) records which raw indices
it removed, in a per-collection log, and bumps a `compaction_epoch` in the
header. A cursor carries the epoch it last synchronised with; one runtime call
per step (`js_map_cursor_next` / `js_set_cursor_next`) rebases it — down by
exactly the removed count below it, in order, through every record since —
then steps over tombstones and returns the next live raw index. This is exact
by the walk's own invariant (the yielded entries are precisely the live ones
below the cursor), needs no key lookup, no "iteration active" registration
that a `break`, `return` or abandoned generator could leak, and the readers no
longer compact at all. The codegen inline entry read is bounded by the raw
extent instead of requiring a dense buffer. The iterator objects use the same
rebase (their field 3 now holds the epoch). Log depth is bounded at 32 records
per collection; a cursor older than that is stepped over holes without rebase
(reaching it needs one loop body to force 32 squeezes of the same collection).

Also correct now: `clear()` inside a walk restarts the cursor at 0, so entries
added afterwards are visited, as the spec's in-place emptying of `[[MapData]]`
requires.

Tests: runtime unit tests for the no-compaction read contract, the exact
multi-hole rebase, successive squeezes + `clear`, and address reuse (Map and
Set); `test_gap_map_set_multi_delete_during_iteration.ts` covers the fast path,
the iterator objects, re-add, `clear`, and a 50k-entry churn, all
Node-differential.
