Prospective work, not implemented or measured here:

The empty-parse sample identifies repeated keyless shape publication as a
remaining cost (575 of 2,197 main-thread samples). Investigate reusing a
validated zero-key ShapeId through the existing stamped allocator. Keep the
normal object allocator, two-slot initialization, ordinary flag, GC pressure
hooks and actual output allocation. A cached scalar id must be validated
against the current worker-local descriptor on every use; stale/retired ids,
thread teardown, class-less learned width and first initialization need tests.
Do not cache or reuse the returned object. The existing
js_object_alloc_class_inline_keys_stamped validates preinstalled facts and
has a fallback; confirm its class_id=0 behavior before applying it.

Stringify's preceding small-record profile still points to string_piece,
record planning, memmove and prototype probes. Investigate passing Piece by
reference and reducing redundant short-string validation/copies. Do not add
an escape-free StringHeader flag without accounting for in-place append,
raw malformed strings and existing flags propagation: flags are currently
combined with OR, which cannot preserve an all-bytes-safe guarantee.

Keep all 38 CPU, 74 peak RSS and 36 retained RSS targets and every unresolved
checkpoint regression. The first empty leaf is not all-row acceptance.

Qualified current regressions vs tape-owned to recover: escaped stringify
+2.2242%, wide stringify +2.1389%, numeric parse +1.52%; empty parse peak
+0.375 MiB and common whole-process RSS shifts near +0.17 MiB. Unchanged
stringify source does not establish equal execution cost. Keep the paired
ranges, source/binary hashes and prior checkpoints as references.
