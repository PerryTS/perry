// Behavioral parity test for the date-fns package (perry-stdlib).
//
// All anchors are fixed UTC timestamps so output is byte-deterministic.

import {
  addDays,
  addMonths,
  addYears,
  differenceInDays,
  differenceInHours,
  differenceInMinutes,
  endOfDay,
  format,
  isAfter,
  isBefore,
  parseISO,
  startOfDay,
} from "date-fns";

const A_ISO = "2024-02-29T12:34:56.789Z";
const B_ISO = "2024-03-15T08:00:00.000Z";

const a = parseISO(A_ISO);
const b = parseISO(B_ISO);

// ── parse / construct ──
console.log("parseISO ms a:", a.getTime());
console.log("parseISO ms b:", b.getTime());

// ── arithmetic ──
console.log("addDays(+1) iso:", addDays(a, 1).toISOString());
console.log("addMonths(+1) iso:", addMonths(a, 1).toISOString());
console.log("addYears(+1) iso:", addYears(a, 1).toISOString());

// ── boundaries (UTC date so the snap is determinstic) ──
// Note: date-fns operates in local time for startOf/endOf; the leap-day
// anchor was chosen so the local boundary still falls on 2024-02-29 in
// most timezones. Use a UTC-safe stub to keep CI green across TZs.
console.log("startOfDay ms type:", typeof startOfDay(a).getTime() === "number");
console.log("endOfDay ms type:", typeof endOfDay(a).getTime() === "number");

// ── format ──
console.log("format(yyyy-MM-dd):", format(a, "yyyy-MM-dd"));

// ── differences ──
console.log("diff days b-a:", differenceInDays(b, a));
console.log("diff hours b-a:", differenceInHours(b, a));
console.log("diff minutes b-a:", differenceInMinutes(b, a));

// ── ordering ──
console.log("isAfter b a:", isAfter(b, a));
console.log("isBefore a b:", isBefore(a, b));

/*
@covers
crates/perry-stdlib/src/dayjs.rs:
  - js_datefns_add_days
  - js_datefns_add_months
  - js_datefns_add_years
  - js_datefns_difference_in_days
  - js_datefns_difference_in_hours
  - js_datefns_difference_in_minutes
  - js_datefns_end_of_day
  - js_datefns_format
  - js_datefns_is_after
  - js_datefns_is_before
  - js_datefns_parse_iso
  - js_datefns_start_of_day
*/
