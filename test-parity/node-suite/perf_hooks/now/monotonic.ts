import { performance } from "node:perf_hooks";
// Successive performance.now() readings are monotonically non-decreasing.
const a = performance.now();
const b = performance.now();
console.log("monotonic:", b >= a);
