### Internal

- **Removed the retired legacy array index-get lowering.** `#6132` replaced
  `lower_legacy_array_index_get` with the typed-feedback guarded path — the old
  one inline-read any receiver as a plain `ArrayHeader`, which reads garbage for
  an off-heap typed array — and left it behind `allow(dead_code)` explicitly
  marked for deletion. It has had no callers since. Dropping it also puts
  `index_get.rs` back under the 2000-line cap.
