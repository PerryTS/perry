import { setInterval, clearTimeout, setTimeout } from "node:timers";
let fired = 0;
const interval = setInterval(() => { fired++; }, 0);
clearTimeout(interval as any);
await new Promise((resolve) => setTimeout(resolve, 5));
console.log("cross clear interval:", fired);
