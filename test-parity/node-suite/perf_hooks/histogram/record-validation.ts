import { createHistogram } from "node:perf_hooks";
const h = createHistogram();
for (
  const [label, value] of [
    ["zero", 0],
    ["fraction", 1.5],
    ["unsafe", Number.MAX_SAFE_INTEGER + 1],
    ["nan", NaN],
    ["string", "1"],
    ["null", null],
  ] as const
) {
  try {
    h.record(value as any);
    console.log(label, "no throw");
  } catch (error) {
    console.log(label, (error as Error).name, (error as any).code);
  }
}
console.log("count:", h.count);
