import { performance } from "node:perf_hooks";
const name = "nodeStart";
try {
  performance.mark(name);
  console.log("mark no throw");
} catch (error) {
  console.log("mark:", (error as Error).name, (error as any).code);
}
const measure = performance.measure("milestone", { start: name, duration: 1 });
console.log(
  "measure start:",
  measure.startTime === performance.nodeTiming.nodeStart,
);
try {
  performance.clearMarks(name);
  console.log("clear no throw");
} catch (error) {
  console.log("clear:", (error as Error).name, (error as any).code);
}
performance.clearMeasures();
