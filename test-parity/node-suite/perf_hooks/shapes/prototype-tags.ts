import {
  performance,
  PerformanceMark,
  PerformanceMeasure,
  PerformanceObserver,
  PerformanceObserverEntryList,
  PerformanceResourceTiming,
} from "node:perf_hooks";
const observer = new PerformanceObserver(() => {});
const names: [string, any][] = [
  ["performance", performance],
  ["mark", performance.mark("tag")],
  ["measure", performance.measure("tag-measure")],
  ["observer", observer],
];
for (const [name, value] of names) {
  console.log(name, Object.prototype.toString.call(value));
}
for (
  const Ctor of [
    PerformanceMark,
    PerformanceMeasure,
    PerformanceObserver,
    PerformanceObserverEntryList,
    PerformanceResourceTiming,
  ]
) {
  console.log(
    Ctor.name,
    Object.getOwnPropertyDescriptor(Ctor.prototype, Symbol.toStringTag)?.value,
  );
}
performance.clearMarks();
performance.clearMeasures();
observer.disconnect();
