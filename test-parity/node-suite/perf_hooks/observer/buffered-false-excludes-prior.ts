import { performance, PerformanceObserver } from "node:perf_hooks";
performance.clearMarks();
performance.mark("prior", { startTime: 1 });
const observer = new PerformanceObserver(() => {});
try {
  observer.observe({ type: "mark", buffered: false });
  performance.mark("future", { startTime: 2 });
  console.log(observer.takeRecords().map((entry) => entry.name).join(","));
} finally {
  observer.disconnect();
  performance.clearMarks();
}
