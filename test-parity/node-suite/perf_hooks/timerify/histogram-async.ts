import { createHistogram, timerify } from "node:perf_hooks";
const histogram = createHistogram();
const wrapped = timerify(async (x: number) => x * 2, { histogram });
const promise = wrapped(3);
console.log("pending:", histogram.count);
console.log("result:", await promise);
console.log("settled:", histogram.count);
console.log("positive:", histogram.minBigInt > 0n);
