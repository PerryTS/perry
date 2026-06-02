import { setTimeout, clearTimeout } from "node:timers";
let fired = 0;
const timeout = setTimeout(() => { fired++; }, 0);
await new Promise((resolve) => setTimeout(resolve, 5));
clearTimeout(timeout);
console.log("clear after fired:", fired);
