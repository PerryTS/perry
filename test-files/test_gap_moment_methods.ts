// Gap test: moment instance methods (format/add/subtract/diff/field
// accessors/predicates) must dispatch — only the factory used to be
// wired, so m.format() returned undefined. Run with TZ=UTC on both
// sides. moment mutates on add/subtract, so arithmetic always goes
// through an explicit clone binding and the original is only read
// before/independently of the mutation.

import moment from "moment";

const m = moment("2024-01-15");
console.log(m.format("YYYY-MM-DD"));
console.log(m.year(), m.month(), m.date(), m.day());
console.log(m.valueOf(), m.unix());
console.log(m.isValid() ? "valid" : "invalid");

const m2 = moment("2024-03-05T06:07:08");
console.log(m2.format("YYYY-MM-DD HH:mm:ss"));
console.log(m2.hour(), m2.minute(), m2.second());

const epoch = moment(1700000000000);
console.log(epoch.valueOf());
console.log(epoch.format("YYYY-MM-DD HH:mm:ss"));

// Arithmetic via clone (moment's add/subtract mutate the receiver).
const mc = m.clone();
const plus = mc.add(7, "days");
console.log(plus.format("YYYY-MM-DD"));

const m2c = m2.clone();
const minus = m2c.subtract(2, "hours");
console.log(minus.format("YYYY-MM-DD HH:mm:ss"));

// startOf on a clone.
const m2d = m2.clone();
const sod = m2d.startOf("day");
console.log(sod.format("YYYY-MM-DD HH:mm:ss"));

// Comparisons / diff (m unchanged: all mutations went through clones).
console.log(m.isBefore(plus) ? "before" : "not-before");
console.log(plus.diff(m, "days"));
console.log(plus.isAfter(m) ? "after" : "not-after");
