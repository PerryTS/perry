import { performance } from "node:perf_hooks";
// performance.mark(name) returns a PerformanceMark with the documented fields.
const m = performance.mark("x");
console.log("name:", m.name);
console.log("entryType:", m.entryType);
console.log("duration:", m.duration);
console.log("detail:", m.detail);
console.log("startTime type:", typeof m.startTime);
