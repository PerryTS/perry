import { performance } from "node:perf_hooks";
// measure(name, { duration, start }) uses the named start and the given duration.
performance.mark("b", { startTime: 10 });
const m = performance.measure("foo", { duration: 11, start: "b" });
console.log("startTime:", m.startTime);
console.log("duration:", m.duration);
