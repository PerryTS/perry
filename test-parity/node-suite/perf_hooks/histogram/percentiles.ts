import { createHistogram } from "node:perf_hooks";
const h = createHistogram();
for (const n of [1, 2, 4, 8]) h.record(n);
console.log(
  "endpoints:",
  h.percentile(1) === h.min,
  h.percentile(100) === h.max,
);
console.log(
  "bigint endpoints:",
  h.percentileBigInt(1) === h.minBigInt,
  h.percentileBigInt(100) === h.maxBigInt,
);
console.log("ordered:", h.percentile(25) <= h.percentile(75));
console.log("types:", typeof h.percentile(50), typeof h.percentileBigInt(50));
