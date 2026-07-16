import { performance, PerformanceObserver } from "node:perf_hooks";
let calls = 0;
const observer = new PerformanceObserver(() => {
  calls++;
});
try {
  observer.observe({ entryTypes: ["mark"] });
  performance.mark("drained");
  console.log("taken:", observer.takeRecords().length);
  await new Promise<void>((resolve) => setImmediate(resolve));
  console.log("calls:", calls);
} finally {
  observer.disconnect();
  performance.clearMarks();
}
