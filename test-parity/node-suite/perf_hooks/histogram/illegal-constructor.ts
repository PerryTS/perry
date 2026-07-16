import { createHistogram } from "node:perf_hooks";
const h = createHistogram();
console.log("constructor:", h.constructor.name);
try {
  new (h.constructor as any)();
  console.log("no throw");
} catch (error) {
  console.log((error as Error).name, (error as any).code);
}
