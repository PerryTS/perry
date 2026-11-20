import { PerformanceObserver } from "node:perf_hooks";
// new PerformanceObserver() without a callback throws a TypeError.
try {
  new (PerformanceObserver as any)();
  console.log("threw: false");
} catch (e) {
  console.log("threw:", e instanceof TypeError);
}
