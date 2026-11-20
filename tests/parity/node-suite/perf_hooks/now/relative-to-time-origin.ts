import { performance } from "node:perf_hooks";
// performance.now() is measured relative to timeOrigin, so it is far smaller
// than timeOrigin (which is ms since the Unix epoch).
console.log("now < timeOrigin:", performance.now() < performance.timeOrigin);
