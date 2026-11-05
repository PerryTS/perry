import { performance } from "node:perf_hooks";
// measure(name, { end }) measures from time-origin (0) to the named end mark.
performance.mark("a", { startTime: 0 });
performance.mark("b", { startTime: 10 });
const m = performance.measure("foo", { end: "b" });
console.log("startTime:", m.startTime);
console.log("duration:", m.duration);
