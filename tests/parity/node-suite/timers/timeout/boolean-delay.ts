import { setTimeout } from "node:timers";
let fired = 0;
await new Promise<void>((resolve) => {
  setTimeout(() => { fired++; resolve(); }, false as any);
});
console.log("boolean delay fired:", fired);
