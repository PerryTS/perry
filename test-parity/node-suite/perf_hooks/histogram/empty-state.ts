import { createHistogram } from "node:perf_hooks";
const h = createHistogram();
console.log("counts:", h.count, h.countBigInt === 0n);
console.log(
  "bounds:",
  h.minBigInt === 9223372036854775807n,
  h.maxBigInt === 0n,
);
console.log("exceeds:", h.exceeds, h.exceedsBigInt === 0n);
console.log("nan stats:", Number.isNaN(h.mean), Number.isNaN(h.stddev));
