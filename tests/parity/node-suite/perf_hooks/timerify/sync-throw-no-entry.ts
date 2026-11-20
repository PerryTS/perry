import { PerformanceObserver, timerify } from "node:perf_hooks";
const observer = new PerformanceObserver(() => {
  console.log("callback fired");
});
try {
  observer.observe({ entryTypes: ["function"] });
  const wrapped = timerify(() => {
    throw new RangeError("boom");
  });
  try {
    wrapped();
  } catch (error) {
    console.log("same error:", (error as Error).name, (error as Error).message);
  }
  await new Promise<void>((resolve) => setImmediate(resolve));
  console.log("records:", observer.takeRecords().length);
} finally {
  observer.disconnect();
}
