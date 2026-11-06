// Component-based Intl.DateTimeFormat (explicit year/month/day/weekday with a
// spelled `month` or a `weekday`) must localize the field names AND the field
// order — `5. Januar 2026`, `2026年1月5日`, `lundi 5 janvier` — not the old
// US-hardcoded `January 5, 2026`. Perry routes these name-bearing combos
// through icu4x's dynamic FieldSetBuilder. #9451 extends the same CLDR route
// to the default numeric Y/M/D field set, including semantic parts. Both entry
// points are covered:
// `Intl.DateTimeFormat(...).format()` and `Date.prototype.toLocaleDateString`.
//
// Compared byte-for-byte against `node --experimental-strip-types`.
const d = new Date(Date.UTC(2026, 0, 5, 14, 37, 9));

// The ECMA-402 no-fields default is numeric year/month/day. Cover both sides
// of the padding boundary: January 5 has single-digit month/day, while
// November 15 has double-digit month/day.
const defaultDates = [
  new Date(Date.UTC(2026, 0, 5, 14, 37, 9)),
  new Date(Date.UTC(2026, 10, 15, 14, 37, 9)),
];
for (const loc of ["de-DE", "fr-FR", "ja-JP", "en-GB", "en-US"]) {
  for (const date of defaultDates) {
    const dtf = new Intl.DateTimeFormat(loc, { timeZone: "UTC" });
    console.log(loc + " | default | " + dtf.format(date));
    console.log(loc + " | parts   | " + JSON.stringify(dtf.formatToParts(date)));
    console.log(
      loc + " | method  | " + date.toLocaleDateString(loc, { timeZone: "UTC" }),
    );
  }
}

// Controls: style-driven requests already use ICU's CLDR patterns and must
// remain unchanged when the numeric component path is enabled.
const styleControls: Array<[string, Opt]> = [
  ["de-DE", { dateStyle: "short" }],
  ["fr-FR", { dateStyle: "long" }],
  ["ja-JP", { timeStyle: "short" }],
  ["en-GB", { dateStyle: "medium", timeStyle: "short" }],
  ["en-US", { dateStyle: "full" }],
];
for (const [loc, opt] of styleControls) {
  console.log(
    loc +
      " | style   | " +
      new Intl.DateTimeFormat(loc, { ...opt, timeZone: "UTC" }).format(d),
  );
}

type Opt = Intl.DateTimeFormatOptions;
const cases: Array<[string, Opt]> = [
  ["de", { year: "numeric", month: "long", day: "numeric" }],
  ["de", { month: "long", day: "numeric" }],
  ["en-US", { month: "short", day: "numeric" }],
  ["en-US", { weekday: "long", year: "numeric", month: "long", day: "numeric" }],
  ["en-GB", { weekday: "short", month: "short", day: "numeric" }],
  ["fr", { weekday: "long", day: "numeric", month: "long" }],
  ["it", { year: "numeric", month: "long", day: "numeric" }],
  ["pt", { day: "numeric", month: "long", year: "numeric" }],
  ["ja", { year: "numeric", month: "long", day: "numeric" }],
  ["ko", { year: "numeric", month: "long", day: "numeric" }],
  ["zh-Hans", { year: "numeric", month: "long", day: "numeric" }],
  ["ru", { day: "numeric", month: "long", year: "numeric" }],
];

for (const [loc, opt] of cases) {
  const viaDtf = new Intl.DateTimeFormat(loc, { ...opt, timeZone: "UTC" }).format(d);
  const viaMethod = d.toLocaleDateString(loc, { ...opt, timeZone: "UTC" });
  console.log(loc + " | dtf    | " + viaDtf);
  console.log(loc + " | method | " + viaMethod);
}
