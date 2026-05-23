import { performance } from "node:perf_hooks";
// performance.now() is monotonic — two reads in the same tick respect order.
const a = performance.now();
const b = performance.now();
console.log("monotonic:", b >= a);
