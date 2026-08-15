### Fixed

- **`Array.prototype.concat` no longer drops a typed-array argument.**
  `[1, 2].concat(new Uint8Array([3, 4]))` returned `[1, 2]`; node returns
  `[1, 2, Uint8Array(2)]`. The argument vanished with no error and no
  diagnostic.

  Two defects stacked. A typed array is **not** concat-spreadable — the spec's
  `IsConcatSpreadable` falls back to `IsArray`, which is false for a TypedArray
  — but this runtime's `js_array_is_array` answers true for one, so
  `append_concat_arg` took the spread branch instead of appending a single
  element. That spread then ran through `js_array_concat`, whose
  `clean_arr_ptr` nulls every tracked typed array, so it contributed nothing.

  Before either could be reached, the all-dense bulk path in
  `dense_concat_array_source` cleaned the argument first: `clean_arr_ptr`
  returned null, the `src.is_null()` arm reported "empty dense source", and the
  bulk path returned early — so the spec-shaped flow never ran at all. The
  typed-array rejection that function already carries sits BELOW that clean and
  was unreachable for exactly the values it names.

  This is the same shape as the comment immediately above it, which describes
  a `class X extends Array` argument being mis-classified as an empty dense
  source and silently dropped.

  Affected files:

  - `crates/perry-runtime/src/array/from_concat.rs` — reject typed arrays and
    registered buffers in `dense_concat_array_source` before the clean, and
    append them as one element in `append_concat_arg`.

  The spread accumulator (`js_array_concat`) is deliberately untouched:
  `[...new Uint8Array([5, 6])]` must keep materializing elements, and
  "fixing" the ordering there instead would have traded a dropped argument for
  a wrong element count.

  Validation: byte-compared against node 26.5.1 across `Uint8Array`,
  `Int32Array` and `Float64Array` arguments, an empty typed array, a
  multi-argument call mixing plain arrays and a typed array, an empty receiver,
  and controls for plain-array concat, nested arrays, string elements, Set
  spread, and `[...typedArray]` spread — all matching.

- **A `Symbol` key on a typed-array receiver was dropped in silence**, which is
  why the `@@isConcatSpreadable` opt-in above could not be exercised by
  assignment. ECMA-262 §10.4.5.5 routes a key that is not a
  CanonicalNumericIndexString to OrdinarySet, and a `Symbol` is definitionally
  not one — but `typed_array_set_numeric_index` could not tell the two apart. A
  `Symbol` arrives as a NaN-boxed pointer, which AS AN `f64` is a NaN, so it
  took the "canonical-invalid index" arm, coerced the value for side effects,
  and returned `true` meaning "write handled". The store vanished:
  `u8[sym] = 5` then read back `undefined` and
  `Object.getOwnPropertySymbols(u8)` stayed empty, while the identical code on
  a plain object, a plain array and a `Buffer` all worked.

  Same shape as #8090/#8109/#8119/#8120/#8141: a receiver-specific fast path
  claims the operation before the key-kind question is asked.

  - `crates/perry-runtime/src/object/polymorphic_index.rs` — ask the key-kind
    question before either typed-array arm claims the receiver, and route a
    `Symbol` to the symbol side table, where `js_put_value_set` and
    `js_array_set_index_or_string` already put it. Gated on the receiver, and
    on BOTH typed-array registries, since either arm alone would still claim
    the write.
  - `crates/perry-runtime/src/typedarray_props.rs` — make the numeric-index
    arm's contract honest: decline a key it cannot classify instead of
    reporting it handled. Inert for this module's own callers, which reach it
    only under `is_int32()` / `is_finite()`.

  This makes the `@@isConcatSpreadable === true` opt-in documented above
  actually reachable by assignment: `[1].concat(u8)` with the flag set now
  gives node's `[1,9,10]`.

  Validation: `test-files/test_gap_typed_array_symbol_key.ts` byte-compared
  against node 26.5.1 across all four receiver kinds, the element-store
  control, both opt-in forms and the default. Sabotage: with the routing
  removed the compiled probe diverges from node (`ta set/get: undefined |
  ownSyms: 0`); with the numeric-arm guard removed the unit test fails.
  `perry-runtime --lib` 2385 passed / 0 failed.
