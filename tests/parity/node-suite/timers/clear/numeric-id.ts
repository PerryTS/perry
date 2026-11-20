import { setTimeout, clearTimeout, setInterval, clearInterval } from "node:timers";
// clearTimeout / clearInterval accept the primitive numeric id (`+handle`),
// not only the Timeout handle object.
let fired = 0;
const t = setTimeout(() => { fired++; }, 20);
clearTimeout(+t);
const iv = setInterval(() => { fired++; }, 5);
clearInterval(+iv);
await new Promise<void>((r) => setTimeout(() => r(), 50));
console.log("cleared by numeric id:", fired === 0);
