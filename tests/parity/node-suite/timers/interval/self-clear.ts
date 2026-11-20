import { setInterval, clearInterval } from "node:timers";
// Reentrant clearInterval from inside the callback must stop further
// firings cleanly (Node parity).
let count = 0;
await new Promise<void>((resolve) => {
  const id = setInterval(() => {
    count++;
    if (count === 3) {
      clearInterval(id);
      resolve();
    }
  }, 1);
});
console.log("self-clear count:", count);
