import { createHistogram, timerify } from "node:perf_hooks";
const histogram = createHistogram();
const wrapped = timerify((x: number) => x * 2, { histogram });
console.log("before:", histogram.count);
console.log("result:", wrapped(3));
console.log("after:", histogram.count);
console.log("positive:", histogram.minBigInt > 0n);
