// #5582 — Intl.DateTimeFormat option validation for a `null` option value.
//
// GetOption (ECMA-402) treats ONLY `undefined` as "absent → fallback"; every
// other value, `null` included, is coerced with ToString. So:
//   * an enum option (localeMatcher, weekday, hourCycle, dateStyle, …) sees the
//     string "null", which no allow-list accepts → RangeError; and
//   * a Unicode locale-extension key (calendar, numberingSystem) sees "null",
//     which is a well-formed but unsupported `type` subtag, so ResolveLocale
//     drops it and resolvedOptions reports the locale default (gregory / latn).
// Both branches must match `node --experimental-strip-types` byte-for-byte.
function probe(opt: string): string {
  try {
    const options: Record<string, unknown> = {};
    options[opt] = null;
    new Intl.DateTimeFormat(undefined, options as Intl.DateTimeFormatOptions);
    return "accepted";
  } catch (e) {
    return (e as Error).constructor.name;
  }
}

for (const opt of ["localeMatcher", "formatMatcher", "weekday", "hourCycle", "dateStyle"]) {
  console.log(opt, probe(opt));
}

// Locale-extension keys fall back to the locale default rather than throwing.
const ro = new Intl.DateTimeFormat("en", {
  calendar: null as unknown as string,
  numberingSystem: null as unknown as string,
}).resolvedOptions();
console.log("calendar", ro.calendar);
console.log("numberingSystem", ro.numberingSystem);
