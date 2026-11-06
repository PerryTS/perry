import { performance } from "node:perf_hooks";
// measure(name, startMark) referencing a mark that doesn't exist throws.
performance.clearMarks();
try {
  performance.measure("x", "does-not-exist");
  console.log("threw: false");
} catch (e) {
  console.log("threw:", e instanceof Error);
}
