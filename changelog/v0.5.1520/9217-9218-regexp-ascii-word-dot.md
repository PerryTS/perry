### Fixed

- **RegExp word escapes/boundaries and non-dotAll `.` now follow ECMAScript
  instead of Rust's defaults (#9217, #9218).** `\w`, `\W`, `\b`, and `\B`
  use the spec's ASCII `[A-Za-z0-9_]` word set; `i`+`u` additionally admits
  U+212A KELVIN SIGN and U+017F LATIN SMALL LETTER LONG S. A `.` without `s`
  now excludes all four LineTerminators (`\n`, `\r`, U+2028, U+2029), while
  dotAll still matches every character. The cheap #9216 translations for
  `[^]` and `[]` remain `(?s:.)` and `[a&&b]`, avoiding full-range case folds.
