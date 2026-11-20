import { performance, PerformanceObserver } from "node:perf_hooks";
const prototype = Object.getPrototypeOf(performance);
for (const key of ["now", "mark", "measure", "getEntries", "toJSON"]) {
  const descriptor = Object.getOwnPropertyDescriptor(prototype, key)!;
  console.log(
    key,
    typeof descriptor.value,
    descriptor.writable,
    descriptor.enumerable,
    descriptor.configurable,
  );
}
const origin = Object.getOwnPropertyDescriptor(prototype, "timeOrigin")!;
console.log(
  "timeOrigin",
  typeof origin.get,
  origin.set,
  origin.enumerable,
  origin.configurable,
);
const supported = Object.getOwnPropertyDescriptor(
  PerformanceObserver,
  "supportedEntryTypes",
)!;
console.log(
  "supportedEntryTypes",
  typeof supported.get,
  supported.set,
  supported.enumerable,
  supported.configurable,
);
