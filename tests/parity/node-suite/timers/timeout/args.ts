import { setTimeout } from "node:timers";
await new Promise<void>((resolve) => {
  setTimeout((a: string, b: number, c: boolean) => {
    console.log("args:", a, b, c);
    resolve();
  }, 0, "x", 2, true);
});
