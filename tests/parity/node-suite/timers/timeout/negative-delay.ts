import { setTimeout } from "node:timers";
let fired = 0;
await new Promise<void>((resolve) => {
  setTimeout(() => { fired++; resolve(); }, -5);
});
console.log("negative fired:", fired);
