import { performance } from "node:perf_hooks";
const first = performance.toJSON();
const second = performance.toJSON();
console.log("fresh root:", first !== second);
console.log("fresh timing:", first.nodeTiming !== second.nodeTiming);
console.log(
  "fresh elu:",
  first.eventLoopUtilization !== second.eventLoopUtilization,
);
console.log(
  "stable origin:",
  first.timeOrigin === second.timeOrigin &&
    first.timeOrigin === performance.timeOrigin,
);
