### Fixed

- `JSON.stringify` of an array built by a runtime helper that writes its element
  slots directly no longer emits `null` for every non-number element.
  `JSON.stringify([...Map.groupBy("aba", ch => ch).entries()])` answered
  `[["a",[null,null]],["b",[null]]]` instead of `[["a",["a","a"]],["b",["b"]]]`,
  while `arr[0]` and `String(arr)` on the same array stayed correct — so only a
  JSON round-trip could see it.

  `js_array_alloc` stamps `GC_ARRAY_RAW_F64_LAYOUT` ("every live slot is an
  unboxed double") on the fresh, empty array, where it is vacuously true. A
  producer such as `groupby.rs`'s `group_by_make_array` then sets `length` and
  `std::ptr::write`s the element words itself, bypassing every noting store
  helper that would have cleared the flag. That mislabelling was harmless until
  `json::stringify_primitive_array` started taking the flag as proof and
  emitting each slot through `write_number`: a NaN-boxed string read as a double
  is non-finite, and JSON renders non-finite as `null`.
  `test_gap_2899_2779_2777_static_helpers` went red on `main` in the window that
  introduced that reader.

  `object::gc_slots::rebuild_array_layout_from_slots` — the choke point every
  such producer already calls to repair the GC pointer bitmap — now re-derives
  the numeric-layout flag from the same slots it just walked. The reclassify is
  clear-only, so an array that really is all-numbers keeps its fast path, and no
  receiver that used to decline now passes. `test_gap_2899_2779_2777_static_helpers`
  gained JSON coverage for string, boolean, object, mixed and numeric group
  values, plus the non-JSON readback beside it so a future failure says which
  half broke.
