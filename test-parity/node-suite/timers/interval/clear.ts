import { setInterval, clearInterval, setTimeout } from "node:timers";
let count = 0;
const id = setInterval(() => { count++; }, 0);
clearInterval(id);
await new Promise((resolve) => setTimeout(resolve, 5));
console.log("cleared interval count:", count);
