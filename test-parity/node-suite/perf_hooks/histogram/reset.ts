import { createHistogram } from "node:perf_hooks";
const h = createHistogram();
h.record(5);
console.log("reset return:", h.reset() === undefined);
console.log("count:", h.count, h.countBigInt === 0n);
console.log(
  "empty bounds:",
  h.minBigInt === 9223372036854775807n,
  h.maxBigInt === 0n,
);
console.log("empty stats:", Number.isNaN(h.mean), Number.isNaN(h.stddev));
