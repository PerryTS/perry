import { performance } from "node:perf_hooks";
// The performance object exposes the W3C User Timing + Node methods as
// callable function-valued properties (not just the now() special case).
console.log("now:", typeof performance.now);
console.log("mark:", typeof performance.mark);
console.log("measure:", typeof performance.measure);
console.log("clearMarks:", typeof performance.clearMarks);
console.log("clearMeasures:", typeof performance.clearMeasures);
console.log("getEntries:", typeof performance.getEntries);
console.log("getEntriesByName:", typeof performance.getEntriesByName);
console.log("getEntriesByType:", typeof performance.getEntriesByType);
console.log("timeOrigin:", typeof performance.timeOrigin);
