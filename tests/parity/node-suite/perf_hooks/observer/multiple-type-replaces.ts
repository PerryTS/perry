import { performance, PerformanceObserver } from "node:perf_hooks";
const observer = new PerformanceObserver(() => {});
try {
  observer.observe({ entryTypes: ["mark"] });
  observer.observe({ entryTypes: ["measure"] });
  performance.mark("a", { startTime: 1 });
  performance.measure("m", { start: 1, duration: 1 });
  console.log(observer.takeRecords().map((entry) => entry.entryType).join(","));
} finally {
  observer.disconnect();
  performance.clearMarks();
  performance.clearMeasures();
}
