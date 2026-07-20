// Gap test: the dayjs factory must parse its argument (ISO strings and
// epoch milliseconds) instead of always returning "now".
//
// Every assertion here is timezone-independent, so the test is
// deterministic on any host: wall-clock inputs are only ever observed as
// wall-clock (format / component getters), and instants are only ever
// asserted from offset-bearing input or epoch numbers. Mixing the two —
// e.g. `dayjs("2024-01-15").valueOf()` — is deliberately avoided: dayjs
// reads offset-less input in the host timezone, while Perry's bundled
// dayjs binding (perry-stdlib/src/dayjs.rs, the `bundled-dayjs` feature)
// is UTC-based throughout. That mismatch is a separate known gap; pinning
// it here would make the test pass only under TZ=UTC.

import dayjs from "dayjs";

// Bare date string — the headline bug: this used to return "now".
const d = dayjs("2024-01-15");
console.log(d.format("YYYY-MM-DD"));
console.log(d.year(), d.month(), d.date(), d.day());

// Offset-less ISO datetime, observed as wall-clock.
const dt = dayjs("2024-03-05T06:07:08");
console.log(dt.format("YYYY-MM-DD HH:mm:ss"));
console.log(dt.hour(), dt.minute(), dt.second());

// Offset-bearing input pins a real instant in every timezone.
// NB: the receiver is bound first — `dayjs(x).valueOf()` chained inline
// off the factory call currently yields the raw handle instead of the
// epoch (a separate `.valueOf()`-fold gap; `.format()` is unaffected).
const fixed = dayjs("2024-01-15T00:00:00Z");
console.log(fixed.valueOf());

// Epoch milliseconds round-trip.
const epoch = dayjs(1700000000000);
console.log(epoch.valueOf());

// Arithmetic on parsed dates (dayjs is immutable — no clone needed).
const plus = d.add(7, "day");
console.log(plus.format("YYYY-MM-DD"));
const minus = dt.subtract(2, "hour");
console.log(minus.format("YYYY-MM-DD HH:mm:ss"));

// Comparisons anchored on parsed values.
console.log(d.isBefore(plus) ? "before" : "not-before");
console.log(plus.diff(d, "day"));
