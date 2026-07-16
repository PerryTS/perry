import { performance, PerformanceObserver } from "node:perf_hooks";
// observe({ type, buffered: true }) delivers entries that already existed
// before observe() was called. (Fallback timer guards against a non-delivering
// runtime so the test reports 0 instead of hanging.)
performance.mark("pre1");
performance.mark("pre2");
let names = "not delivered";
const obs = new PerformanceObserver((list) => {
  names = list.getEntries().map((entry) => entry.name).join(",");
});
try {
  obs.observe({ type: "mark", buffered: true });
  await new Promise<void>((resolve) => setImmediate(resolve));
  console.log("buffered names:", names);
} finally {
  obs.disconnect();
  performance.clearMarks();
}
