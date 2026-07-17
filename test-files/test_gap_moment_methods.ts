// Gap test: moment instance methods (format/add/subtract/diff/field
// accessors/predicates) must dispatch — only the factory used to be
// wired, so m.format() returned undefined. moment mutates on
// add/subtract, so arithmetic always goes through an explicit clone
// binding and the original is only read before/independently of the
// mutation.
//
// Every assertion here is timezone-independent, so the test is
// deterministic on any host: wall-clock inputs are only ever observed as
// wall-clock (format / component getters), and instants are only ever
// asserted from offset-bearing input or epoch numbers. Mixing the two —
// e.g. `moment("2024-01-15").valueOf()` — is deliberately avoided: moment
// reads offset-less input in the host timezone, while Perry's bundled
// moment binding (perry-stdlib/src/moment.rs) is UTC-based throughout.
// That mismatch is a separate known gap; pinning it here would make the
// test pass only under TZ=UTC.

import moment from "moment";

const m = moment("2024-01-15");
console.log(m.format("YYYY-MM-DD"));
console.log(m.year(), m.month(), m.date(), m.day());
console.log(m.isValid() ? "valid" : "invalid");

const m2 = moment("2024-03-05T06:07:08");
console.log(m2.format("YYYY-MM-DD HH:mm:ss"));
console.log(m2.hour(), m2.minute(), m2.second());

// Offset-bearing input pins a real instant in every timezone.
const fixed = moment("2024-01-15T00:00:00Z");
console.log(fixed.valueOf(), fixed.unix());

const epoch = moment(1700000000000);
console.log(epoch.valueOf());

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
