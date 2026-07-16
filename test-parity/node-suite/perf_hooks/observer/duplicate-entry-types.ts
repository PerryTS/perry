import { performance, PerformanceObserver } from "node:perf_hooks";
const observer = new PerformanceObserver(() => {});
try {
  observer.observe({ entryTypes: ["mark", "mark"] });
  performance.mark("dedupe");
  console.log("records:", observer.takeRecords().length);
} finally {
  observer.disconnect();
  performance.clearMarks();
}
