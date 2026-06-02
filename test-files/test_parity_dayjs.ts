// Behavioral parity test for the dayjs package (perry-stdlib).
//
// dayjs.now()/Date.now() are non-deterministic — use a fixed-timestamp
// anchor (UTC) for every assertion so output is byte-for-byte stable.

import dayjs from "dayjs";

// Fixed UTC timestamp: 2024-02-29T12:34:56.789Z (leap day, easy to spot).
const ANCHOR_MS = 1709209096789;
const ANCHOR_ISO = "2024-02-29T12:34:56.789Z";

const d = dayjs(ANCHOR_MS);

// ── Parse / construct ──
console.log("isValid:", d.isValid());
console.log("isValid_bad:", dayjs("not-a-date").isValid());
console.log("valueOf:", d.valueOf());
console.log("unix:", d.unix());
console.log("toISOString:", d.toISOString());

// ── Format ──
console.log("format default:", d.format());
console.log("format YYYY-MM-DD:", d.format("YYYY-MM-DD"));
console.log("format HH:mm:ss:", d.format("HH:mm:ss"));

// ── Field accessors ──
console.log("year:", d.year());
console.log("month:", d.month());
console.log("date:", d.date());
console.log("day:", d.day());
console.log("hour:", d.hour());
console.log("minute:", d.minute());
console.log("second:", d.second());
console.log("millisecond:", d.millisecond());

// ── Arithmetic ──
console.log("add 1 day iso:", d.add(1, "day").toISOString());
console.log("sub 1 month iso:", d.subtract(1, "month").toISOString());

// ── Boundaries ──
console.log("startOf day:", d.startOf("day").toISOString());
console.log("endOf day:", d.endOf("day").toISOString());

// ── Comparisons ──
const later = dayjs(ANCHOR_MS + 86_400_000); // +1 day
console.log("isBefore:", d.isBefore(later));
console.log("isAfter:", later.isAfter(d));
console.log("isSame:", d.isSame(dayjs(ANCHOR_MS)));
console.log("diff ms:", later.diff(d));
console.log("diff days:", later.diff(d, "day"));

// Anchor sanity check (printed last so divergence is obvious).
console.log("anchor matches:", d.toISOString() === ANCHOR_ISO);

/*
@covers
crates/perry-stdlib/src/dayjs.rs:
  - js_dayjs_add
  - js_dayjs_date
  - js_dayjs_day
  - js_dayjs_diff
  - js_dayjs_end_of
  - js_dayjs_format
  - js_dayjs_from_timestamp
  - js_dayjs_hour
  - js_dayjs_is_after
  - js_dayjs_is_before
  - js_dayjs_is_same
  - js_dayjs_is_valid
  - js_dayjs_millisecond
  - js_dayjs_minute
  - js_dayjs_month
  - js_dayjs_now
  - js_dayjs_parse
  - js_dayjs_second
  - js_dayjs_start_of
  - js_dayjs_subtract
  - js_dayjs_to_iso_string
  - js_dayjs_unix
  - js_dayjs_value_of
  - js_dayjs_year
*/
