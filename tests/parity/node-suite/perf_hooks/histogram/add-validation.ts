import { createHistogram } from "node:perf_hooks";
const h = createHistogram();
for (const value of [undefined, null, 1, "x", {}, h.toJSON()]) {
  try {
    h.add(value as any);
    console.log(typeof value, "no throw");
  } catch (error) {
    console.log(typeof value, (error as Error).name, (error as any).code);
  }
}
