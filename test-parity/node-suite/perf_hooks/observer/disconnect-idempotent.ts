import { PerformanceObserver } from "node:perf_hooks";
// PerformanceObserver#disconnect() is idempotent — calling it again after
// the observer is already disconnected is a no-op (does not throw).
const obs = new PerformanceObserver(() => {});
obs.observe({ entryTypes: ["mark"] });
obs.disconnect();
let threw = false;
try {
  obs.disconnect();
} catch {
  threw = true;
}
console.log("double-disconnect threw:", threw);
