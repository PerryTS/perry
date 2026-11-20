import { performance, PerformanceObserver } from "node:perf_hooks";
const observer = new PerformanceObserver(() => {});
try {
  observer.observe({ entryTypes: ["mark"] });
  observer.disconnect();
  observer.observe({ type: "measure" });
  performance.mark("a", { startTime: 0 });
  performance.measure("m", { start: 0, duration: 1 });
  console.log(observer.takeRecords().map((e) => e.entryType).join(","));
} finally {
  observer.disconnect();
  performance.clearMarks();
  performance.clearMeasures();
}
