import { setImmediate, setTimeout } from "node:timers";
const order: string[] = [];
await new Promise<void>((resolve) => {
  setImmediate(() => order.push("immediate"));
  setTimeout(() => order.push("timeout"), 0);
  setTimeout(() => {
    console.log("mixed order length:", order.length);
    console.log("mixed order sorted:", order.slice().sort().join(","));
    resolve();
  }, 5);
});
