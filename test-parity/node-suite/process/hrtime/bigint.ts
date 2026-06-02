// process.hrtime.bigint() returns a BigInt nanosecond counter; the diff is
// non-negative.
const a = process.hrtime.bigint();
console.log("is bigint:", typeof a === "bigint");
const b = process.hrtime.bigint();
console.log("non-negative diff:", b - a >= 0n);
