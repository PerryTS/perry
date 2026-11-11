import { setTimeout } from "node:timers";
// "interval-via-setTimeout" pattern: callback re-schedules itself.
let count = 0;
await new Promise<void>((resolve) => {
  const tick = () => {
    count++;
    if (count < 3) setTimeout(tick, 1);
    else resolve();
  };
  setTimeout(tick, 1);
});
console.log("recursive count:", count);
