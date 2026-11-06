import {
  Performance,
  PerformanceEntry,
  PerformanceMeasure,
  PerformanceObserverEntryList,
  PerformanceResourceTiming,
} from "node:perf_hooks";
for (
  const [name, Ctor] of Object.entries({
    Performance,
    PerformanceEntry,
    PerformanceMeasure,
    PerformanceObserverEntryList,
    PerformanceResourceTiming,
  })
) {
  try {
    new (Ctor as any)();
    console.log(`${name}: no throw`);
  } catch (error) {
    console.log(`${name}:`, (error as any).name, (error as any).code);
  }
}
