// Temporal.Now (#4689) parity. Wall-clock values are non-deterministic, so this
// asserts only invariants that hold on every run (and match Node byte-for-byte).
console.log(typeof Temporal);
console.log(typeof Temporal.Now);
console.log(typeof Temporal.Now.instant().epochMilliseconds === "number");
console.log(typeof Temporal.Now.instant().epochNanoseconds === "bigint");
console.log(typeof Temporal.Now.timeZoneId() === "string");
console.log(typeof Temporal.Now.plainDateISO().year === "number");
console.log(typeof Temporal.Now.plainTimeISO().hour === "number");
console.log(typeof Temporal.Now.plainDateTimeISO().month === "number");
console.log(typeof Temporal.Now.zonedDateTimeISO().epochMilliseconds === "number");
console.log(typeof Temporal.Now.plainDateISO("UTC").day === "number");
// A Now-derived PlainDate compared to itself is 0 (deterministic).
const a = Temporal.Now.plainDateISO("UTC");
console.log(Temporal.PlainDate.compare(a, a));
