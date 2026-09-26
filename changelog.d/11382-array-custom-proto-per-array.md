perf(array): `Object.setPrototypeOf` on one array no longer takes every
array off the index fast path (#10593).

Retargeting a real array's `[[Prototype]]` used to flip the process-wide
`PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED` byte that every inline element guard
loads. From then on every element read, store, push and pop in the program went
out of line — and every element store paid the full old-to-young write barrier
(`mark_dirty_old_page_uncached` + the remembered-set insert) instead of the
inline one. One line of ordinary setup made the issue's fixture 33x slower
(1.51 G -> 50.0 G retired instructions).

The fact is per-array, so it now lives on the array: arrays are never
meta-capable, so `Object.setPrototypeOf(arr, p)` always records into the
residual prototype registry, which already sets `GC_RESIDUAL_PROTO_OWNER`
(bit 6 of `_reserved`) on the owner and carries it across growth and every GC
relocation (#10362). It is now also named `GC_ARRAY_CUSTOM_PROTO`, and every
inline guard tests it beside the byte:

- codegen: a shared `emit_array_default_prototype_chain` helper
  (`expr/array_proto_guard.rs`) replaces the bare byte load in the guarded
  index read/store, dynamic-index read, `push`/`pop`, logical-collection scan,
  cached-field index return and versioned indexed loop preheader (the
  per-iteration guard already compares the header fingerprint, which includes
  the bit);
- runtime: `array_index_fast_path_invalid_for(reserved)` replaces the byte in
  the direct push/pop lanes, the strict numeric store lanes, the proxy
  object-array numeric write loop, the subclass/plain loop guards and the
  typed-feedback `plain_array_index_guard` (which also used to decline for
  `array_static_proto_recorded()` globally).

The byte keeps its genuinely global meanings — an index on `Array.prototype` /
`Object.prototype`, a retargeted `Array.prototype`, and (conservatively) a
retargeted lazy JSON array, whose materialized storage is a separate
allocation without the owner's bit. `ARRAY_TARGET_PROTO_RECORDED` still gates
the slow paths' per-array side-table probe.

Tests: `array::custom_proto_scope_tests` (the byte stays clear, the retargeted
array alone carries the bit, the bit survives growth, a lazy array still
invalidates globally), the `pop` codegen IR test now requires the per-array
mask, and `test_gap_10593_array_custom_proto_scope.ts` pins the custom-chain
semantics (hole/OOB reads, inherited setters on hole stores, a retarget
mid-loop, an array held in an object field) next to unaffected bystander
arrays. Two pre-existing slow-path gaps were found and left out of the gap test
because they reproduce with the byte forced on: `push` onto a retargeted array
bypasses an inherited index setter, and `pop` of a trailing hole does not read
through the custom chain.
