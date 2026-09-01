### Fixed

- **`new Date("2026/09/01")` is no longer Invalid Date.** The numeric
  slash-separated forms node accepts — `"2026/09/01"`, `"2026/9/1"`,
  `"09/01/2026"` — all produced `NaN` in Perry. Every other date format tested
  against node already matched, so this was narrowly the
  implementation-defined-format branch of `Date.parse` / `new Date(string)`.

  ECMA-262 §21.4.3.2 deliberately leaves this format to the implementation, so
  the new branch reproduces **V8's measured behaviour**, not a reading of the
  spec. `parse_date_string` had exactly two grammars — ISO 8601 / MySQL and
  RFC-1123 / month-name — and the second one requires a spelled month
  (`let m = month?;`), so a purely numeric input fell out of both and returned
  `NaN`.

  The subtle half is not the acceptance, it is the **time zone**: unlike the ISO
  branch, which is UTC, these components are LOCAL wall-clock time, so
  `new Date("2026/09/01").getHours()` is `0` everywhere and the epoch value
  differs per host. Getting that backwards would have looked like a working fix
  in one time zone.

  Behaviours reproduced from node (all measured, none assumed):

  - Three numeric components are collected in order and padded with `1`. If the
    FIRST is not a valid day-of-month (`1..=31`) the triple is Y/M/D, otherwise
    it is US M/D/Y — which is what makes `"2026/09/01"` year-first and
    `"09/01/2026"` month-first with no lookahead, and what makes `"31/1/2026"`
    Invalid (31 read as a month) while `"12/1/2026"` is 1 December.
  - Two-digit years: `0..=49` → 2000s, `50..=99` → 1900s. `"09/01/26"` is 2026,
    `"99/1/1"` is 1999, `"1/1/100"` is literally year 100.
  - The month must be `1..=12` and the day `1..=31`, but a day past the end of
    its month ROLLS OVER instead of failing: `"2026/02/30"` is 2 March 2026 and
    `"2026/09/31"` is 1 October. `"2026/13/01"`, `"2026/09/00"` and
    `"2026/09/32"` are Invalid Date.
  - An optional clock with `am`/`pm`, fractional seconds, and `GMT`/`UTC`/`Z`/
    `GMT±HHMM` zone designators. `24:00` rolls to the next midnight; `25:00` and
    `10:60` are Invalid. A bare `+0500` is a zone only AFTER a clock has been
    read, which is why node's `new Date("2026/09/01 +0500")` is Invalid Date
    while `"2026/09/01 10:30 +0500"` is not.
  - `T` is ISO-only: `"2026/09/01T10:30"` stays Invalid Date, as in node.

  Affected files:

  - `crates/perry-runtime/src/date/parse.rs` — new `parse_slash_date`, tried
    only after the two existing grammars and only when the input actually
    contains a `/`, so the ISO, MySQL, RFC-1123 and month-name paths are
    bit-for-bit unchanged.

  Validation: `test-files/test_gap_date_parse_slash_9414.ts` — 60 rows covering
  the three shapes, two-digit years, out-of-range and rolling-over components,
  clocks, meridiem, zone designators, `Date.parse`, and ISO/RFC controls —
  byte-compared against node 26.5.1. Before the change 41 of its lines diverged
  (every slash row read `Invalid Date`); after it the output is byte-identical.
  Host-zone independent by construction: local rows print the local getters plus
  a delta from a locally-constructed reference instant, zone-designated rows
  print `toISOString()`.
