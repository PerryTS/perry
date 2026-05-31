import {
  performance,
  PerformanceObserver,
  PerformanceObserverEntryList,
} from "node:perf_hooks";

await new Promise<void>((resolve) => {
  const obs = new PerformanceObserver((list, observer) => {
    console.log("list instanceof:", list instanceof PerformanceObserverEntryList);
    console.log(
      "list methods:",
      typeof list.getEntries,
      typeof list.getEntriesByName,
      typeof list.getEntriesByType,
    );
    console.log("observer same:", observer === obs);
    obs.disconnect();
    resolve();
  });
  obs.observe({ entryTypes: ["mark"] });
  performance.mark("list-instanceof-mark");
});
