import { createHistogram } from "node:perf_hooks";
const h = createHistogram();
h.record(1);
for (const value of [-1, 101, NaN, "50", null] as const) {
  try {
    h.percentile(value as any);
    console.log(String(value), "no throw");
  } catch (error) {
    console.log(String(value), (error as Error).name, (error as any).code);
  }
}
