import { performance } from "node:perf_hooks";
// measure(name, { start, end }) with numeric endpoints (no marks).
const m = performance.measure("n", { start: 10, end: 25 });
console.log("startTime:", m.startTime);
console.log("duration:", m.duration);
