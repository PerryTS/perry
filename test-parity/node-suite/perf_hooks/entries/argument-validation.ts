import { performance } from "node:perf_hooks";
function outcome(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label, "no throw");
  } catch (error) {
    console.log(label, (error as any).name, (error as any).code);
  }
}
outcome("byName missing", () => (performance.getEntriesByName as any)());
outcome("byType missing", () => (performance.getEntriesByType as any)());
outcome(
  "byName symbol",
  () => performance.getEntriesByName(Symbol("x") as any),
);
outcome(
  "byType symbol",
  () => performance.getEntriesByType(Symbol("x") as any),
);
performance.mark("1", { startTime: 1 });
console.log("name coerced:", performance.getEntriesByName(1 as any).length);
performance.clearMarks();
