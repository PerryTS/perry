import { createHistogram } from "node:perf_hooks";
const h = createHistogram();
console.log("first return:", h.recordDelta() === undefined, h.count);
await new Promise<void>((resolve) => setImmediate(resolve));
console.log("second return:", h.recordDelta() === undefined, h.count);
console.log("positive:", h.minBigInt > 0n);
