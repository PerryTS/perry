import {
  performance,
  PerformanceObserver,
  PerformanceObserverEntryList,
} from "node:perf_hooks";
let list: PerformanceObserverEntryList | undefined;
const observer = new PerformanceObserver((value) => {
  list = value;
});
try {
  observer.observe({ entryTypes: ["mark"] });
  performance.mark("list-validation");
  await new Promise<void>((resolve) => setImmediate(resolve));
  for (
    const [label, fn] of [
      ["name missing", () => (list!.getEntriesByName as any)()],
      ["type missing", () => (list!.getEntriesByType as any)()],
      ["name symbol", () => list!.getEntriesByName(Symbol("x") as any)],
      ["wrong receiver", () =>
        Reflect.apply(
          PerformanceObserverEntryList.prototype.getEntries,
          {},
          [],
        )],
    ] as const
  ) {
    try {
      fn();
      console.log(label, "no throw");
    } catch (error) {
      console.log(label, (error as Error).name, (error as any).code);
    }
  }
} finally {
  observer.disconnect();
  performance.clearMarks();
}
