import { performance, PerformanceEntry, PerformanceMark } from "node:perf_hooks";
// A mark is both a PerformanceEntry and a PerformanceMark.
const m = performance.mark("x");
console.log("PerformanceEntry:", m instanceof PerformanceEntry);
console.log("PerformanceMark:", m instanceof PerformanceMark);
