import {
  performance,
  PerformanceEntry,
  PerformanceObserver,
} from "node:perf_hooks";
function outcome(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label, "no throw");
  } catch (error) {
    console.log(label, (error as Error).name);
  }
}
outcome(
  "performance.now",
  () => Reflect.apply(Object.getPrototypeOf(performance).now, {}, []),
);
outcome(
  "performance.mark",
  () => Reflect.apply(Object.getPrototypeOf(performance).mark, {}, ["x"]),
);
outcome(
  "entry.toJSON",
  () => Reflect.apply(PerformanceEntry.prototype.toJSON, {}, []),
);
outcome(
  "observer.observe",
  () =>
    Reflect.apply(PerformanceObserver.prototype.observe, {}, [{
      entryTypes: ["mark"],
    }]),
);
outcome(
  "observer.disconnect",
  () => Reflect.apply(PerformanceObserver.prototype.disconnect, {}, []),
);
outcome(
  "observer.takeRecords",
  () => Reflect.apply(PerformanceObserver.prototype.takeRecords, {}, []),
);
