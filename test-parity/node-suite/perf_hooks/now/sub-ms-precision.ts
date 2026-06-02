import { performance } from "node:perf_hooks";
// performance.now() returns sub-millisecond fractional values.
const n = performance.now();
console.log("fractional:", n !== Math.floor(n));
