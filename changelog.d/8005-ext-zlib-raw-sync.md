### Fixed

- **`perry-ext-zlib` now implements `deflateRawSync` / `inflateRawSync`**, so
  `test_gap_zlib_4917_level` links on the auto-optimize path instead of failing
  with two undefined symbols.

  When the well-known flip routes `node:zlib` to `perry-ext-zlib`,
  `optimized_libs/driver.rs` strips the per-codec features from the stdlib
  rebuild on a stated premise: *"The ext crate carries all codecs, so nothing is
  lost by dropping them here."* That is true of the codecs and false of the RAW
  one-shot entry points — `js_zlib_deflate_raw_sync` and
  `js_zlib_inflate_raw_sync` existed only in `perry-stdlib`, so the flip removed
  them from the link.

  **Note the ABI.** Codegen declares this pair as `(DOUBLE, DOUBLE)` and
  `(DOUBLE)`, unlike the zlib-format one-shots beside them which take their data
  as `I64`. The parameter types match the declaration rather than this crate's
  local convention: the bits are the same NaN-boxed value either way, so a
  mismatch would link cleanly and misread the argument.

- **The flip's premise is now checked, not just asserted in a comment.**
  `ext_zlib_covers_every_stdlib_symbol_the_flip_strips` scans both crates for
  exported `js_zlib_*` symbols and requires the ext surface to be a superset,
  minus an explicit shrink-only `KNOWN_EXT_GAPS` list. A symbol that leaves
  stdlib, or gains an ext implementation, must be deleted from that list in the
  same commit — an entry matching nothing fails.

  Writing the check found the gap is wider than the reported pair: **19 further
  `js_zlib_*` symbols** exist only in stdlib. Most are stream constructors the
  ext crate serves through its own dispatch, and pump plumbing supplied by the
  `external-zlib-pump` feature the flip *adds*; each is listed with its reason.
  Five are genuine one-shot gaps of exactly the #8005 shape — `js_zlib_crc32`,
  `js_zlib_deflate_raw`, `js_zlib_inflate_raw`, `js_zlib_unzip`,
  `js_zlib_unzip_sync` — which have not broken a link only because no gap test
  links them on this path yet.

  Affected files: `crates/perry-ext-zlib/src/lib.rs`,
  `crates/perry/src/commands/compile/optimized_libs/tests.rs`.

  Validation: sabotage-verified in both directions. Removing the #8005 pair from
  the ext crate fails the check by name; making an unlisted stdlib symbol
  disappear reports it as missing. The scan also asserts its own subject is live
  (>20 stdlib and >10 ext symbols found), so a broken matcher cannot make the
  superset check vacuously true.
