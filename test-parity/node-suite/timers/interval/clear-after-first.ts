import { setInterval, clearInterval, setTimeout } from "node:timers";
let count = 0;
await new Promise<void>((resolve) => {
  const id = setInterval(() => {
    count++;
    clearInterval(id);
    setTimeout(() => { console.log("after first interval:", count); resolve(); }, 5);
  }, 0);
});
