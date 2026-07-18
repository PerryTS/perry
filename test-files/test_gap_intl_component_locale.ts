// Component-based Intl.DateTimeFormat (explicit year/month/day/weekday with a
// spelled `month` or a `weekday`) must localize the field names AND the field
// order — `5. Januar 2026`, `2026年1月5日`, `lundi 5 janvier` — not the old
// US-hardcoded `January 5, 2026`. Perry routes these name-bearing combos
// through icu4x's dynamic FieldSetBuilder; numeric-only combos keep the
// existing assembly and aren't asserted here. Both entry points are covered:
// `Intl.DateTimeFormat(...).format()` and `Date.prototype.toLocaleDateString`.
//
// Compared byte-for-byte against `node --experimental-strip-types`.
const d = new Date(Date.UTC(2026, 0, 5, 14, 37, 9));

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
