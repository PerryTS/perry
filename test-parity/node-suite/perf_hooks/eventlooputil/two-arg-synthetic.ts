import { performance } from "node:perf_hooks";

const newer = { idle: 10, active: 20, utilization: 2 / 3 };
const older = { idle: 3, active: 5, utilization: 5 / 8 };
const delta = performance.eventLoopUtilization(newer, older);
console.log("idle:", delta.idle);
console.log("active:", delta.active);
console.log("utilization:", delta.utilization === 15 / 22);
