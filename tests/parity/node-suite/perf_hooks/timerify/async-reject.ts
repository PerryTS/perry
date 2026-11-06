import { PerformanceObserver, timerify } from "node:perf_hooks";
const observer = new PerformanceObserver(() => {});
try {
  observer.observe({ entryTypes: ["function"] });
  const reason = new Error("reject");
  const wrapped = timerify(async () => {
    throw reason;
  });
  try {
    await wrapped();
  } catch (error) {
    console.log("same reason:", error === reason);
  }
  console.log("records:", observer.takeRecords().length);
} finally {
  observer.disconnect();
}
