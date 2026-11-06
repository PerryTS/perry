import { PerformanceObserver } from "node:perf_hooks";
const types = PerformanceObserver.supportedEntryTypes;
console.log(types.join(","));
console.log("frozen:", Object.isFrozen(types));
console.log("fresh getter:", types !== PerformanceObserver.supportedEntryTypes);
