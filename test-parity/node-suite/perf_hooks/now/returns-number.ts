import { performance } from "node:perf_hooks";
// performance.now() returns a non-negative high-resolution millisecond reading.
const t = performance.now();
console.log("type:", typeof t);
console.log("non-negative:", t >= 0);
