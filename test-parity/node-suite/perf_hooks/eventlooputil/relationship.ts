import { performance } from "node:perf_hooks";
// utilization == active / (idle + active) for a single reading.
const e = performance.eventLoopUtilization();
const expected = e.idle + e.active > 0 ? e.active / (e.idle + e.active) : 0;
console.log("consistent:", Math.abs(e.utilization - expected) < 0.001);
