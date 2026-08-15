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
