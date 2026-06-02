import { setTimeout } from "node:timers";
// `refresh()` reschedules a still-pending timer using its original delay.
// We refresh repeatedly while pending, then wait long enough for it to fire.
let fired = 0;
const timeout = setTimeout(() => { fired++; }, 30);
console.log("refresh returns same:", timeout.refresh() === timeout);
console.log("hasRef while pending:", timeout.hasRef());
await new Promise((resolve) => setTimeout(resolve, 60));
console.log("fired after wait:", fired);
