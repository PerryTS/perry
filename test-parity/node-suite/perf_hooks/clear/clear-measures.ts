import { performance } from "node:perf_hooks";
// clearMeasures() removes measure entries but leaves marks intact.
performance.mark("a", { startTime: 0 });
performance.mark("b", { startTime: 10 });
performance.measure("m1", "a", "b");
performance.measure("m2", "a", "b");
console.log("measures before:", performance.getEntriesByType("measure").length);
performance.clearMeasures();
console.log("measures after:", performance.getEntriesByType("measure").length);
console.log("marks still there:", performance.getEntriesByType("mark").length);
