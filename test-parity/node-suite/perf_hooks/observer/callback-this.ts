import { performance, PerformanceObserver } from "node:perf_hooks";
let same = false;
const observer = new PerformanceObserver(function () {
  same = this === observer;
});
try {
  observer.observe({ entryTypes: ["mark"] });
  performance.mark("callback-this");
  await new Promise<void>((resolve) => setImmediate(resolve));
  console.log("this is observer:", same);
} finally {
  observer.disconnect();
  performance.clearMarks();
}
