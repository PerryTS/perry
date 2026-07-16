import { createHistogram } from "node:perf_hooks";
const h = createHistogram();
h.record(5);
h.record(9n);
console.log("count pair:", h.count, h.countBigInt === 2n);
console.log("min pair:", h.min, h.minBigInt === 5n);
console.log("max pair:", h.max, h.maxBigInt === 9n);
console.log("mean range:", h.mean >= h.min && h.mean <= h.max);
console.log("stddev:", h.stddev >= 0);
