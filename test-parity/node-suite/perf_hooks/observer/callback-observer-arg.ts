import { performance, PerformanceObserver } from "node:perf_hooks";
// The observer callback's second argument is the PerformanceObserver itself.
await new Promise<void>((resolve) => {
  const obs = new PerformanceObserver((_list, observer) => {
    console.log("second arg is observer:", observer === obs);
    obs.disconnect();
    resolve();
  });
  obs.observe({ entryTypes: ["mark"] });
  performance.mark("a");
});
