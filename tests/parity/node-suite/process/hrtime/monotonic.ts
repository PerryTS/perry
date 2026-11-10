// process.hrtime.bigint() is monotonic: a subsequent reading is >= the prior.
const a = process.hrtime.bigint();
const b = process.hrtime.bigint();
console.log("monotonic:", b >= a);
