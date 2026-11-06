import { performance, PerformanceObserver } from "node:perf_hooks";
const observer = new PerformanceObserver(() => {});
try {
  observer.observe({ entryTypes: ["measure"] });
  performance.measure("queued", { start: 1, duration: 1 });
  observer.observe({ entryTypes: ["mark"] });
  performance.mark("later", { startTime: 3 });
  console.log(
    observer.takeRecords().map((entry) => `${entry.name}:${entry.entryType}`)
      .join(","),
  );
} finally {
  observer.disconnect();
  performance.clearMarks();
  performance.clearMeasures();
}
