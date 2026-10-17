import { performance, PerformanceEntry } from "node:perf_hooks";

const timing = performance.nodeTiming;
const prototype = Object.getPrototypeOf(timing);
console.log("entry prototype:", PerformanceEntry.prototype.isPrototypeOf(timing));
try {
  Object.setPrototypeOf(timing, null);
  console.log("cached identity:", performance.nodeTiming === timing);
  console.log("override:", Object.getPrototypeOf(performance.nodeTiming) === null);
  console.log("not entry:", performance.nodeTiming instanceof PerformanceEntry);
} finally {
  Object.setPrototypeOf(timing, prototype);
}
