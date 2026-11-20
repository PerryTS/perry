import { setInterval, clearInterval } from "node:timers";
let count = 0;
await new Promise<void>((resolve) => {
  const id = setInterval(() => {
    count++;
    if (count === 3) {
      clearInterval(id);
      console.log("interval count:", count);
      resolve();
    }
  }, 0);
});
