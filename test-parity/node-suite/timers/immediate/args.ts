import { setImmediate } from "node:timers";
await new Promise<void>((resolve) => {
  setImmediate((a: string, b: number) => {
    console.log("immediate args:", a, b);
    resolve();
  }, "x", 3);
});
