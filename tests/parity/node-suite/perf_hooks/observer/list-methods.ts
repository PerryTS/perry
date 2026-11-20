import { performance, PerformanceObserver } from "node:perf_hooks";
// The PerformanceObserverEntryList passed to the callback supports
// getEntries / getEntriesByType / getEntriesByName.
await new Promise<void>((resolve) => {
  const obs = new PerformanceObserver((list) => {
    console.log("getEntries:", list.getEntries().length);
    console.log("byType mark:", list.getEntriesByType("mark").length);
    console.log("byType measure:", list.getEntriesByType("measure").length);
    console.log("byName:", list.getEntriesByName("om").length);
    obs.disconnect();
    resolve();
  });
  obs.observe({ entryTypes: ["mark"] });
  performance.mark("om");
});
