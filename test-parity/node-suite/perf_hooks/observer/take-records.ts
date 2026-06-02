import { performance, PerformanceObserver } from "node:perf_hooks";
// takeRecords() synchronously drains the observer's buffered entries.
const obs = new PerformanceObserver(() => {});
obs.observe({ entryTypes: ["mark"] });
performance.mark("tr");
const recs = obs.takeRecords();
console.log("count:", recs.length);
console.log("name:", recs.length ? recs[0].name : "-");
obs.disconnect();
