import { performance } from "node:perf_hooks";
// clearMarks() and clearMeasures() return undefined.
performance.mark("a");
performance.measure("m", "a");
console.log("clearMarks:", performance.clearMarks() === undefined);
console.log("clearMeasures:", performance.clearMeasures() === undefined);
