import * as timers from "node:timers";
// `import * as timers from "node:timers"` resolves to a namespace whose timer
// functions route to the same global implementations.
console.log("setTimeout:", typeof timers.setTimeout);
console.log("setInterval:", typeof timers.setInterval);
console.log("setImmediate:", typeof timers.setImmediate);
console.log("clearTimeout:", typeof timers.clearTimeout);
console.log("clearInterval:", typeof timers.clearInterval);
console.log("clearImmediate:", typeof timers.clearImmediate);
let fired = 0;
const t = timers.setTimeout(() => { fired++; }, 0);
timers.clearTimeout(t);
const i = timers.setInterval(() => { fired++; }, 0);
timers.clearInterval(i);
await new Promise<void>((r) => timers.setTimeout(() => r(), 20));
console.log("cleared via namespace:", fired === 0);
