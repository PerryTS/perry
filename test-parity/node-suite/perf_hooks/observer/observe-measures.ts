import { performance, PerformanceObserver } from "node:perf_hooks";
// An observer subscribed to 'measure' receives only measure entries.
await new Promise<void>((resolve) => {
  const obs = new PerformanceObserver((list) => {
    console.log("observed:", list.getEntriesByType("measure").map((e) => e.name).join(","));
    obs.disconnect();
    resolve();
  });
  obs.observe({ entryTypes: ["measure"] });
  performance.mark("x", { startTime: 0 });
  performance.mark("y", { startTime: 9 });
  performance.measure("xy", "x", "y");
});
