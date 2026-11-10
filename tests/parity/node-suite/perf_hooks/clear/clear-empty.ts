import { performance } from "node:perf_hooks";
// clearMarks() / clearMeasures() with nothing recorded is a no-op.
performance.clearMarks();
performance.clearMeasures();
console.log("marks:", performance.getEntriesByType("mark").length);
console.log("measures:", performance.getEntriesByType("measure").length);
