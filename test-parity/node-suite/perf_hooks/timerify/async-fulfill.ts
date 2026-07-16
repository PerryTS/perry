import { PerformanceObserver, timerify } from "node:perf_hooks";
const observer = new PerformanceObserver(() => {});
try {
  observer.observe({ entryTypes: ["function"] });
  const wrapped = timerify(async (value: number) => value + 1);
  const promise = wrapped(4);
  console.log("pending records:", observer.takeRecords().length);
  console.log("result:", await promise);
  console.log("settled records:", observer.takeRecords().length);
} finally {
  observer.disconnect();
}
