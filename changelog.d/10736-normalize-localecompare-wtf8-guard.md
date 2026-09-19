### Fixed

- **`normalize`/`localeCompare` no longer build a `&str` from WTF-8 (#10692).**
  `js_string_normalize` and `js_string_locale_compare`/`_opts` reached their
  payloads through `string_as_str`, an unconditional
  `str::from_utf8_unchecked`. Perry stores strings as WTF-8, so a lone
  surrogate is representable on the heap as three bytes (`ED A0..BF 80..BF`)
  that are **not** valid UTF-8 — decoding them yields a `char` in
  `U+D800..=U+DFFF`, a range Rust's `char` is documented never to hold. That is
  undefined behaviour regardless of what it currently prints. Two functions
  away in the same file, `js_string_is_well_formed` and
  `js_string_to_well_formed` already check `STRING_FLAG_HAS_LONE_SURROGATES`
  before touching the payload; these three did not.

  Unsound call sites, all in `crates/perry-runtime/src/string/compare.rs`:
  `js_string_normalize` (both the subject *and* the user-controlled `form`
  argument — `"x".normalize("\uD800")` was reachable UB),
  `js_string_locale_compare`, `js_string_locale_compare_opts`, and
  `locale_compare_canonical`'s `.nfc()`/`is_nfc_quick` calls on the borrowed
  `&str`.

  The fix reuses the flag guard those siblings established, behind a new
  `crates/perry-runtime/src/string/wtf8.rs`:

  - `Wtf8Str` pairs the payload bytes with the header's lone-surrogate flag and
    hands out a `&str` **only** when that bit is clear.
  - Everything else walks `Wtf8Str::code_points()`, a `u32` iterator that can
    represent a surrogate. The well-formed arm keeps `str::Chars`, so the hot
    collation path decodes with the same core routine it always has.
  - `normalize` and `to_lowercase` split the payload into maximal well-formed
    runs at surrogate boundaries, apply the `&str` operation to each run, and
    copy lone surrogates through verbatim — the idiom `js_string_to_well_formed`
    already uses. Runs are borrowed with the *checked* `str::from_utf8`, so a
    malformed payload (a truncated lead from an FFI blob, #6085) degrades to a
    verbatim copy instead of UB.

  Run splitting is exact, not an approximation: a lone surrogate is unassigned,
  so it has canonical combining class 0 (a starter — it blocks composition and
  stops canonical reordering), no decomposition mapping, and is neither `Cased`
  nor `Case_Ignorable` (so it breaks the Final_Sigma context).

  **Behaviour is unchanged**, which was the point — the previous output was
  already right, by the accident of `unicode-normalization` tolerating an
  out-of-range `char`. It is now right by construction, and pinned: a 48-cell
  table (12 lone-surrogate inputs × 4 forms) and 10 `localeCompare` pairs
  measured under Node v26.5.1, the `.node-version` oracle, in
  `crates/perry-runtime/src/string/tests_wtf8_collation.rs` and
  `test-files/test_gap_10692_normalize_lone_surrogate.ts`.

- **`normalize()` no longer reports its own result as well-formed (#10692).**
  `js_string_normalize` returned through `js_string_from_bytes`, which does not
  derive `STRING_FLAG_HAS_LONE_SURROGATES`, so
  `"\uD800".normalize().isWellFormed()` answered `true` where Node answers
  `false` — on every lone-surrogate input. Pre-existing; surfaced by the new gap
  fixture, which is the first thing to call `isWellFormed()` on a *normalized*
  result. It also defeated the guard above one step downstream, since
  `Wtf8Str::as_str` trusts that flag. Now returns through
  `js_string_from_builder_bytes`, which derives the flag while counting UTF-16
  units and canonicalizes surrogate pairs.
