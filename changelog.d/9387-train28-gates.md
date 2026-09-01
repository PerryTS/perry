### Internal

- **Splits two object-module files back under the 2000-line cap.** `object/mod.rs`
  (1963) and `object/tests.rs` (1979) were each within ~35 lines of the gate and
  #9367's transition-IC work took both over. The test-only side-table root
  accessors and the transition-IC tests move to siblings, following the existing
  `own_key_probe_tests` split.

- **Refreshes the shape-descriptor census.** One new
  `object_header_size_bytes(ctx.target_triple)` callsite in `proxy_reflect.rs`
  (42 → 43) — the same `fields_base = handle + header_size` idiom already used
  twice in that file — plus one `keys_array` access relocated by the split above.
  Verified as exactly those two changes and nothing else.

- **Drops a redundant `unsafe` block** in `string/concat.rs` that `-D warnings`
  rejects.
