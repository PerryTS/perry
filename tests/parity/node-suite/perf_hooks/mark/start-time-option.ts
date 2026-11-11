import { performance } from "node:perf_hooks";
// mark(name, { startTime }) sets a deterministic startTime on the entry.
const m = performance.mark("a", { startTime: 1 });
console.log("startTime:", m.startTime);
console.log("duration:", m.duration);
