import { performance } from "node:perf_hooks";
try {
  performance.clearMeasures(Symbol("x") as any);
  console.log("no throw");
} catch (error) {
  console.log((error as Error).name);
}
