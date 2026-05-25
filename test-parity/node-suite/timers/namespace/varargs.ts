import * as timers from "node:timers";

const events: string[] = [];

const timeout: any = timers.setTimeout(function (this: any, a: string, b: string, c: string) {
  events.push("timeout:" + (this === timeout) + ":" + [a, b, c].join(","));
}, 1, "a", "b", "c");

const interval: any = timers.setInterval(function (this: any, a: string, b: string, c: string) {
  events.push("interval:" + (this === interval) + ":" + [a, b, c].join(","));
  timers.clearInterval(interval);
}, 1, "d", "e", "f");

const immediate: any = timers.setImmediate(function (this: any, a: string, b: string, c: string) {
  events.push("immediate:" + (this === immediate) + ":" + [a, b, c].join(","));
}, "g", "h", "i");

await new Promise<void>((resolve) => timers.setTimeout(() => resolve(), 20));
console.log(events.sort().join("\n"));
