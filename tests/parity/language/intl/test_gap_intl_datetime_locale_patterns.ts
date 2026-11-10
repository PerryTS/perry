// Intl.DateTimeFormat CLDR locale patterns — dateStyle/timeStyle across locales
// must match `node --experimental-strip-types` byte-for-byte. Perry backs these
// with icu4x's `icu_datetime` + bundled CLDR (feature `intl-datetime`), so the
// per-locale field order, separators (`.` vs `/` vs `年`), localized month and
// weekday names, and day-period markers all come straight from CLDR.
//
// Scope note: this covers the combinations Perry formats via icu directly —
// short/medium date+time, all date-only lengths, and short/medium time-only.
// `long`/`full` *timeStyle* additionally render a localized time-zone name and,
// in some locales, spelled-out clock units; those defer to a separate path and
// are intentionally not asserted here. `es` is also omitted: Node's newer CLDR
// leaves the short-time hour un-padded (`9:07`) where icu4x pads it (`09:07`) —
// a data-version skew, not a bug.
//
// A fixed UTC instant keeps output independent of the host time zone.
const d = new Date(Date.UTC(2026, 0, 5, 9, 7, 3));

const locales = [
  "en-US", "en-GB", "de", "fr", "it", "ja", "ko", "pt", "zh-Hans", "tr", "ru", "nl",
];

const combos: Array<[string, Intl.DateTimeFormatOptions]> = [
  ["short", { dateStyle: "short", timeStyle: "short" }],
  ["medium", { dateStyle: "medium", timeStyle: "medium" }],
  ["date-short", { dateStyle: "short" }],
  ["date-medium", { dateStyle: "medium" }],
  ["date-long", { dateStyle: "long" }],
  ["date-full", { dateStyle: "full" }],
  ["time-short", { timeStyle: "short" }],
  ["time-medium", { timeStyle: "medium" }],
];

for (const loc of locales) {
  for (const [name, opt] of combos) {
    const fmt = new Intl.DateTimeFormat(loc, { ...opt, timeZone: "UTC" });
    console.log(loc + " | " + name + " | " + fmt.format(d));
  }
}
