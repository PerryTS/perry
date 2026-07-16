import { timerify } from "node:perf_hooks";
for (const value of [undefined, null, 1, Infinity, "x", {}, []]) {
  try {
    timerify(value as any);
    console.log(typeof value, "no throw");
  } catch (error) {
    console.log(typeof value, (error as Error).name, (error as any).code);
  }
}
