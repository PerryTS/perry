`js_typed_array_index_get_dynamic` answered `undefined` for a receiver its own
classifier had just accepted as a typed array.

The two receiver gates in the element-READ path are not the same predicate.
`classify_element_read_receiver` (`typedarray/mod.rs`, #8109) answers
`TypedArray` on a registry hit **or** on a `GC_TYPE_TYPED_ARRAY` /
`GC_TYPE_NATIVE_TYPED_VIEW` managed header — deliberately, and its doc comment
states the intent: "a lookup failure can only cost the diversion, never the
element read". `typed_array_addr_from_value` (`typedarray_props.rs`) gates on
`typed_array_owner_kind`, i.e. the thread-local registry alone. Where those
disagree, `typed_array_index_get_dynamic` fell through to `_` and returned
`undefined`.

The consequence is an inconsistency between two helpers, not just a dead arm:
codegen emits `js_typed_array_get` for a proven integer index and
`js_typed_array_index_get_dynamic` for a runtime key, on the same receiver in
the same function. `js_typed_array_get` reads a header-established typed array
correctly (it is `classify_element_read_receiver`-backed end to end); its
runtime-key twin did not.

**Reachability, measured.** No TypeScript-level construction of the
disagreeing receiver was found, and the census says there should not be one.
Both allocation sites register before returning (`typedarray::typed_array_alloc`,
`native_arena::js_native_arena_view` via `register_view`); the only
unregistrations outside tests are GC finalizers for a provably dead object
(`finalize_collected_dead_typed_array` runs post-full-trace, and the sweep's
own comment records that interior block space is not re-allocated before
`BlockCleanup`, so there is no address-reuse ABA) and
`native_arena::unregister_view` (`dispose_owner` does *not* unregister, so a
disposed-owner view stays registered and keeps throwing); and a typed array
cannot cross a thread boundary (`thread::unsupported_transfer_type_name`).
Both internal callers of `js_typed_array_index_get_dynamic`
(`value::dyn_index`, `object::polymorphic_index`, `typed_feedback`) pre-gate on
`lookup_typed_array_kind`, so only the codegen call site can deliver an
unregistered receiver. A live disagreement therefore implies a dangling
pointer to a collected typed array — the #7154 class — where `undefined` was
never the interesting part. A 15-receiver-shape TypeScript probe (plain array,
plain object, string, non-pointer, `Int32Array`, `Float64Array`, `Int16Array`
subarray/slice, `ArrayBuffer` view, offset view, Buffer-backed `Uint8Array`,
`Buffer.from`, `DataView`, `Uint8ClampedArray`, `BigInt64Array`, `map()`
result, symbol/expando/canonical-string keys) is byte-identical to Node
26.5.1 both before and after this change, as expected.

The fix is therefore consistency, not a repair: `typed_array_index_get_dynamic`
now resolves the classifier's `TypedArray(addr)` into an owner and runs the
same key logic (symbol side table / canonical-index string key / numeric
element) instead of falling through. The registry-gated primitives grew
`_for(owner, kind, …)` forms that take the ALREADY-RESOLVED owner kind, so the
key logic no longer re-derives it and re-loses the header-established
receiver; the registry-resolving wrappers are unchanged, so no other caller's
behaviour moves.

`crates/perry-runtime/src/typedarray/element_read_receiver_tests.rs` gains six
tests. `header_only_typed_array_is_the_disagreement_8116_names` pins the
fixture (registry MISSES, header WINS). `js_typed_array_get_reads_the_sibling_
of_the_dynamic_hole` is the control that makes the arm worth closing — it
passes with and without the fix, and shows the same receiver already reads
correctly through the constant-index helper. Three tests assert recovered
VALUES through the dynamic helper (`70000 & 0xFFFF == 4464`, so a fallback
that read boxed-f64 slots instead of 16-bit lanes fails them), covering the
numeric key, the canonical numeric-index string key and a Symbol key. Two
controls guard over-reach: `Absent` receivers stay `undefined`, and a
`Uint8ArrayBuffer` owner still reads its bytes through `js_buffer_get`
(different payload offset — serving it through the typed-array lane reader
would not answer 250).

Not fixed here, found by the same probe and filed as #8149: a `DataView`
and a raw `ArrayBuffer` are byte-indexable in Perry (`(dv as any)[0]` is `0`,
Node's `undefined`), and `Object.keys(buffer)` is `[]` where Node lists the
byte indices. Both reproduce with no typed-array static hint at all, so they
live in `js_dyn_index_get`'s registered-buffer arm, not here.
