import { setTimeout } from "node:timers";
let fired = 0;
const timeout = setTimeout(() => { fired++; }, 0);
console.log("close returns same:", timeout.close() === timeout);
await new Promise((resolve) => setTimeout(resolve, 5));
console.log("close fired:", fired);
