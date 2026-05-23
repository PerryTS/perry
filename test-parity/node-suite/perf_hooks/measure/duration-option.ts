import { performance } from "node:perf_hooks";
// measure(name, { start, duration }) sets duration directly (endTime derived).
const m = performance.measure("d", { start: 5, duration: 10 });
console.log("startTime:", m.startTime);
console.log("duration:", m.duration);
