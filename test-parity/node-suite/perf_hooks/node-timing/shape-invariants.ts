import { performance, PerformanceEntry } from "node:perf_hooks";
const timing = performance.nodeTiming;
console.log("identity:", timing === performance.nodeTiming);
console.log(
  "entry:",
  timing instanceof PerformanceEntry,
  timing.name,
  timing.entryType,
  timing.startTime,
);
console.log(
  "ordered:",
  timing.nodeStart <= timing.v8Start && timing.v8Start <= timing.environment &&
    timing.environment <= timing.bootstrapComplete,
);
console.log("duration:", timing.duration >= timing.bootstrapComplete);
console.log(
  "loop sentinels:",
  timing.loopStart === -1 || timing.loopStart >= timing.bootstrapComplete,
  timing.loopExit === -1 || timing.loopExit >= timing.loopStart,
);
