// Gap test: the dayjs factory must parse its argument (ISO strings and
// epoch milliseconds) instead of always returning "now". Run with
// TZ=UTC on both sides — dayjs treats offset-less inputs as local time
// and Perry's date runtime is UTC-based.

import dayjs from "dayjs";

// Bare date string.
const d = dayjs("2024-01-15");
console.log(d.format("YYYY-MM-DD"));
console.log(d.year(), d.month(), d.date(), d.day());
console.log(d.valueOf());

// Offset-less ISO datetime.
const dt = dayjs("2024-03-05T06:07:08");
console.log(dt.format("YYYY-MM-DD HH:mm:ss"));
console.log(dt.hour(), dt.minute(), dt.second());

// Epoch milliseconds.
const epoch = dayjs(1700000000000);
console.log(epoch.valueOf());
console.log(epoch.format("YYYY-MM-DD HH:mm:ss"));

// Arithmetic on parsed dates (dayjs is immutable — no clone needed).
const plus = d.add(7, "day");
console.log(plus.format("YYYY-MM-DD"));
const minus = dt.subtract(2, "hour");
console.log(minus.format("YYYY-MM-DD HH:mm:ss"));

// Comparisons anchored on parsed values.
console.log(d.isBefore(plus) ? "before" : "not-before");
console.log(plus.diff(d, "day"));
