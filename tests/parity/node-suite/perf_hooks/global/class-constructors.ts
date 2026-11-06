import {
  performance,
  Performance,
  PerformanceEntry,
  PerformanceMark,
  PerformanceMeasure,
  PerformanceObserver,
  PerformanceObserverEntryList,
  PerformanceResourceTiming,
} from "node:perf_hooks";

const g: any = globalThis;

console.log(
  "Performance:",
  typeof Performance,
  typeof g.Performance,
  Performance === g.Performance,
  Performance.name,
  Performance.length,
);
console.log(
  "PerformanceEntry:",
  typeof PerformanceEntry,
  typeof g.PerformanceEntry,
  PerformanceEntry === g.PerformanceEntry,
  PerformanceEntry.name,
  PerformanceEntry.length,
);
console.log(
  "PerformanceMark:",
  typeof PerformanceMark,
  typeof g.PerformanceMark,
  PerformanceMark === g.PerformanceMark,
  PerformanceMark.name,
  PerformanceMark.length,
);
console.log(
  "PerformanceMeasure:",
  typeof PerformanceMeasure,
  typeof g.PerformanceMeasure,
  PerformanceMeasure === g.PerformanceMeasure,
  PerformanceMeasure.name,
  PerformanceMeasure.length,
);
console.log(
  "PerformanceObserver:",
  typeof PerformanceObserver,
  typeof g.PerformanceObserver,
  PerformanceObserver === g.PerformanceObserver,
  PerformanceObserver.name,
  PerformanceObserver.length,
);
console.log(
  "PerformanceObserverEntryList:",
  typeof PerformanceObserverEntryList,
  typeof g.PerformanceObserverEntryList,
  PerformanceObserverEntryList === g.PerformanceObserverEntryList,
  PerformanceObserverEntryList.name,
  PerformanceObserverEntryList.length,
);
console.log(
  "PerformanceResourceTiming:",
  typeof PerformanceResourceTiming,
  typeof g.PerformanceResourceTiming,
  PerformanceResourceTiming === g.PerformanceResourceTiming,
  PerformanceResourceTiming.name,
  PerformanceResourceTiming.length,
);

console.log("performance same:", g.performance === performance);
console.log("performance instanceof Performance:", performance instanceof Performance);

const mark = performance.mark("class-constructors-mark");
console.log(
  "mark instances:",
  mark instanceof PerformanceEntry,
  mark instanceof PerformanceMark,
  mark instanceof PerformanceMeasure,
  mark instanceof PerformanceResourceTiming,
);
console.log(
  "observer static:",
  Array.isArray(PerformanceObserver.supportedEntryTypes),
  PerformanceObserver.supportedEntryTypes.includes("mark"),
);
