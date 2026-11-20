import {
  performance,
  PerformanceObserver,
  PerformanceEntry,
  PerformanceMark,
  PerformanceMeasure,
  constants,
} from "node:perf_hooks";
// The named exports of node:perf_hooks must have their Node-documented shapes.
console.log("performance:", typeof performance);
console.log("PerformanceObserver:", typeof PerformanceObserver);
console.log("PerformanceEntry:", typeof PerformanceEntry);
console.log("PerformanceMark:", typeof PerformanceMark);
console.log("PerformanceMeasure:", typeof PerformanceMeasure);
console.log("constants:", typeof constants);
