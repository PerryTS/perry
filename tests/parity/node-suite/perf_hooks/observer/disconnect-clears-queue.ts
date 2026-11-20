import { performance, PerformanceObserver } from "node:perf_hooks";
let calls = 0;
const observer = new PerformanceObserver(() => {
  calls++;
});
try {
  observer.observe({ entryTypes: ["mark"] });
  performance.mark("pending");
  observer.disconnect();
  await new Promise<void>((resolve) => setImmediate(resolve));
  console.log("calls:", calls);
  console.log("records:", observer.takeRecords().length);
} finally {
  observer.disconnect();
  performance.clearMarks();
}
