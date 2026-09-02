// #9449: `new Date("2026-09-01T10:30")` was parsed as UTC. ECMA-262 §21.4.3.2
// splits on whether a TIME is present: a date-ONLY form with no offset is UTC,
// a date-TIME form with no offset is LOCAL. Perry applied UTC to both, so every
// `<input type="datetime-local">` value, log line and hand-written timestamp
// was silently off by the host's UTC offset.
//
// Fixture discipline (carried over from the #9414 slash-date fixture): a raw
// epoch — or a delta against any fixed reference — couples the expected bytes
// to the host zone's DST regime at each row's date. So:
//   * rows that carry a zone designator (`Z`, `+HH:MM`, `-HH:MM`) and the
//     date-ONLY rows are absolute instants: assert `toISOString()`.
//   * offsetless date-TIME rows are wall-clock: assert the LOCAL getters
//     (which read back the very digits that were written, in any zone) and
//     compare the instant against a locally-constructed reference `Date` by
//     equality.
// Every expectation is measured against `node --experimental-strip-types`.

// ---- absolute rows: a zone designator, or no time at all -------------------
function iso(input: string): void {
  const d = new Date(input);
  console.log(input, "=>", Number.isNaN(d.getTime()) ? "Invalid Date" : d.toISOString());
}

// Date-only forms have no time component, so they stay UTC. THIS IS THE HALF
// THAT MUST NOT MOVE.
iso("2026-09-01");
iso("2026-01-15");
iso("2026-09");
iso("2026");
iso("+002026-09-01");
iso("-000001-07-01");

// An explicit designator wins in the date-time form, exactly as before.
iso("2026-09-01T10:30Z");
iso("2026-09-01T10:30:45Z");
iso("2026-09-01T10:30:45.123Z");
iso("2026-09-01 10:30Z");
iso("2026-09-01T10:30+05:00");
iso("2026-09-01T10:30-08:00");
iso("2026-09-01 10:30+05:00");
iso("2026-09-01 10:30-08:00");
iso("2026-01-15T10:30Z");
iso("2026-09-01T00:00Z");

// A trailing, whitespace-separated `GMT` / `UTC` / `UT` / `Z` word is node's
// designator in the space-separated spelling. These rows are the ones this
// change could most easily have BROKEN: they were UTC before it only because
// everything was UTC, so "no designator => local" has to recognise the word
// rather than ignore it.
iso("2026-09-01 10:30 GMT");
iso("2026-09-01 10:30 UTC");
iso("2026-09-01 10:30 UT");
iso("2026-09-01 10:30 Z");
iso("2026-09-01 10:30 gmt");
iso("2026-09-01 10:30 z");
iso("2026-09-01 10:30:45 GMT");
iso("2026-09-01 10:30:45.123 UTC");
iso("2026-09-01 10:30 GMT+0500");
iso("2026-09-01 10:30 GMT+05:00");
iso("2026-09-01 10:30 +0500");
iso("2026-09-01 10:30:45 +05:00");

// ---- wall-clock rows: a time, no designator => LOCAL -----------------------
function local(input: string): void {
  const d = new Date(input);
  if (Number.isNaN(d.getTime())) {
    console.log(input, "=> Invalid Date");
    return;
  }
  console.log(
    input,
    "=>",
    d.getFullYear(),
    d.getMonth(),
    d.getDate(),
    d.getHours(),
    d.getMinutes(),
    d.getSeconds(),
    d.getMilliseconds(),
  );
}

local("2026-09-01T10:30");
local("2026-09-01T10:30:45");
local("2026-09-01T10:30:45.123");
local("2026-09-01T10:30:45.1");
local("2026-09-01T10:30:45.123456");
local("2026-09-01T00:00");
local("2026-09-01T24:00");
local("2026-12-31T23:59:59.999");
// The space separator (the MySQL spelling) behaves exactly like `T` in node.
local("2026-09-01 10:30");
local("2026-09-01 10:30:45");
local("2026-09-01 10:30:45.123");
local("2026-09-01 00:00");
// A January row and a July row: if the conversion used a FIXED offset rather
// than the offset in effect at that instant, one of these two would be wrong
// in any zone that observes DST.
local("2026-01-15T10:30");
local("2026-07-15T10:30");
// The expanded-year spelling takes the same branch.
local("+002026-09-01T10:30");
// A trailing parenthesised comment is not a designator, so these stay local.
local("2026-09-01 10:30 (comment)");

// ---- the instant itself, compared to a locally-constructed reference -------
// `new Date(y, m, d, h, ...)` is defined on LOCAL components, so these
// equalities hold in every host zone if and only if the parse is local too.
function sameInstant(input: string, ref: Date): void {
  console.log(input, "==", new Date(input).getTime() === ref.getTime());
}

sameInstant("2026-09-01T10:30", new Date(2026, 8, 1, 10, 30, 0, 0));
sameInstant("2026-09-01T10:30:45", new Date(2026, 8, 1, 10, 30, 45, 0));
sameInstant("2026-09-01T10:30:45.123", new Date(2026, 8, 1, 10, 30, 45, 123));
sameInstant("2026-09-01 10:30", new Date(2026, 8, 1, 10, 30, 0, 0));
sameInstant("2026-09-01 10:30:45.123", new Date(2026, 8, 1, 10, 30, 45, 123));
sameInstant("2026-09-01T00:00", new Date(2026, 8, 1, 0, 0, 0, 0));
sameInstant("2026-09-01T24:00", new Date(2026, 8, 2, 0, 0, 0, 0));
sameInstant("2026-01-15T10:30", new Date(2026, 0, 15, 10, 30, 0, 0));
sameInstant("2026-07-15T10:30", new Date(2026, 6, 15, 10, 30, 0, 0));
// And the date-only form is NOT the local midnight of that day (except in a
// UTC host); it is the UTC midnight, which `Date.UTC` spells.
console.log(
  "date-only is UTC:",
  new Date("2026-09-01").getTime() === Date.UTC(2026, 8, 1, 0, 0, 0, 0),
);

// ---- Date.parse is the same grammar ---------------------------------------
for (
  const s of [
    "2026-09-01T10:30",
    "2026-09-01 10:30:45.123",
    "2026-09-01",
    "2026-09-01T10:30Z",
    "2026-09-01T10:30-08:00",
  ]
) {
  console.log("parse==new", s, Date.parse(s) === new Date(s).getTime());
}
console.log(Number.isNaN(Date.parse("2026-09-01T10")));
console.log(Number.isNaN(Date.parse("2026-09-01Tnope")));

// ---- controls: the neighbouring grammars must not move ---------------------
// The slash grammar (#9414) is already local and is guarded by its own
// fixture; these rows only pin that this change did not reach it.
console.log(new Date("2026/09/01").getTime() === new Date(2026, 8, 1, 0, 0, 0, 0).getTime());
console.log(new Date("2026/09/01 10:30").getTime() === new Date(2026, 8, 1, 10, 30).getTime());
console.log(new Date("2026/09/01 10:30 UTC").toISOString());
// RFC-1123 / month-name forms.
console.log(new Date("Thu, 01 Jan 1970 00:00:00 GMT").toISOString());
console.log(new Date("01 Jan 1970 00:00:00 GMT").toISOString());
console.log(new Date("March 7, 2020").getTime() === new Date(2020, 2, 7, 0, 0, 0, 0).getTime());
console.log(String(new Date("not a date")));
