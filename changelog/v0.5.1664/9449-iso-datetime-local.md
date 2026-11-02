### Fixed

- **`new Date("2026-09-01T10:30")` was parsed as UTC; node and ECMA-262 read an
  offsetless date-TIME as LOCAL wall-clock time.** In a UTC+2 host the string
  became `2026-09-01T10:30:00.000Z` instead of node's
  `2026-09-01T08:30:00.000Z` — silently off by the host's UTC offset, with no
  error and no diagnostic. `"YYYY-MM-DDTHH:mm"` is what every
  `<input type="datetime-local">` value, most log lines and most hand-written
  timestamps look like, so the wrong instant was plausible enough to survive
  review and it differed between a developer's machine and a UTC server.

  ECMA-262 §21.4.3.2 splits the default zone on whether a TIME is present:

  - a date-**only** form with no offset (`"2026-09-01"`) is **UTC**;
  - a date-**time** form with no offset (`"2026-09-01T10:30"`) is **LOCAL**.

  Perry applied UTC to both. The doc comment on `parse_date_string` asserted
  exactly that — *"date-time forms without an offset are also treated as UTC
  (matching V8's ISO handling)"* — which is wrong against both node and the
  spec, and is the reason the behaviour looked deliberate. It is corrected
  with the code.

  The date-only half was already right and is unchanged; the fixture pins it
  so "make everything local" can never be the fix. An explicit `Z` /
  `±HH:MM` designator still wins in both forms.

  Affected files:

  - `crates/perry-runtime/src/date/parse.rs` — `parse_iso8601` now records
    whether a time component was read and, with no designator, converts a
    date-time through the host's UTC offset at that instant; the doc comment
    is corrected. The conversion the RFC/month-name and numeric-slash
    grammars already performed inline is lifted into one
    `local_wall_clock_to_utc` helper that all three now share, so the three
    grammars cannot drift apart again.

  The space-separated MySQL spelling (`"2026-09-01 10:30"`) takes the same
  branch, which is what node does — measured, including the seconds and
  fractional-second variants. The numeric slash grammar from #9414 was
  already local and is byte-for-byte unchanged; its fixture
  (`test_gap_date_parse_slash_9414.ts`) is the regression guard and still
  passes.

  Validation: `test-files/test_gap_date_iso_datetime_local_9449.ts` is 68 lines
  byte-identical to node 26.5.1, of which **25 diverged** on a compiler
  built from unfixed `origin/main`. It covers date-only (must stay UTC),
  `T10:30`, `T10:30:45`, `T10:30:45.123`, over-long and short fractions, the
  space spelling, midnight `T00:00`, the rolling `T24:00`, a January and a
  July row (a fixed offset instead of the offset at that instant would break
  one of them in any DST zone), the expanded-year spelling, `Z`, `+05:00`,
  `-08:00`, `Date.parse` agreeing with `new Date`, and controls for the
  slash, RFC-1123 and month-name grammars, plus twelve rows for the trailing
  `GMT` / `UTC` / `UT` / `Z` designator word — the rows this change could
  most easily have broken, since they were UTC before it only because
  everything was. Offsetless rows assert the LOCAL getters and an equality
  against a locally-constructed reference `Date`, so the expected bytes do
  not depend on the runner's zone.

  `cargo test --release -p perry-runtime --lib`: 2973 passed, 2 failed. Both
  failures are `gc::tests::handle_bound_method_name::*`, which assert
  `&'static` literal POINTER IDENTITY; they reproduce identically on
  unmodified `origin/main` sources under the same local build profile
  (`CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`, which lets the same read-only
  literal be emitted into two codegen units) and are unrelated to dates.
