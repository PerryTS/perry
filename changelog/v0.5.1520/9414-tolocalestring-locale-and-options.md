### Fixed

- **`Number.prototype.toLocaleString` no longer discards its locale and its
  options bag.** `(1234.5).toLocaleString("de-DE")` printed the en-US default
  `1,234.5` instead of node's `1.234,5`, `(0.5).toLocaleString("en-US",
  { style: "percent" })` printed `0.5` instead of `50%`, and
  `(1e6).toLocaleString("en-US", { notation: "compact" })` printed
  `1,000,000` instead of `1M`. There are 28 `toLocaleString` sites in the
  claude-code bundle, so this was user-visible.

  This was **not** a missing feature. Perry has a real ECMA-402
  `Intl.NumberFormat` — `new Intl.NumberFormat("de-DE").format(1234.5)` already
  produced node's bytes — and ECMA-402 defines
  `Number.prototype.toLocaleString(locales, options)` as nothing more than
  "construct an `Intl.NumberFormat` with exactly these arguments and
  FormatNumeric the receiver with it". The arguments simply never got there.
  They were dropped twice on the way:

  - `native_call_method/common_methods.rs` answered **every** `toLocaleString`
    call — arguments or not — with `js_object_default_to_locale_string`, a
    helper that takes no arguments at all. `BigInt` already had a carve-out
    here (#5845) for exactly this reason; a number did not.
  - `object/primitive_proto_thunks.rs`'s
    `number_proto_to_locale_string_thunk`, the method that arm was shadowing,
    was itself declared `(closure)` with no parameters and called the
    hand-rolled en-US grouping helper unconditionally.

  Both are fixed: a number receiver carrying an argument now falls through to
  the prototype thunk (which also makes a user override of
  `Number.prototype.toLocaleString` reachable), and the thunk is installed
  rest-based so `(locales, options)` arrive and are handed to a real
  `Intl.NumberFormat`.

  This is also what made `Array.prototype.toLocaleString` look broken.
  `js_array_to_locale_string` had been forwarding `(locales, options)` to each
  element correctly all along; the arguments died one level below it, in the
  element's own `toLocaleString`. `[0.5, 0.25].toLocaleString("en-US",
  { style: "percent" })` is now node's `50%,25%` with no change to the array
  code.

  **The no-argument path is untouched and still free.** `(1234.5)
  .toLocaleString()` never reaches the thunk at all — codegen folds the
  zero-arg form to an inline `js_number_to_locale_string` call — and the
  explicit `toLocaleString(undefined, undefined)` spelling is the same request,
  so it takes the same branch rather than paying for a NumberFormat
  construction. That matters because Intl has **no formatter cache**: every
  argument-bearing call builds one instance, exactly as the spec describes.

- **`Date.prototype.toLocale{,Date,Time}String` now honors the locale for
  `dateStyle` / `timeStyle`.** `d.toLocaleDateString("de-DE",
  { dateStyle: "long" })` printed `September 1, 2026` — an English month name
  in a German locale — and `d.toLocaleString("ja-JP", { dateStyle: "full",
  timeStyle: "short" })` printed the en-US rendering.

  `Intl.DateTimeFormat.prototype.format` (`format_ms_with_dtf_obj`) had already
  been moved onto icu4x's CLDR patterns for these two options.
  `temporal_locale_string` — the *other* spelling of the same operation, and the
  one `Date.prototype.toLocale*String` delegates to — was left behind on the
  bespoke `format_date_style` / `format_time_style` pair, which hard-codes the
  en-US layout and the English month/weekday tables. The same instant with the
  same options therefore formatted differently depending on which spelling was
  used. The style arms now go through the same `icu_style`, keeping the bespoke
  pair as the fallback for the combinations icu declines (a `long`/`full`
  timeStyle carries a localized time-zone name) and for the Temporal partials
  that own their own layout.

  Affected files:

  - `crates/perry-runtime/src/object/native_call_method/common_methods.rs`
  - `crates/perry-runtime/src/object/primitive_proto_thunks.rs`
  - `crates/perry-runtime/src/intl/number_format.rs` — new
    `number_to_locale_string`, the same `make_instance` +
    `format_number_instance` pair `bigint_to_locale_string` uses.
  - `crates/perry-runtime/src/intl.rs`
  - `crates/perry-runtime/src/intl/date_collator/temporal.rs`

  Validation: `test-files/test_gap_tolocalestring_locale_options_9414.ts`
  byte-compared against node 26.5.1 — de-DE / fr-FR / ja-JP / en-US and an
  unknown tag; `style` percent and currency; `notation` compact short and long;
  min/max fraction digits, `minimumIntegerDigits` and `useGrouping`; an
  `undefined` locale with an options bag and an empty locale list; the
  `Date` family with `dateStyle` / `timeStyle` / explicit field options and a
  `timeZone`; `Array.prototype.toLocaleString` over numbers and dates; the
  `Intl.NumberFormat` / `Intl.DateTimeFormat` rows that pin the delegation
  target; and the no-argument calls as controls. Before the change 26 of its 64
  lines diverged from node; after it, none.

  Two pre-existing `Intl` gaps this delegation now exposes are deliberately NOT
  pinned by that fixture, because each is wrong standalone — the fixture's own
  `Intl.*` control rows prove it — and neither is a routing defect:

  - `Intl.NumberFormat` groups in fixed 3-digit runs, so `en-IN` gives
    `1,234,567.891` where node gives `12,34,567.891`.
  - A purely NUMERIC field set — which is the ECMA-402 *default* for
    `Intl.DateTimeFormat` and for a bare `toLocaleDateString(locale)` — is
    deliberately declined by `icu_dtf::format_components` (icu's `Short` length
    pads and truncates: `05.01.26`, not node's `5.1.2026`), and the caller's
    fallback assembly is hard-coded `M/D/YYYY` + `h:mm:ss AM/PM`. So
    `new Intl.DateTimeFormat("de-DE").format(new Date(0))` is `1/1/1970`
    instead of `1.1.1970`. icu4x 2.2's `FieldSetBuilder` exposes `alignment`
    and `year_style`, which look like the right knobs (`Alignment::Auto` +
    `YearStyle::Full` on a `Short` `YMD`) — that is the follow-up.
