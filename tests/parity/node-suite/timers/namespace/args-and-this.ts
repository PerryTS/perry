import * as timers from "node:timers";

const events: string[] = [];

const timeout: any = timers.setTimeout(function (this: any, value: string) {
  events.push("timeout:" + (this === timeout) + ":" + value);
}, 1, "t");

const interval: any = timers.setInterval(function (this: any, value: string) {
  events.push("interval:" + (this === interval) + ":" + value);
  timers.clearInterval(interval);
}, 1, "i");

const immediate: any = timers.setImmediate(function (this: any, value: string) {
  events.push("immediate:" + (this === immediate) + ":" + value);
}, "m");

await new Promise<void>((resolve) => timers.setTimeout(() => resolve(), 20));
console.log(events.sort().join("\n"));
