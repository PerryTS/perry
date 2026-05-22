import { performance, PerformanceObserver } from "node:perf_hooks";
// observe({ entryTypes: ["mark", "measure"] }) buffers both kinds; entries
// created in one task are delivered together.
await new Promise<void>((resolve) => {
  const obs = new PerformanceObserver((list) => {
    console.log("marks:", list.getEntriesByType("mark").length);
    console.log("measures:", list.getEntriesByType("measure").length);
    obs.disconnect();
    resolve();
  });
  obs.observe({ entryTypes: ["mark", "measure"] });
  performance.mark("a", { startTime: 0 });
  performance.mark("b", { startTime: 4 });
  performance.measure("ab", "a", "b");
});
