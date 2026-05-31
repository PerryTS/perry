import { performance } from "node:perf_hooks";

const start = performance.eventLoopUtilization();

setTimeout(() => {
  const end = performance.eventLoopUtilization();
  const twoArg = performance.eventLoopUtilization(end, start);
  const oneArg = performance.eventLoopUtilization(end);
  const twoSpan = twoArg.idle + twoArg.active;
  const oneSpan = oneArg.idle + oneArg.active;

  console.log("two arg type:", typeof twoArg.utilization);
  console.log("two arg wider than one arg:", twoSpan > oneSpan + 5);
  console.log("two arg in range:", twoArg.utilization >= 0 && twoArg.utilization <= 1);
}, 35);
