Fixed nine fused `js_array_*` callback/reduce entry points that returned
**garbage values** — not empty, not a throw — for a Buffer-backed `Uint8Array`
receiver codegen could not statically prove (#8137).

`holder.u.map(x => x * 2)` answered `[1.297723e-318, 0, 0]` where node answers
`[6,2,4]`; `holder.u.reduce((a, b) => a + b, 0)` answered `6.4886e-319` where
node answers `6`. Also affected: `filter` (`[]`), `find` (`undefined`),
`findIndex` (`-1`), `some`/`every` (`false`), `reduceRight`, `forEach`, and
`js_array_map_discard`.

Perry's `new Uint8Array([…])` is a `BufferHeader` (`buffer::js_uint8array_new`),
not a `TypedArrayHeader`, so it is absent from the typed-array registry and the
`lookup_typed_array_kind` re-dispatch each helper performs never answers for it.
The helper then reads the `BufferHeader` as an `ArrayHeader`. Both share the
`{length: u32, capacity: u32}` prefix, so `length` reads CORRECTLY while the
elements — decoded as NaN-boxed f64 slots at `base + 8 + i*8` over a payload of
one byte per element — are raw bytes reinterpreted, `length * 7` bytes past the
real payload. Correct length, garbage values, which is why the symptom is not an
empty result and why a probe that only asks "did we return `[]`?" is blind to it.

This is a **standing gap**, not the #8041 funnel-ordering regression #8090 /
#8109 / #8119 / #8130 / #8140 have been closing: `normalize_array_receiver` is
permissive for a registered Buffer and returns the raw address rather than null.
The new receiver-kind question is nevertheless asked ABOVE the funnel, because
the sibling funnel `clean_arr_ptr` DOES null the same receiver and every caller
reads that as "empty" — keeping the question above means the ordering cannot rot
back into that shape.

The new `array::buffer_receiver::buffer_receiver_dispatch` delegates to
`dispatch_uint8_buffer_method`, the shared uint8 `%TypedArray%.prototype`
dispatcher a *statically typed* receiver already reaches through
`dispatch_buffer_method`'s catch-all. That choice is load-bearing, not
incidental: these nine pass the receiver to the callback as the 3rd/4th
argument, and the dispatcher passes the ORIGINAL Buffer. Delegating through
`buffer_receiver_as_uint8_typed_array` instead — whose answer is a COPY, and
whose own doc scopes it to the immutable methods — would have made
`u.forEach((v, i, arr) => { arr[0] = 9 })` silently lose the write.
`the_callbacks_third_argument_is_the_receiver_itself_not_a_copy` fails when the
implementation is swapped to a copy, so the choice is pinned rather than
commented.

Delegating also closed two statically-typed holes:

* `reduceRight` was wrong for `const u = new Uint8Array([3,1,2])` too, because
  codegen folds that call straight to `js_array_reduce_right` rather than routing
  it through `dispatch_buffer_method`. Measured
  `z|6.36e-314|5.09e-313|6.49e-319` against node's `z|2|1|3`.
* `findLast` had no arm in the uint8 dispatcher at all, so it threw
  `TypeError: (Buffer).findLast is not a function` on BOTH dispatch paths. Its
  sibling `findLastIndex` was already served, which is why the hole survived —
  the two are always cited together.

`ArrayBuffer` / `SharedArrayBuffer` / `DataView` are declined by
`is_typed_array_buffer`, the same gate `dispatch_buffer_method`'s catch-all uses,
so the two receiver populations cannot drift apart; none has
`%TypedArray%.prototype` and serving them would have invented iteration node does
not have. (Perry answers `[0,0,0,0]` for `ab.map(cb)` where node throws; that
divergence is pre-existing and byte-identical before and after this change.)

On the ordinary path the gate opens with
`typedarray::arena_payload_has_gc_type(addr, GC_TYPE_ARRAY)`, so `[1,2,3].map(…)`
reaches NEITHER registry. That predicate rather than a bare header-byte read is
the correction #8142 wrote into `array_receiver_gc_tag`'s doc: a Buffer comes in
both backings, and an EXTERNAL one has no `GcHeader` at all — the eight bytes
below its payload are allocator bookkeeping that can read as any `obj_type`,
`GC_TYPE_ARRAY` included, so a bare tag read would skip the probe for exactly the
receiver this function exists to catch. The fast path is asserted, not assumed:
`a_plain_array_never_reaches_the_buffer_gate` measures a `#[cfg(test)]` probe
counter on `is_typed_array_buffer` and fails if the gate is deleted, even though
every ANSWER stays correct.

18 tests in `array/typed_array_receiver_tests.rs`, all asserting **observed
element values, never a predicate**. #8137 names the trap and it has been
shipped here once already: `u.every(x => x > 0)` answers `true` under node AND
under the bug, because `1.297723e-318 > 0`. The callbacks record what they
actually saw, so a garbage read fails whatever the predicate says. Five sabotage
arms were run and reverted: gate always declines (12/12 subject tests fail, 4
controls correctly pass), gate deleted (probe-count test fails), `findLast` arm
removed (its test fails), over-reach to every registered buffer (the
ArrayBuffer/DataView control fails), and delegate-through-a-copy (the
3rd-argument test fails while the values stay correct). Sabotage A caught a
vacuous test of my own — the 3rd-argument test passed under it, because the
pre-fix helper already passed the raw buffer as the 3rd argument while reading
garbage elements — so it now carries an element assertion too.

Verified byte-for-byte against node `v26.5.1` (`.node-version`) across 36 probe
cases, with plain-array, array-like-object and `Int32Array` controls.
`cargo test -p perry-runtime --lib`: 2436 passed, 0 failed, 4 ignored.
