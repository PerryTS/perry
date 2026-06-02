import { performance } from "node:perf_hooks";
// measure(name, { end }) with no start defaults start to 0.
const m = performance.measure("eo", { end: 100 });
console.log("startTime:", m.startTime);
console.log("duration:", m.duration);
