### Fixed

- **`Infinity.toLocaleString()` returned `"Infinity"`; node returns `"∞"`.**
  The argument-bearing forms have been correct since #9448, which routes them
  through `Intl.NumberFormat` — so `Infinity.toLocaleString("en-US")` gave
  `"∞"` while `Infinity.toLocaleString()` gave `"Infinity"`, the same
  operation disagreeing with itself depending on whether an argument was
  passed. The no-argument call is folded by codegen to the inline
  `js_number_to_locale_string`, which returned Rust's `Display` spelling.

  `Number.prototype.toLocaleString` is defined as "format with a default
  `Intl.NumberFormat`", and that formatter renders the infinities with
  U+221E. The same fast path also dropped the sign of negative zero —
  `(-0).toLocaleString()` was `"0"` where node gives `"-0"` — because
  `-0.0 < 0.0` is false; `intl::number_format` already reads the sign off the
  bit pattern, and the fast path now applies the identical test. `NaN` is
  `"NaN"` in both and does not move.

  `Object.prototype.toLocaleString.call(Infinity)` is a different operation
  (ECMA-262 defines it as `Invoke(O, "toString")`), stays `"Infinity"`, and
  has a control row.

  Affected files:

  - `crates/perry-runtime/src/date.rs` — `js_number_to_locale_string` spells
    ±Infinity with U+221E and keeps the sign of `-0`; the doc comment, which
    pinned the old spelling as intended behaviour, is corrected.

  Validation: the no-argument rows of
  `test-files/test_gap_tolocalestring_locale_options_9414.ts` are extended
  with `Infinity`, `-Infinity`, `NaN` and `-0` — as literals, as a binding,
  and as the results of `0 * -1`, `-1 / Infinity`, `1 / 0`, `-1 / 0` and
  `0 / 0`, so a constant-folded receiver and a computed one are both covered
  — alongside the argument-bearing spelling of each, the standalone
  `Intl.NumberFormat` rows, the array-element spelling and the
  `Object.prototype` control. Byte-compared against node 26.5.1.
