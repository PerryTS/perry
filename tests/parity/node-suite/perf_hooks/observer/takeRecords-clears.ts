import { performance, PerformanceObserver } from "node:perf_hooks";
// observer.takeRecords() returns and CLEARS the pending entry list — the
// next call returns 0 (unless new entries were observed in between).
let count = 0;
const obs = new PerformanceObserver(() => {
  count++;
});
obs.observe({ entryTypes: ["mark"] });
performance.mark("tr");
const r1 = obs.takeRecords().length;
const r2 = obs.takeRecords().length;
obs.disconnect();
console.log("first call has entries:", r1 > 0);
console.log("second call is empty:", r2 === 0);
