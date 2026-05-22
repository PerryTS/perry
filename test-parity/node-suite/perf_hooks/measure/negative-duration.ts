import { performance } from "node:perf_hooks";
// When the end mark precedes the start mark, the measure duration is negative.
performance.mark("s", { startTime: 20 });
performance.mark("e", { startTime: 5 });
const m = performance.measure("d", "s", "e");
console.log("startTime:", m.startTime);
console.log("duration:", m.duration);
