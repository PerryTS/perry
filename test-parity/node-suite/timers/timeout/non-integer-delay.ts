import { setTimeout } from "node:timers";
let fired = 0;
await new Promise<void>((resolve) => {
  setTimeout(() => { fired++; }, 0.5);
  setTimeout(() => { fired++; resolve(); }, 1.5);
});
console.log("fired:", fired);
