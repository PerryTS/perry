import { setTimeout } from "node:timers";
let fired = 0;
await new Promise<void>((resolve) => {
  setTimeout(() => { fired++; resolve(); }, "0" as any);
});
console.log("string delay fired:", fired);
