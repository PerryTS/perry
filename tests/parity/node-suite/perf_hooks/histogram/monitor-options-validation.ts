import { monitorEventLoopDelay } from "node:perf_hooks";
function outcome(label: string, options: any) {
  let h: any;
  try {
    h = monitorEventLoopDelay(options);
    console.log(label, "ok");
  } catch (error) {
    console.log(label, (error as Error).name, (error as any).code);
  } finally {
    h?.disable();
  }
}
outcome("default", undefined);
outcome("null", null);
outcome("number", 1);
outcome("zero", { resolution: 0 });
outcome("fraction", { resolution: 1.5 });
outcome("string", { resolution: "10" });
outcome("valid", { resolution: 10 });
