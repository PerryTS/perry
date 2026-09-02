**Map/Set: an array-like `map[i]` / `set[i]` read inside a `forEach` callback no longer makes the walk skip entries.**

`#9504` made the array-like indexed read on a collection a *live-index*
accessor: it squeezes tombstones first so raw index == live index, and never
hands out a hole. `forEach` walks the raw entries with a counter that is
protected against the *delete-path* squeeze (the walk registers itself and
that squeeze defers) — but not against the accessor's. A callback that deleted
already-visited entries and then read `map[j]` compacted the buffer under the
walk's counter, shifting the survivors below it: with two earlier entries
deleted, two later ones were never visited (18 of 20).

While a `forEach` walk is active the accessor now defers the squeeze exactly
as the delete path does, and resolves the live index by stepping over the
tombstones (O(idx), on a path that is rare by construction); the outermost
walk's completion performs the deferred squeeze as before. Outside a walk the
accessor squeezes as `#9504` specified. The `for…of` fast path is unaffected —
its cursor rebases through the compaction log (`#9513`), so a squeeze under it
was already exact.

Found by the automated review on #9513. Regressions: runtime unit tests for
Map and Set (visit set, live values read mid-walk, layout left alone during
the walk, squeezed on completion) and
`test_gap_foreach_live_index_read_no_skip.ts` (single and nested walks),
node-differential.
