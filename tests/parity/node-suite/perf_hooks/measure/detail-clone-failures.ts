import { performance } from "node:perf_hooks";
const cycle: any = {};
cycle.self = cycle;
try {
  const measure: any = performance.measure("cycle", {
    start: 0,
    duration: 1,
    detail: cycle,
  });
  console.log(
    "cycle:",
    measure.detail !== cycle,
    measure.detail.self === measure.detail,
  );
  try {
    performance.measure("function", {
      start: 0,
      duration: 1,
      detail: () => {},
    });
    console.log("function no throw");
  } catch (error) {
    console.log("function:", (error as Error).name);
  }
} finally {
  performance.clearMeasures();
}
