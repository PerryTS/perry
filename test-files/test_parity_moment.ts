// Behavioral parity test for the moment package (perry-stdlib).
//
// Fixed UTC anchor so all output is deterministic; .now()/fromNow() are
// time-dependent so they're shape-checked only.

import moment from "moment";

const ANCHOR_MS = 1709209096789; // 2024-02-29T12:34:56.789Z

const m = moment.utc(ANCHOR_MS);

// ── Parse / construct ──
console.log("isValid:", m.isValid());
console.log("isValid_bad:", moment("not-a-date", moment.ISO_8601, true).isValid());
console.log("valueOf:", m.valueOf());
console.log("unix:", m.unix());
console.log("toISOString:", m.toISOString());

// ── Format ──
console.log("format default:", m.format());
console.log("format YYYY-MM-DD:", m.format("YYYY-MM-DD"));
console.log("format HH:mm:ss:", m.format("HH:mm:ss"));

// ── Field accessors ──
console.log("year:", m.year());
console.log("month:", m.month());
console.log("date:", m.date());
console.log("day:", m.day());
console.log("hour:", m.hour());
console.log("minute:", m.minute());
console.log("second:", m.second());
console.log("millisecond:", m.millisecond());

// ── Arithmetic (clone first — moment mutates) ──
console.log("add 1 day iso:", m.clone().add(1, "day").toISOString());
console.log("sub 1 month iso:", m.clone().subtract(1, "month").toISOString());

// ── Boundaries ──
console.log("startOf day:", m.clone().startOf("day").toISOString());
console.log("endOf day:", m.clone().endOf("day").toISOString());

// ── Comparisons ──
const later = moment.utc(ANCHOR_MS + 86_400_000);
console.log("isBefore:", m.isBefore(later));
console.log("isAfter:", later.isAfter(m));
console.log("isSame:", m.isSame(moment.utc(ANCHOR_MS)));
console.log("isBetween:", m.isBetween(moment.utc(ANCHOR_MS - 1), later));
console.log("diff ms:", later.diff(m));
console.log("diff days:", later.diff(m, "days"));

// fromNow is time-dependent — shape check only.
console.log("fromNow is string:", typeof m.fromNow() === "string");

// toDate / clone
const native = m.toDate();
console.log("toDate ms:", native.getTime());
const cloned = m.clone();
console.log("clone iso:", cloned.toISOString());

/*
@covers
crates/perry-stdlib/src/moment.rs:
  - js_moment_add
  - js_moment_clone
  - js_moment_date
  - js_moment_day
  - js_moment_diff
  - js_moment_end_of
  - js_moment_format
  - js_moment_from_now
  - js_moment_from_timestamp
  - js_moment_hour
  - js_moment_is_after
  - js_moment_is_before
  - js_moment_is_between
  - js_moment_is_same
  - js_moment_is_valid
  - js_moment_millisecond
  - js_moment_minute
  - js_moment_month
  - js_moment_now
  - js_moment_parse
  - js_moment_second
  - js_moment_start_of
  - js_moment_subtract
  - js_moment_to_date
  - js_moment_to_iso_string
  - js_moment_unix
  - js_moment_value_of
  - js_moment_year
*/
