import { createHistogram } from "node:perf_hooks";
function outcome(label: string, value: any) {
  try {
    createHistogram(value);
    console.log(label, "ok");
  } catch (error) {
    console.log(label, (error as Error).name, (error as any).code);
  }
}
outcome("valid", { lowest: 1, highest: 11, figures: 1 });
outcome("null", null);
outcome("number", 1);
outcome("lowest string", { lowest: "1" });
outcome("highest null", { highest: null });
outcome("figures fraction", { figures: 1.5 });
outcome("figures high", { figures: 6 });
