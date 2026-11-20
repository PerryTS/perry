import { performance, PerformanceObserver } from "node:perf_hooks";
// A PerformanceObserver observing 'mark' receives buffered mark entries
// in its callback (delivered asynchronously after the marks are created).
await new Promise<void>((resolve) => {
  const obs = new PerformanceObserver((list) => {
    const names = list.getEntries().map((e) => e.name);
    console.log("observed:", names.join(","));
    obs.disconnect();
    resolve();
  });
  obs.observe({ entryTypes: ["mark"] });
  performance.mark("a");
  performance.mark("b");
});
