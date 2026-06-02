import { performance } from "node:perf_hooks";
// After clearing marks and measures, getEntries() is empty.
performance.mark("a");
performance.measure("m", "a");
performance.clearMarks();
performance.clearMeasures();
console.log("total:", performance.getEntries().length);
