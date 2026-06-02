import * as timers from "node:timers";

let fired = 0;
const timeout: any = timers.setTimeout(() => { fired++; }, 5);
timers.clearTimeout(+timeout);

const timeoutString: any = timers.setTimeout(() => { fired++; }, 5);
timers.clearTimeout(String(+timeoutString) as any);

const interval: any = timers.setInterval(() => { fired++; }, 5);
timers.clearInterval(+interval);

await new Promise<void>((resolve) => timers.setTimeout(() => resolve(), 25));
console.log("fired:", fired);
