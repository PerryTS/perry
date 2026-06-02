import { performance } from "node:perf_hooks";
// eventLoopUtilization(utilSince1, utilSince2) computes the delta between two
// prior readings; utilization stays in [0, 1].
const a = performance.eventLoopUtilization();
const b = performance.eventLoopUtilization();
const d = performance.eventLoopUtilization(b, a);
console.log("utilization type:", typeof d.utilization);
console.log("in range:", d.utilization >= 0 && d.utilization <= 1);
