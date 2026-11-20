import { performance, PerformanceEntry, PerformanceMeasure } from "node:perf_hooks";
// A measure is both a PerformanceEntry and a PerformanceMeasure.
performance.mark("a", { startTime: 0 });
performance.mark("b", { startTime: 5 });
const m = performance.measure("ab", "a", "b");
console.log("PerformanceEntry:", m instanceof PerformanceEntry);
console.log("PerformanceMeasure:", m instanceof PerformanceMeasure);
