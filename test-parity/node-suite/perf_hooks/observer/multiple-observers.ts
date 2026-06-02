import { performance, PerformanceObserver } from "node:perf_hooks";
// Two observers subscribed to 'mark' both receive the entry.
let count = 0;
await new Promise<void>((resolve) => {
  function finish() {
    if (count === 2) {
      console.log("both fired: true");
      resolve();
    }
  }
  const o1 = new PerformanceObserver(() => {
    o1.disconnect();
    count++;
    finish();
  });
  const o2 = new PerformanceObserver(() => {
    o2.disconnect();
    count++;
    finish();
  });
  o1.observe({ entryTypes: ["mark"] });
  o2.observe({ entryTypes: ["mark"] });
  performance.mark("shared");
});
