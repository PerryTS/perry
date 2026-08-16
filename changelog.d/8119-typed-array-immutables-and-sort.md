### Fixed

- **Typed-array `sort`, `toSorted`, `toReversed` and `with` were silent no-ops
  or returned an empty array (#8096).** #8090 fixed `fill`, ranged `fill`,
  `reverse` and `copyWithin` by asking `array::header::typed_array_receiver()`
  BEFORE the `clean_arr_ptr` funnel, and named these six helpers as carrying the
  identical dead delegation:

  | helper | file | clean at | delegation at |
  |---|---|---|---|
  | `js_array_to_reversed` | `array/immutable.rs:32` | 33 | 37 |
  | `js_array_to_sorted_default` | `array/immutable.rs:59` | 60 | 61 |
  | `js_array_to_sorted_with_comparator` | `array/immutable.rs:96` | 105 | 106 |
  | `js_array_with` | `array/immutable.rs:232` | 242 | 246 |
  | `js_array_sort_default` | `array/sort.rs:542` | 551 | 561 |
  | `js_array_sort_with_comparator` | `array/sort.rs:635` | 651 | 658 |

  Codegen routes statically-typed typed-array receivers through the generic
  `js_array_*` helpers on purpose (#3148 / #654 — `is_array_expr` answers `true`
  for `Int32Array` &co.) on the contract that each helper re-dispatches on
  `lookup_typed_array_kind`. `clean_arr_ptr` rejects those receivers, and must:
  since #7574 it returns null for every tracked non-`GC_TYPE_ARRAY` object,
  because a `TypedArrayHeader`'s raw per-kind storage is not boxed-f64
  `ArrayHeader` slots. So every delegation written below the clean was
  unreachable.

  Two distinct silent failures followed. In-place `sort` returned the null'd
  pointer having sorted nothing; `toReversed` / `toSorted` / `with` returned
  `js_array_alloc(0)` — an EMPTY PLAIN ARRAY, so `.constructor.name` was
  `"Array"` and `JSON.stringify(Array.from(x))` was `[]`.

  Measured against the pinned node oracle (`v26.5.1`, `.node-version`), both
  arms exit 0:

  ```
                          node                perry before
  i32 sort default    1 2 9 10            10 9 2 1        (no-op)
  f64 sort cmp desc   10.5 9.25 2 1       1 10.5 2 9.25   (no-op)
  i32 toSorted        1 2 9 10            undefined x4    (empty array)
  f64 toReversed      4.5 3.5 2.5 1.5     undefined x4
  i32 with            1 99 3 4            undefined x4
  i8  with wraps      -56                 undefined
  ctor names          Int32Array x3       Array x3
  ```

  The plain-`Array` controls in the same program are correct in BOTH arms
  (`plain sort default: 1 10 2 9`, the ToString ordering), which is what makes
  the defect typed-array-specific rather than a general mutator break.

  `sort` and `toSorted` have to reach the typed helper for the ORDER as well as
  the layout: `%TypedArray%.prototype.sort` compares NUMERICALLY (§23.2.3.29
  CompareTypedArrayElements) where `Array.prototype.sort` compares by `ToString`.
  `[10, 9, 2, 1]` separates all three possible answers — numeric `1, 2, 9, 10`,
  string `1, 10, 2, 9`, no-op `10, 9, 2, 1` — and is what the new tests feed.
  The comparator cases feed `[1, 10, 2, 9]` for the same reason: an
  already-descending input would let a no-op pass a descending-sort assertion.

- **`Uint8Array` and `Buffer` `toReversed` / `toSorted` answered an empty array
  on every dispatch path (#8096, second receiver shape).** Perry's
  `new Uint8Array([…])` is not a registry `TypedArrayHeader` at all —
  `buffer::js_uint8array_new` returns a `BufferHeader`, registered as a buffer
  and marked `mark_as_uint8array`. So `typed_array_receiver`, which is
  registry-backed, legitimately answers `None` for the most common typed array
  in the language, while `clean_arr_ptr` still rejects it as a tracked
  non-array. Verified directly on the real constructor path:

  ```
  addr=0x20000b00008   is_registered_buffer=true   lookup_typed_array_kind=None
  gc_header_obj_type=Some(10)   clean_arr_ptr_null=true
  js_array_to_reversed(...).length = 0
  ```

  Most `Array.prototype` entry points never see this: `sort` / `with` /
  `reverse` / `fill` on that shape resolve through the dynamic method
  dispatcher, and `copyWithin` grew its own Buffer arm in #8090. But
  `toReversed` and `toSorted` fold unconditionally in HIR
  (`lower/expr_call/local_array_methods.rs` — no receiver-type guard), and the
  dynamic tower's own `toReversed` / `toSorted` arms
  (`object/native_call_method/handle_methods.rs`) call straight back into these
  same helpers, so those two were wrong for `Uint8Array` and `Buffer` on every
  dispatch path, static and dynamic:

  ```
  ann u8 toReversed:  node 4 9 2 10 1 [object Uint8Array]
                     perry 0 undefined undefined undefined undefined [object Array]
  ```

  New `buffer_receiver_as_uint8_typed_array` (`array/header.rs`) resolves that
  shape into a fresh `KIND_UINT8` %TypedArray% copy. It is sound only for the
  IMMUTABLE methods — a copy-based in-place `sort` would sort the copy and leave
  the receiver untouched, a different wrong answer — so `toReversed`,
  `toSorted` and `with` use it and `js_array_sort_*` deliberately does not. It
  declines `ArrayBuffer` / `SharedArrayBuffer` / `DataView` receivers, which
  have no `%TypedArray%.prototype` in node.

  Validation: five probes byte-identical to node `v26.5.1`, all exit 0 in both
  arms. 12 tests added to
  `crates/perry-runtime/src/array/typed_array_receiver_tests.rs` (19 in the file
  with #8090's), sabotage-verified in two stages after real rebuilds —
  reverting `immutable.rs` + `sort.rs` turns 6 of them red, neutering
  `buffer_receiver_as_uint8_typed_array` turns the other 3 red. The 3 that stay
  green in both arms are the intended controls: the plain-`Array` control, the
  `ArrayBuffer` / `DataView` decline, and the registry precondition the Buffer
  arm exists for (#8090's `clean_arr_ptr_still_rejects_a_typed_array_receiver`
  keeps #7574 pinned alongside them). `cargo test -p perry-runtime --lib`: 2380 passed / 0 failed / 4 ignored
  (`main`: 2361 / 0 / 4). `perry-codegen`: 1434 passed / 11 failed, identical to
  the `main` baseline (#8092).

  `js_array_to_spliced` needs no equivalent — `%TypedArray%.prototype` has no
  `toSpliced`.
