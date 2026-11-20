import { performance } from "node:perf_hooks";
// eventLoopUtilization() returns { idle, active, utilization } numbers, and
// the diff form (elu2, elu1) yields a utilization in the [0, 1] range.
const elu1 = performance.eventLoopUtilization();
console.log("idle:", typeof elu1.idle);
console.log("active:", typeof elu1.active);
console.log("utilization:", typeof elu1.utilization);
const elu2 = performance.eventLoopUtilization(elu1);
console.log("diff in range:", elu2.utilization >= 0 && elu2.utilization <= 1);
