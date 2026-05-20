import { setTimeout, clearTimeout } from "node:timers";
let fired = 0;
const id = setTimeout(() => { fired++; }, 0);
clearTimeout(id);
clearTimeout(id);
await new Promise((resolve) => setTimeout(resolve, 5));
console.log("multiple clear fired:", fired);
