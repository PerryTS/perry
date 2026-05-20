import { setInterval, clearInterval } from "node:timers";
let count = 0;
await new Promise<void>((resolve) => {
  const id = setInterval(() => {
    count++;
    clearInterval(id);
    console.log("negative interval:", count);
    resolve();
  }, -10);
});
