import { performance } from "node:perf_hooks";
// performance.timeOrigin is a number (ms since the Unix epoch at process start).
console.log("type:", typeof performance.timeOrigin);
console.log("positive:", performance.timeOrigin > 0);
