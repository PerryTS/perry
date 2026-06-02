import { setImmediate } from "node:timers";
await new Promise<void>((resolve) => {
  setImmediate((a: boolean, b: string) => {
    console.log("mixed immediate args:", a, b);
    resolve();
  }, true, "ok");
});
