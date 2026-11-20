import { performance, PerformanceObserver } from "node:perf_hooks";
// observe({ type }) is the single-entry-type form of observe().
await new Promise<void>((resolve) => {
  const obs = new PerformanceObserver((list) => {
    console.log("observed:", list.getEntries().map((e) => e.name).join(","));
    obs.disconnect();
    resolve();
  });
  obs.observe({ type: "mark" });
  performance.mark("a");
  performance.mark("b");
});
