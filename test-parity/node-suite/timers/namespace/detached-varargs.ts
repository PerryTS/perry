import * as timers from "node:timers";

const setTimeoutFn = timers.setTimeout;
const setIntervalFn = timers["setInterval"];
const setImmediateFn = timers.setImmediate;
const clearIntervalFn = timers.clearInterval;

const events: string[] = [];

const timeout: any = setTimeoutFn(function (this: any, a: string, b: string, c: string) {
  events.push("timeout:" + (this === timeout) + ":" + [a, b, c].join(","));
}, 1, "a", "b", "c");

const interval: any = setIntervalFn(function (this: any, a: string, b: string, c: string) {
  events.push("interval:" + (this === interval) + ":" + [a, b, c].join(","));
  clearIntervalFn(interval);
}, 1, "d", "e", "f");

const immediate: any = setImmediateFn(function (this: any, a: string, b: string, c: string) {
  events.push("immediate:" + (this === immediate) + ":" + [a, b, c].join(","));
}, "g", "h", "i");

setTimeoutFn(() => {
  console.log(events.sort().join("\n"));
}, 20);
