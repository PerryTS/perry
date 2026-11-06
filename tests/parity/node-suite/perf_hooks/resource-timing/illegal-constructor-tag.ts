import { PerformanceResourceTiming } from "node:perf_hooks";
console.log(
  "tag:",
  Object.getOwnPropertyDescriptor(
    PerformanceResourceTiming.prototype,
    Symbol.toStringTag,
  )?.value,
);
try {
  new PerformanceResourceTiming();
  console.log("no throw");
} catch (error) {
  console.log((error as Error).name, (error as any).code);
}
