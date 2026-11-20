### Fixed

- **Reassigning a `Uint8Array` binding dropped element writes and read
  `undefined` (#8111).** The `Uint8Array`-specialized twin of #8100.
  `js_uint8array_get`, `js_uint8array_index_get_value` and `js_uint8array_set`
  (`typedarray/access.rs`) are a SEPARATE emission path from the two helpers
  #8109 fixed: codegen picks them from `is_uint8array_receiver`
  (`perry-codegen/src/expr/index_{get,set}.rs`), which reads
  `receiver_class_name` rather than the `local_type_hint` predicate #8100 is
  about — but it fires for a reassigned `Uint8Array` local just the same.

  Each had a three-way shape (registered typed array of the right kind /
  registered buffer / fall off the end) and TWO of those arms answered for a
  receiver that is perfectly readable: the trailing arm (a plain array, object
  or string) and the wrong-KIND arm (a registered typed array that is not
  `Uint8Array` / `Uint8ClampedArray`). Reads answered `0` / `undefined`; the
  store was dropped with no trace at all. Unlike #8100 these helpers were
  memory-SAFE — they validated before dereferencing — they just answered
  wrongly, silently, in the shipped default configuration.

  Measured against the pinned node oracle (`v26.5.1`, `.node-version`), both
  arms exit 0:

  ```
                       node             perry before
  store:               [5,10]           [9,10]                 <- write DROPPED
  dyn:                 10               undefined
  read:                5 10 2 10        undefined undefined 2 10
  obj store+read:      42 8             undefined undefined
  string read:         h i              undefined undefined
  wrong-kind:          77 12 2          undefined undefined 2
  ```

  Correct in BOTH arms: a real `Uint8Array` (`3 250 2`), a `Buffer`
  (`200 2`), a plain array (`5 10`), a plain object (`5 10`), a
  `Uint8ClampedArray` (`255 2`). `Q.length` and `Q.at(1)` were already right, so
  the binding really did hold the plain array — only the specialized element
  accessors were wrong.

  The read arms now delegate to `js_typed_array_get`, which owns #8109's
  `classify_element_read_receiver` dispatch, so this path INHERITS it rather
  than growing a third classifier — including the property that a
  `GC_TYPE_TYPED_ARRAY` / `GC_TYPE_NATIVE_TYPED_VIEW` header still wins on a
  registry miss, so a lookup failure can cost the diversion but never the
  element read. The store arm asks the same classifier directly, because
  `js_dyn_index_set` has its own return-value contract and the value arrives as
  an `i32`. `js_uint8array_get`'s ABI is a byte-typed `i32`, so a recovered
  value there goes through `ToNumber` rather than having its NaN-box bits
  reinterpreted, and `undefined` still collapses to the `0` byte sentinel
  (#6088).

  The wrong-KIND arms are REMOVED rather than kept as the issue suggested:
  `js_typed_array_{get,set}` are kind-generic (they read `(*ta).kind`), node
  reads and writes the real element there, and `js_typed_array_set` even
  performs the spec's `ToBigInt` `TypeError` for a Number written into a bigint
  view. `Uint8Array` / `Uint8ClampedArray` behaviour is unchanged and pinned by
  two control tests (`300 -> 44` wrapping, `300 -> 255` clamping).

  RESIDUAL, documented in-code and not introduced here: codegen narrows
  `js_uint8array_set`'s value to `i32` at the call site (`fptosi`,
  `perry-codegen/src/expr/arrays_finds.rs`) because the DECLARED receiver is a
  byte view, so a fractional value written through a reassigned binding arrives
  already truncated — `Q[0] = 1.5` stores `1` where node stores `1.5`. Closing
  that needs an f64-valued entry point plus a codegen change, not a
  runtime-side dispatch. Every integer store is exact.

  Validation: the issue's probe is byte-identical to node post-fix. 7 unit tests
  in `crates/perry-runtime/src/typedarray/element_read_receiver_tests.rs`, each
  asserting the recovered VALUE rather than "did not panic";
  sabotage-verified after a real rebuild — reverting `access.rs` turns 4 of them
  red, and the 3 that stay green are the intended controls.
  `cargo test -p perry-runtime --lib`: 2380 passed / 0 failed / 4 ignored
  (`main`: 2361 / 0 / 4). `perry-codegen`: 1434 passed / 11 failed, identical to
  the `main` baseline (#8092).
