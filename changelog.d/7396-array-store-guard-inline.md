**Fixed** the numeric array element **store** guard being paid as an out-of-line
call on element-to-element writes. `js_typed_feedback_numeric_array_index_set_guard`
was the largest single non-generated frame in `bench_array_ops` — 55 of ~660
`sample` frames on the profiled build — a five-argument cross-crate call per
stored element for a predicate that reduces to a handful of loads off two header
words and one sticky global byte. The benchmark is **1.32x faster** (min of 10
runs, 237ms → 180ms by its own timer; 4.39x → 3.33x of Node 26.5.1), with the
Node-verified `CHECKSUM:5000000000` unchanged.

An inline tier for exactly this guard already existed
(`index.rs::lower_index_set_fast`, Repsel 4a.1) and *looked* like it covered the
case. It did not: it was gated on `value_is_canonical_raw_f64` — the RHS had to
be canonical raw f64 **by construction**. A guarded array *read* lowers to a phi
over `{raw slot load, boxed fallback}`, which is not statically canonical, so the
gate excluded the most common numeric store there is: `arr[i] = arr[j]`. Both
stores in the benchmark's reverse-in-place loop fell out of the tier and called
the helper 10.5M times.

That flag was buying exactly one thing — the guard's `is_numeric_value_bits(value)`
leg being *statically* true. The `inbounds`, widened `extend_inline` and
feedback-shape `extend_inline` arms already canonicalize a non-canonical RHS
through `js_array_numeric_value_to_raw_f64`, so nothing else depended on it. It is
replaced by a **runtime** test of the same predicate, emitted only when the answer
is not statically known.

The runtime test is deliberately **stricter** than `is_numeric_value_bits`, which
also admits `INT32_TAG` payloads that are not registered class ids; reproducing
that needs the class-id registry lookup, i.e. the very call being removed.
Rejecting the whole `0x7FF9..=0x7FFF` NaN-box tag range in one unsigned range
compare covers pointers, strings, BigInts, `undefined`/`null`/booleans and INT32
at once, and sends INT32 numerics to the cold arm instead. This is load-bearing
rather than redundant with `require_numeric_layout`: that is a *static* TypeScript
judgment, and Perry does not validate declared types at runtime, so a `number[]`
slot genuinely receives `undefined` whenever a hole or out-of-bounds read feeds
one — `TAG_HOLE`'s top 16 bits are `0x7FFC`, inside the rejected range, so those
still take the helper.

**One hole found and closed.** The tier masks the NaN-box payload to 48 bits and
checked only the handle-band *lower* bound, omitting `is_valid_obj_ptr`'s upper
bound (`0x8000_0000_0000`) that `gc_header_for_user_addr` applies before the
out-of-line guard dereferences anything. A corrupted `POINTER_TAG` box carrying a
payload in `[2^47, 2^48)` therefore passed the inline tier and was dereferenced
for a **store** while the helper it fronts would have rejected it. One `icmp`
makes the tier provably no weaker than the guard it replaces. The sibling
read-side tier in `index_get/guarded_array.rs` still omits the same bound — a bad
*load* rather than a bad *store*, left alone here.

Verified: in the hot function the guard call moves out of the two hot merge blocks
into cold arms (inline-predicate blocks 0 → 2, `icmp` 78 → 106) and disappears from
the profile entirely (55 → 0 samples, generated function 243 → 196); a 71-line
differential covering negative indices, growing/sparse writes, holes read back into
a store, non-numeric values forcing layout downgrade, `-0`/`NaN` via `Object.is`,
`2^32-2`/`2^32-1`/`1e15` indices, frozen/sealed/non-extensible arrays, accessor
elements, a polluted `Array.prototype` index, and writes into an array being
iterated by `for…of` is **byte-identical to the pre-change compiler**; 47/48
`array`/`index` gap tests match Node byte-for-byte (the 48th is `node_fail`);
`cargo test -p perry-codegen --lib` 632 passed.
