**Intl:** two parity fixes for #6960.

1. `Intl.RelativeTimeFormat` with `numeric: "auto"` now substitutes the en-US
   CLDR relative word forms (`yesterday`/`today`/`tomorrow`, `last`/`this`/
   `next <unit>`, `now`, …) instead of always rendering the numeric form.
   `format` and `formatToParts` share the path; a word-form result is a single
   `"literal"` part (no `unit` field), matching Node. Short/narrow styles
   abbreviate `week`/`year` (`wk.` / `yr.`). Values without a CLDR word form
   (including non-integers and `|value| > 1` for most units) still use the
   numeric form; `numeric: "always"` is unchanged.

2. `value instanceof Intl.<Ctor>` is true for `class X extends Intl.<Ctor>`
   subclass instances. `intl_subclass_super` already copied the constructor's
   `__intlKind` brand onto `this`, but `intl_instanceof` only walked the
   static-prototype side table — which never linked `X.prototype` to
   `Intl.<Ctor>.prototype` because Intl constructors are closures without a
   class-id registry edge. The probe now brand-matches first (same shape as
   Temporal's brand-cell arm) and still falls through to a real
   `getPrototypeOf` walk for direct instances.

Regression: `test-files/test_gap_intl_rtf_auto_instanceof_6960.ts` (byte-for-
byte vs Node 26.5.0).
