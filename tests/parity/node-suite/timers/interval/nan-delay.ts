import { setInterval, clearInterval } from "node:timers";
// NaN / Infinity delays clamp to 1ms (Node behavior).
let count = 0;
await new Promise<void>((resolve) => {
  const id = setInterval(() => {
    count++;
    if (count === 2) {
      clearInterval(id);
      resolve();
    }
  }, NaN);
});
console.log("nan-delay count:", count);
