import { performance } from "node:perf_hooks";
// measure(name) with no start/end measures from time-origin (0) to now.
const m = performance.measure("solo");
console.log("startTime:", m.startTime);
console.log("duration non-negative:", m.duration >= 0);
console.log("entryType:", m.entryType);
