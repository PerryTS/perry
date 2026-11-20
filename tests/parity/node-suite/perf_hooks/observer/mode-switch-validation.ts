import { PerformanceObserver } from "node:perf_hooks";
function outcome(label: string, first: any, second: any) {
  const observer = new PerformanceObserver(() => {});
  try {
    observer.observe(first);
    observer.observe(second);
    console.log(label, "no throw");
  } catch (error) {
    console.log(label, (error as Error).name);
  } finally {
    observer.disconnect();
  }
}
outcome("multi to single", { entryTypes: ["mark"] }, { type: "measure" });
outcome("single to multi", { type: "mark" }, { entryTypes: ["measure"] });
