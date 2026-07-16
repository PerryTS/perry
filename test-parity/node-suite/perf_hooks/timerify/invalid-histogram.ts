import { timerify } from "node:perf_hooks";
for (const value of [null, 1, "", false, {}, []]) {
  try {
    timerify(() => {}, { histogram: value as any });
    console.log(typeof value, "no throw");
  } catch (error) {
    console.log(typeof value, (error as Error).name, (error as any).code);
  }
}
