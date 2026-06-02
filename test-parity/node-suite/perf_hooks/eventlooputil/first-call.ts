import { performance } from "node:perf_hooks";
// The first (no-arg) eventLoopUtilization() reading is a self-consistent
// triple: all numbers, utilization in [0, 1].
const elu = performance.eventLoopUtilization();
console.log("idle:", typeof elu.idle);
console.log("active:", typeof elu.active);
console.log("utilization in range:", elu.utilization >= 0 && elu.utilization <= 1);
