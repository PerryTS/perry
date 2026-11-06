import {
  performance,
  PerformanceEntry,
  PerformanceMark,
} from "node:perf_hooks";
const detail = { value: 1 };
const mark = new PerformanceMark("direct", { startTime: 7, detail });
console.log("shape:", mark.name, mark.entryType, mark.startTime, mark.duration);
console.log(
  "instances:",
  mark instanceof PerformanceEntry,
  mark instanceof PerformanceMark,
);
console.log(
  "detail cloned:",
  JSON.stringify(mark.detail),
  mark.detail !== detail,
);
console.log(
  "not timeline entry:",
  performance.getEntriesByName("direct").length,
);
