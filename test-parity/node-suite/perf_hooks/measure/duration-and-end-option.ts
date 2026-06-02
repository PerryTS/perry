import { performance } from "node:perf_hooks";
// measure(name, { duration, end }) back-computes startTime = end - duration.
performance.mark("b", { startTime: 10 });
const m = performance.measure("foo", { duration: 11, end: "b" });
console.log("startTime:", m.startTime);
console.log("duration:", m.duration);
