import { eventLoopUtilization, performance, timerify } from "node:perf_hooks";
console.log("timerify alias:", performance.timerify === timerify);
console.log(
  "elu alias:",
  performance.eventLoopUtilization === eventLoopUtilization,
);
