### Fixed

- `.values()` / `.keys()` / `.entries()` on a Buffer-backed `Uint8Array` no
  longer yield an empty iterator when codegen cannot statically prove the
  receiver (`holder.u.values()`, or any fully dynamic receiver).
  `array_iter_obj_raw` opens with `clean_arr_ptr`, which #8041 widened to reject
  every *tracked* non-array; `buffer_alloc` stamps a real `GC_TYPE_BUFFER`
  header, so a Buffer receiver was nulled exactly as a `GC_TYPE_TYPED_ARRAY` one
  is and every branch below the funnel became unreachable. `keys` is the proof
  this was a regression rather than a standing gap — it only reads `length`, so
  it answered correctly before #8041. The receiver is now resolved in
  `typed_array_iter_arr`, above that funnel, and receiver-tag gated so an
  ordinary array reaches neither registry — strictly fewer probes than before,
  which asked `lookup_typed_array_kind` unconditionally. `ArrayBuffer` /
  `SharedArrayBuffer` / `DataView` are excluded, matching node's `TypeError`
  (#8117).
